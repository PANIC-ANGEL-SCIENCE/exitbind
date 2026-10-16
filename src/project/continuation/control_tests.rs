//! Synthetic pure decisions. IO adapters still prove persisted claims,
//! canonical Work state, actual owned liveness and durable compare-and-write.
use super::*;
use serde_json::json;

struct Fixture {
    mandate: Mandate,
    paths: Vec<String>,
    commands: Vec<String>,
}

impl Fixture {
    fn new() -> Self {
        let mandate: Mandate = serde_json::from_value(json!({"id":"advance","revision":1,
            "outcome":"Advance the approved project.","mode":"continue","allowedSources":["brief"],
            "write":["src"],"commands":["python3 checks.py"],"roles":["lead","worker","reviewer","adviser"],
            "preserve":["Preserve records."],"priorityPolicy":"Useful small changes first.",
            "expiresAt":"1970-01-01T01:00:00Z","maxWorks":4,"maxActions":20,"maxParallel":2,
            "maxElapsedSeconds":600,"externalPublication":false})).unwrap();
        Self {
            mandate,
            paths: vec!["src/**".into()],
            commands: vec!["python3 checks.py".into()],
        }
    }

    fn policy(&self) -> Policy<'_> {
        Policy {
            project: "project-identity",
            specification_sha256: SPEC,
            mandate: &self.mandate,
        }
    }

    fn facts(&self) -> Facts<'_> {
        Facts {
            now: 1000,
            stopped: false,
            source_current: true,
            configuration_current: true,
            mandate_current: true,
            memory_current: true,
            recipient_role: Role::Worker,
            recipient_write: &self.paths,
            recipient_commands: &self.commands,
            works: &[],
            active_native_operation: None,
            successor_of: None,
            observation: None,
        }
    }

    fn account(&self) -> Account {
        Account::new(&self.policy(), 1000).unwrap()
    }

    fn association(&self, work: &str, task: &str) -> VerifiedAssociation {
        prepare_association(&self.policy(), task, "private-claim-root", work).unwrap()
    }
}

const SPEC: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
const INPUT: &str = "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";

fn work(index: usize) -> String {
    format!("smw_{index:064x}")
}

fn intent(work: &str, kind: Kind) -> Intent {
    Intent {
        kind,
        work: work.into(),
        role: Role::Worker,
        paths: vec!["src/parser.py".into()],
        commands: vec!["python3 checks.py".into()],
        input_sha256: INPUT.into(),
        predecessor: None,
    }
}

fn attempt<'a>(
    account: &Account,
    key: &'a str,
    intent: &'a Intent,
    association: &'a VerifiedAssociation,
) -> Attempt<'a> {
    Attempt {
        expected_revision: account.revision,
        key,
        intent,
        association,
    }
}

fn executed(decision: Result<Decision, Refusal>) -> Account {
    match decision.unwrap() {
        Decision::Execute { next } => next,
        other => panic!("expected one controlled attempt: {other:?}"),
    }
}

fn runtime(association: &VerifiedAssociation) -> VerifiedAssociation {
    verify_association(
        association.association(),
        association.association(),
        association.work(),
    )
    .unwrap()
}

fn observe(
    fixture: &Fixture,
    account: &Account,
    key: &str,
    intent: &Intent,
    association: &VerifiedAssociation,
    outcome: Outcome,
) -> Account {
    let mut facts = fixture.facts();
    facts.observation = Some(Observation {
        operation: key,
        intent,
        outcome,
        known_ended: matches!(outcome, Outcome::Succeeded | Outcome::Failed),
    });
    match evaluate(
        &fixture.policy(),
        &facts,
        account,
        &Request::Observe(attempt(account, key, intent, association)),
    )
    .unwrap()
    {
        Decision::RecordObservation { next } => next,
        other => panic!("expected an observation only: {other:?}"),
    }
}

fn admitted(fixture: &Fixture) -> (Account, VerifiedAssociation, Intent) {
    let work = work(1);
    let association = fixture.association(&work, "task-one");
    let intent = intent(&work, Kind::Admission);
    let original = fixture.account();
    let account = executed(evaluate(
        &fixture.policy(),
        &fixture.facts(),
        &original,
        &Request::Attempt(attempt(&original, "start-one", &intent, &association)),
    ));
    let account = observe(
        fixture,
        &account,
        "start-one",
        &intent,
        &association,
        Outcome::Succeeded,
    );
    (account, runtime(&association), intent)
}

#[test]
fn associations_are_structural_until_the_actual_claim_matches() {
    let fixture = Fixture::new();
    let prepared = fixture.association(&work(1), "task-one");
    prepared.association().validate().unwrap();
    let marker: Association =
        serde_json::from_value(serde_json::to_value(prepared.association()).unwrap()).unwrap();
    let mut different = marker.clone();
    different.task_id = "other-task".into();
    assert!(matches!(
        verify_association(&marker, &different, &work(1)),
        Err(Refusal::AssociationMismatch)
    ));
    let mut malformed = marker.clone();
    malformed.version = 2;
    assert_eq!(malformed.validate(), Err(Refusal::InvalidAssociation));
    assert!(matches!(
        verify_association(&marker, &marker, "caller-label"),
        Err(Refusal::AssociationMismatch)
    ));
    assert!(!runtime(&prepared).prepared);
    assert!(prepared.prepared);
    // No optional/unassociated execution request exists here. The ordinary
    // no-mandate Work route remains the IO adapter's separate existing path.
}

#[test]
fn reservation_precedes_execution_and_failed_start_is_never_refunded_or_restarted() {
    let mut fixture = Fixture::new();
    fixture.mandate.max_actions = 1;
    let initial = fixture.account();
    let association = fixture.association(&work(1), "task-one");
    let intent = intent(&work(1), Kind::Admission);
    let request = Request::Attempt(attempt(&initial, "start-one", &intent, &association));
    let reserved = executed(evaluate(
        &fixture.policy(),
        &fixture.facts(),
        &initial,
        &request,
    ));
    assert_eq!((reserved.actions_spent(), reserved.works_spent()), (1, 1));
    assert_eq!(reserved.operations[0].outcome, Outcome::Reserved);
    let failed = observe(
        &fixture,
        &reserved,
        "start-one",
        &intent,
        &association,
        Outcome::Failed,
    );
    assert_eq!((failed.actions_spent(), failed.works_spent()), (1, 1));
    assert!(matches!(
        evaluate(&fixture.policy(), &fixture.facts(), &failed, &request),
        Ok(Decision::RecoverExisting {
            outcome: Outcome::Failed,
            ..
        })
    ));
    fixture.mandate.revision = 2;
    fixture.mandate.outcome = "A revised description, same original mandate.".into();
    let renamed = fixture.association(&work(2), "renamed-task");
    let fresh = intent_for_other(&intent, &work(2));
    assert_eq!(
        evaluate(
            &fixture.policy(),
            &fixture.facts(),
            &failed,
            &Request::Attempt(attempt(&failed, "different-plan-label", &fresh, &renamed))
        ),
        Err(Refusal::ActionBudget)
    );
    assert_eq!(failed.started_at, 1000);
}

fn intent_for_other(original: &Intent, work: &str) -> Intent {
    let mut next = original.clone();
    next.work = work.into();
    next
}

#[test]
fn every_controlled_native_review_check_mutation_effect_and_publication_attempt_debits() {
    let mut fixture = Fixture::new();
    fixture.mandate.external_publication = true;
    let (mut account, association, _) = admitted(&fixture);
    let observations = [WorkObservation {
        work: association.work(),
        state: WorkState::Active,
    }];
    for (index, (kind, role)) in [
        (Kind::Native, Role::Worker),
        (Kind::Native, Role::Reviewer),
        (Kind::Check, Role::Lead),
        (Kind::Mutation, Role::Worker),
        (Kind::Effect, Role::Worker),
        (Kind::Publication, Role::Lead),
    ]
    .into_iter()
    .enumerate()
    {
        let mut next_intent = intent(association.work(), kind);
        next_intent.role = role;
        let key = format!("attempt-{index}");
        let mut facts = fixture.facts();
        facts.recipient_role = role;
        facts.works = &observations;
        account = executed(evaluate(
            &fixture.policy(),
            &facts,
            &account,
            &Request::Attempt(attempt(&account, &key, &next_intent, &association)),
        ));
        account = observe(
            &fixture,
            &account,
            &key,
            &next_intent,
            &association,
            Outcome::Failed,
        );
        assert_eq!(account.actions_spent(), index as u64 + 2);
    }
    assert_eq!(account.works_spent(), 1);
    assert!(account
        .operations
        .iter()
        .skip(1)
        .all(|o| o.outcome == Outcome::Failed));
}

#[test]
fn exact_replay_is_read_only_and_changed_intent_with_same_key_is_refused() {
    let fixture = Fixture::new();
    let original = fixture.account();
    let association = fixture.association(&work(1), "task-one");
    let intent = intent(&work(1), Kind::Admission);
    let account = executed(evaluate(
        &fixture.policy(),
        &fixture.facts(),
        &original,
        &Request::Attempt(attempt(&original, "same-key", &intent, &association)),
    ));
    assert!(matches!(
        evaluate(
            &fixture.policy(),
            &fixture.facts(),
            &account,
            &Request::Attempt(attempt(&original, "same-key", &intent, &association))
        ),
        Ok(Decision::RecoverExisting {
            outcome: Outcome::Reserved,
            ..
        })
    ));
    let mut changed = intent.clone();
    changed.input_sha256 = SPEC.into();
    assert_eq!(
        evaluate(
            &fixture.policy(),
            &fixture.facts(),
            &account,
            &Request::Attempt(attempt(&account, "same-key", &changed, &association))
        ),
        Err(Refusal::OperationConflict)
    );
    changed = intent.clone();
    changed.commands.clear();
    assert_eq!(
        evaluate(
            &fixture.policy(),
            &fixture.facts(),
            &account,
            &Request::Attempt(attempt(&account, "same-key", &changed, &association))
        ),
        Err(Refusal::OperationConflict)
    );
    assert_eq!(account.actions_spent(), 1);
}

#[test]
fn concurrent_stale_snapshot_cannot_commit_second_reservation() {
    let fixture = Fixture::new();
    let initial = fixture.account();
    let first = fixture.association(&work(1), "task-one");
    let second = fixture.association(&work(2), "task-two");
    let one = intent(&work(1), Kind::Admission);
    let two = intent(&work(2), Kind::Admission);
    let updated = executed(evaluate(
        &fixture.policy(),
        &fixture.facts(),
        &initial,
        &Request::Attempt(attempt(&initial, "one", &one, &first)),
    ));
    assert_eq!(
        evaluate(
            &fixture.policy(),
            &fixture.facts(),
            &updated,
            &Request::Attempt(attempt(&initial, "two", &two, &second))
        ),
        Err(Refusal::StaleAccount)
    );
    assert_eq!(updated.actions_spent(), 1);
}

#[test]
fn unknown_effects_reserve_slot_even_when_terminal_work_is_reported() {
    let mut fixture = Fixture::new();
    fixture.mandate.max_parallel = 1;
    let initial = fixture.account();
    let first = fixture.association(&work(1), "task-one");
    let one = intent(&work(1), Kind::Admission);
    let reserved = executed(evaluate(
        &fixture.policy(),
        &fixture.facts(),
        &initial,
        &Request::Attempt(attempt(&initial, "one", &one, &first)),
    ));
    let unknown = observe(&fixture, &reserved, "one", &one, &first, Outcome::Unknown);
    let second = fixture.association(&work(2), "task-two");
    let two = intent(&work(2), Kind::Admission);
    let observations = [WorkObservation {
        work: first.work(),
        state: WorkState::Ended,
    }];
    let mut facts = fixture.facts();
    facts.works = &observations;
    assert_eq!(
        evaluate(
            &fixture.policy(),
            &facts,
            &unknown,
            &Request::Attempt(attempt(&unknown, "two", &two, &second))
        ),
        Err(Refusal::ConcurrencyBudget)
    );
    let ended = observe(&fixture, &unknown, "one", &one, &first, Outcome::Failed);
    assert!(matches!(
        evaluate(
            &fixture.policy(),
            &facts,
            &ended,
            &Request::Attempt(attempt(&ended, "two", &two, &second))
        ),
        Ok(Decision::Execute { .. })
    ));
    assert_eq!(ended.actions_spent(), 1);
}

#[test]
fn missing_or_uncertain_work_observation_does_not_release_capacity() {
    let mut fixture = Fixture::new();
    fixture.mandate.max_parallel = 1;
    let (account, first, _) = admitted(&fixture);
    let second = fixture.association(&work(2), "task-two");
    let two = intent(&work(2), Kind::Admission);
    assert_eq!(
        evaluate(
            &fixture.policy(),
            &fixture.facts(),
            &account,
            &Request::Attempt(attempt(&account, "two", &two, &second))
        ),
        Err(Refusal::ConcurrencyBudget)
    );
    let observations = [WorkObservation {
        work: first.work(),
        state: WorkState::Unknown,
    }];
    let mut facts = fixture.facts();
    facts.works = &observations;
    assert_eq!(
        evaluate(
            &fixture.policy(),
            &facts,
            &account,
            &Request::Attempt(attempt(&account, "two", &two, &second))
        ),
        Err(Refusal::ConcurrencyBudget)
    );
}

#[test]
fn successor_requires_actual_predecessor_and_consumes_original_aggregate() {
    let mut fixture = Fixture::new();
    fixture.paths = vec!["src/parser.py".into()];
    fixture.mandate.max_works = 2;
    fixture.mandate.max_parallel = 1;
    let (account, first, _) = admitted(&fixture);
    let second = fixture.association(&work(2), "task-one");
    let mut two = intent(&work(2), Kind::Successor);
    two.predecessor = Some(first.work().into());
    let observations = [WorkObservation {
        work: first.work(),
        state: WorkState::Ended,
    }];
    let mut facts = fixture.facts();
    facts.works = &observations;
    assert_eq!(
        evaluate(
            &fixture.policy(),
            &facts,
            &account,
            &Request::Attempt(attempt(&account, "successor", &two, &second))
        ),
        Err(Refusal::InvalidSuccessor)
    );
    facts.successor_of = Some(first.work());
    let next = executed(evaluate(
        &fixture.policy(),
        &facts,
        &account,
        &Request::Attempt(attempt(&account, "successor", &two, &second)),
    ));
    assert_eq!(
        (next.works_spent(), next.actions_spent(), next.started_at),
        (2, 2, 1000)
    );
    let third = fixture.association(&work(3), "third-task");
    let three = intent(&work(3), Kind::Admission);
    assert_eq!(
        evaluate(
            &fixture.policy(),
            &facts,
            &next,
            &Request::Attempt(attempt(&next, "third", &three, &third))
        ),
        Err(Refusal::WorkBudget)
    );
}

#[test]
fn all_freshness_holds_preserve_inspection_and_exact_saved_recovery() {
    let fixture = Fixture::new();
    let (account, association, original) = admitted(&fixture);
    let native = intent(association.work(), Kind::Native);
    let observations = [WorkObservation {
        work: association.work(),
        state: WorkState::Active,
    }];
    for expected in [
        Refusal::Stopped,
        Refusal::SourceDrift,
        Refusal::ConfigurationDrift,
        Refusal::MandateDrift,
        Refusal::MemoryDrift,
        Refusal::Expired,
        Refusal::ElapsedBudget,
        Refusal::ClockRegression,
    ] {
        let mut facts = fixture.facts();
        facts.works = &observations;
        match expected {
            Refusal::Stopped => facts.stopped = true,
            Refusal::SourceDrift => facts.source_current = false,
            Refusal::ConfigurationDrift => facts.configuration_current = false,
            Refusal::MandateDrift => facts.mandate_current = false,
            Refusal::MemoryDrift => facts.memory_current = false,
            Refusal::Expired => facts.now = 3600,
            Refusal::ElapsedBudget => facts.now = 1600,
            Refusal::ClockRegression => facts.now = 999,
            _ => unreachable!(),
        }
        assert_eq!(
            evaluate(
                &fixture.policy(),
                &facts,
                &account,
                &Request::Attempt(attempt(&account, "new-native", &native, &association))
            ),
            Err(expected)
        );
        assert_eq!(
            evaluate(&fixture.policy(), &facts, &account, &Request::Inspect),
            Ok(Decision::Inspect)
        );
        assert!(matches!(
            evaluate(
                &fixture.policy(),
                &facts,
                &account,
                &Request::RecoverSaved {
                    key: "start-one",
                    intent: &original,
                    association: &association
                }
            ),
            Ok(Decision::RecoverExisting { .. })
        ));
    }
}

#[test]
fn revised_policy_never_rebinds_old_frozen_association_or_resets_elapsed_origin() {
    let mut fixture = Fixture::new();
    let (account, association, _) = admitted(&fixture);
    fixture.mandate.revision = 2;
    let native = intent(association.work(), Kind::Native);
    assert_eq!(
        evaluate(
            &fixture.policy(),
            &fixture.facts(),
            &account,
            &Request::Attempt(attempt(&account, "new", &native, &association))
        ),
        Err(Refusal::MandateDrift)
    );
    let revised = fixture.association(&work(2), "renamed-task");
    let two = intent(&work(2), Kind::Admission);
    let mut facts = fixture.facts();
    facts.now = 1600;
    assert_eq!(
        evaluate(
            &fixture.policy(),
            &facts,
            &account,
            &Request::Attempt(attempt(&account, "new-plan-label", &two, &revised))
        ),
        Err(Refusal::ElapsedBudget)
    );
    assert_eq!(account.started_at, 1000);
}

#[test]
fn recipient_and_mandate_commands_paths_roles_and_publication_are_enforced_exactly() {
    let fixture = Fixture::new();
    let (account, association, _) = admitted(&fixture);
    let observations = [WorkObservation {
        work: association.work(),
        state: WorkState::Active,
    }];
    let mut facts = fixture.facts();
    facts.works = &observations;
    let mut native = intent(association.work(), Kind::Native);
    native.commands = vec!["python3 checks.py && publish".into()];
    assert_eq!(
        evaluate(
            &fixture.policy(),
            &facts,
            &account,
            &Request::Attempt(attempt(&account, "command", &native, &association))
        ),
        Err(Refusal::CommandDenied)
    );
    native = intent(association.work(), Kind::Native);
    facts.recipient_commands = &[];
    assert_eq!(
        evaluate(
            &fixture.policy(),
            &facts,
            &account,
            &Request::Attempt(attempt(
                &account,
                "configured-command",
                &native,
                &association
            ))
        ),
        Err(Refusal::CommandDenied)
    );
    facts.recipient_commands = &fixture.commands;
    native.paths = vec!["src-other/parser.py".into()];
    assert_eq!(
        evaluate(
            &fixture.policy(),
            &facts,
            &account,
            &Request::Attempt(attempt(&account, "path", &native, &association))
        ),
        Err(Refusal::PathDenied)
    );
    native = intent(association.work(), Kind::Native);
    facts.recipient_write = &[];
    assert_eq!(
        evaluate(
            &fixture.policy(),
            &facts,
            &account,
            &Request::Attempt(attempt(&account, "configured-path", &native, &association))
        ),
        Err(Refusal::PathDenied)
    );
    facts.recipient_write = &fixture.paths;
    native.role = Role::Reviewer;
    assert_eq!(
        evaluate(
            &fixture.policy(),
            &facts,
            &account,
            &Request::Attempt(attempt(&account, "role", &native, &association))
        ),
        Err(Refusal::RoleDenied)
    );
    native = intent(association.work(), Kind::Publication);
    assert_eq!(
        evaluate(
            &fixture.policy(),
            &facts,
            &account,
            &Request::Attempt(attempt(&account, "publish", &native, &association))
        ),
        Err(Refusal::PublicationDenied)
    );
    native.paths = vec![".git/config".into()];
    assert_eq!(
        evaluate(
            &fixture.policy(),
            &facts,
            &account,
            &Request::Attempt(attempt(&account, "protected", &native, &association))
        ),
        Err(Refusal::InvalidIntent)
    );
}

#[test]
fn only_known_owned_end_can_record_success_and_observation_cannot_rewrite_a_failure() {
    let fixture = Fixture::new();
    let initial = fixture.account();
    let association = fixture.association(&work(1), "task-one");
    let one = intent(&work(1), Kind::Admission);
    let reserved = executed(evaluate(
        &fixture.policy(),
        &fixture.facts(),
        &initial,
        &Request::Attempt(attempt(&initial, "one", &one, &association)),
    ));
    let mut facts = fixture.facts();
    facts.stopped = true;
    facts.observation = Some(Observation {
        operation: "one",
        intent: &one,
        outcome: Outcome::Succeeded,
        known_ended: false,
    });
    assert_eq!(
        evaluate(
            &fixture.policy(),
            &facts,
            &reserved,
            &Request::Observe(attempt(&reserved, "one", &one, &association))
        ),
        Err(Refusal::InvalidObservation)
    );
    facts.observation = Some(Observation {
        operation: "one",
        intent: &one,
        outcome: Outcome::Failed,
        known_ended: true,
    });
    let failed = match evaluate(
        &fixture.policy(),
        &facts,
        &reserved,
        &Request::Observe(attempt(&reserved, "one", &one, &association)),
    )
    .unwrap()
    {
        Decision::RecordObservation { next } => next,
        other => panic!("observation must grant no effect: {other:?}"),
    };
    facts.observation = Some(Observation {
        operation: "one",
        intent: &one,
        outcome: Outcome::Succeeded,
        known_ended: true,
    });
    assert_eq!(
        evaluate(
            &fixture.policy(),
            &facts,
            &failed,
            &Request::Observe(attempt(&failed, "one", &one, &association))
        ),
        Err(Refusal::ObservationConflict)
    );
    assert_eq!(failed.actions_spent(), 1);
}

#[test]
fn propose_only_zero_budget_and_cycle_limits_do_not_create_fresh_permission() {
    let mut fixture = Fixture::new();
    fixture.mandate.mode = Mode::Propose;
    let initial = fixture.account();
    let association = fixture.association(&work(1), "task-one");
    let one = intent(&work(1), Kind::Admission);
    assert_eq!(
        evaluate(
            &fixture.policy(),
            &fixture.facts(),
            &initial,
            &Request::Attempt(attempt(&initial, "one", &one, &association))
        ),
        Err(Refusal::ProposeOnly)
    );
    fixture.mandate.max_actions = 0;
    assert!(matches!(
        Account::new(&fixture.policy(), 1000),
        Err(Refusal::InvalidPolicy)
    ));
    fixture = Fixture::new();
    fixture.mandate.mode = Mode::Cycle;
    let (account, _, _) = admitted(&fixture);
    let second = fixture.association(&work(2), "task-two");
    let two = intent(&work(2), Kind::Admission);
    assert_eq!(
        evaluate(
            &fixture.policy(),
            &fixture.facts(),
            &account,
            &Request::Attempt(attempt(&account, "two", &two, &second))
        ),
        Err(Refusal::WorkBudget)
    );
}

#[test]
fn active_native_allows_only_verified_subordinate_boundaries_and_every_one_debits() {
    let fixture = Fixture::new();
    let (account, association, _) = admitted(&fixture);
    let observations = [WorkObservation {
        work: association.work(),
        state: WorkState::Active,
    }];
    let mut facts = fixture.facts();
    facts.works = &observations;
    let native = intent(association.work(), Kind::Native);
    let mut account = executed(evaluate(
        &fixture.policy(),
        &facts,
        &account,
        &Request::Attempt(attempt(&account, "native-owner", &native, &association)),
    ));
    let mutation = intent(association.work(), Kind::Mutation);
    assert_eq!(
        evaluate(
            &fixture.policy(),
            &facts,
            &account,
            &Request::Attempt(attempt(&account, "mutation", &mutation, &association))
        ),
        Err(Refusal::WorkNotActive)
    );
    facts.active_native_operation = Some("arbitrary-other-operation");
    assert_eq!(
        evaluate(
            &fixture.policy(),
            &facts,
            &account,
            &Request::Attempt(attempt(&account, "mutation", &mutation, &association))
        ),
        Err(Refusal::WorkNotActive)
    );
    facts.active_native_operation = Some("native-owner");
    for (index, kind) in [Kind::Mutation, Kind::Effect, Kind::Check]
        .into_iter()
        .enumerate()
    {
        let subordinate = intent(association.work(), kind);
        let key = format!("subordinate-{index}");
        account = executed(evaluate(
            &fixture.policy(),
            &facts,
            &account,
            &Request::Attempt(attempt(&account, &key, &subordinate, &association)),
        ));
        assert_eq!(account.actions_spent(), index as u64 + 3);
        account = observe(
            &fixture,
            &account,
            &key,
            &subordinate,
            &association,
            Outcome::Succeeded,
        );
    }
    assert_eq!(account.operations[1].outcome, Outcome::Reserved);
    assert_eq!(
        evaluate(
            &fixture.policy(),
            &facts,
            &account,
            &Request::Attempt(attempt(&account, "second-native", &native, &association))
        ),
        Err(Refusal::WorkNotActive)
    );
    let mut reviewer = intent(association.work(), Kind::Mutation);
    reviewer.role = Role::Reviewer;
    facts.recipient_role = Role::Reviewer;
    assert_eq!(
        evaluate(
            &fixture.policy(),
            &facts,
            &account,
            &Request::Attempt(attempt(
                &account,
                "other-recipient",
                &reviewer,
                &association
            ))
        ),
        Err(Refusal::WorkNotActive)
    );
    facts.recipient_role = Role::Worker;
    account = observe(
        &fixture,
        &account,
        "native-owner",
        &native,
        &association,
        Outcome::Unknown,
    );
    assert_eq!(
        evaluate(
            &fixture.policy(),
            &facts,
            &account,
            &Request::Attempt(attempt(&account, "unknown-parent", &mutation, &association))
        ),
        Err(Refusal::WorkNotActive)
    );
}

#[test]
fn imported_runtime_operation_must_keep_its_originating_work_association() {
    let fixture = Fixture::new();
    let (account, association, _) = admitted(&fixture);
    let observations = [WorkObservation {
        work: association.work(),
        state: WorkState::Active,
    }];
    let mut facts = fixture.facts();
    facts.works = &observations;
    let native = intent(association.work(), Kind::Native);
    let account = executed(evaluate(
        &fixture.policy(),
        &facts,
        &account,
        &Request::Attempt(attempt(&account, "native", &native, &association)),
    ));
    let mut account = observe(
        &fixture,
        &account,
        "native",
        &native,
        &association,
        Outcome::Failed,
    );
    account.operations[1].association.task_id = "different-valid-task".into();
    assert_eq!(
        evaluate(
            &fixture.policy(),
            &facts,
            &account,
            &Request::Attempt(attempt(&account, "new-native", &native, &association))
        ),
        Err(Refusal::InvalidAccount)
    );
    assert_eq!(
        evaluate(&fixture.policy(), &facts, &account, &Request::Inspect),
        Ok(Decision::Inspect)
    );
}

#[test]
fn successor_cannot_rebind_task_root_revision_or_specification_in_live_or_saved_accounts() {
    let mut fixture = Fixture::new();
    fixture.paths = vec!["src/parser.py".into()];
    let (account, first, _) = admitted(&fixture);
    let mut successor = intent(&work(2), Kind::Successor);
    successor.predecessor = Some(first.work().into());
    let observations = [WorkObservation {
        work: first.work(),
        state: WorkState::Ended,
    }];
    let mut facts = fixture.facts();
    facts.works = &observations;
    facts.successor_of = Some(first.work());
    let exact = fixture.association(&work(2), "task-one");
    let valid = executed(evaluate(
        &fixture.policy(),
        &facts,
        &account,
        &Request::Attempt(attempt(&account, "same-root", &successor, &exact)),
    ));
    let mut revised = fixture.mandate.clone();
    revised.revision = 2;
    for (mandate, specification_sha256, task, root) in [
        (
            &fixture.mandate,
            SPEC,
            "different-task",
            "private-claim-root",
        ),
        (&fixture.mandate, SPEC, "task-one", "different-claim-root"),
        (&revised, SPEC, "task-one", "private-claim-root"),
        (&fixture.mandate, INPUT, "task-one", "private-claim-root"),
    ] {
        let policy = Policy {
            project: "project-identity",
            specification_sha256,
            mandate,
        };
        let association = prepare_association(&policy, task, root, &work(2)).unwrap();
        assert_eq!(
            evaluate(
                &policy,
                &facts,
                &account,
                &Request::Attempt(attempt(&account, "rebound", &successor, &association))
            ),
            Err(Refusal::InvalidSuccessor)
        );
        let mut imported = valid.clone();
        imported.operations.last_mut().unwrap().association = association.association().clone();
        assert_eq!(validate_account(&imported), Err(Refusal::InvalidAccount));
    }
}

#[test]
fn successor_requires_actual_retirement_and_all_predecessor_effects_known_ended() {
    let mut fixture = Fixture::new();
    fixture.paths = vec!["src/parser.py".into()];
    let (account, first, original) = admitted(&fixture);
    let second = fixture.association(&work(2), "task-one");
    let mut successor = intent(&work(2), Kind::Successor);
    successor.predecessor = Some(first.work().into());
    for state in [WorkState::Active, WorkState::Unknown] {
        let observations = [WorkObservation {
            work: first.work(),
            state,
        }];
        let mut facts = fixture.facts();
        facts.successor_of = Some(first.work());
        facts.works = &observations;
        assert_eq!(
            evaluate(
                &fixture.policy(),
                &facts,
                &account,
                &Request::Attempt(attempt(&account, "replacement", &successor, &second))
            ),
            Err(Refusal::InvalidSuccessor)
        );
    }
    let observations = [WorkObservation {
        work: first.work(),
        state: WorkState::Ended,
    }];
    let mut facts = fixture.facts();
    facts.successor_of = Some(first.work());
    facts.works = &observations;
    for outcome in [Outcome::Reserved, Outcome::Unknown] {
        let mut unsettled = account.clone();
        unsettled.operations[0].outcome = outcome;
        assert_eq!(
            evaluate(
                &fixture.policy(),
                &facts,
                &unsettled,
                &Request::Attempt(attempt(&unsettled, "replacement", &successor, &second))
            ),
            Err(Refusal::InvalidSuccessor)
        );
    }
    assert_eq!(account.operations[0].intent, original);
    assert!(matches!(
        evaluate(
            &fixture.policy(),
            &facts,
            &account,
            &Request::Attempt(attempt(&account, "replacement", &successor, &second))
        ),
        Ok(Decision::Execute { .. })
    ));
}

#[test]
fn configured_write_maxima_accept_globs_without_extending_mandate_or_protected_paths() {
    let mut fixture = Fixture::new();
    fixture.mandate.write.push("src-neighbor".into());
    let (account, association, _) = admitted(&fixture);
    let observations = [WorkObservation {
        work: association.work(),
        state: WorkState::Active,
    }];
    let mut facts = fixture.facts();
    facts.works = &observations;
    let mut mutation = intent(association.work(), Kind::Mutation);
    assert!(matches!(
        evaluate(
            &fixture.policy(),
            &facts,
            &account,
            &Request::Attempt(attempt(
                &account,
                "configured-prefix",
                &mutation,
                &association
            ))
        ),
        Ok(Decision::Execute { .. })
    ));
    mutation.paths = vec!["src-neighbor/parser.py".into()];
    assert_eq!(
        evaluate(
            &fixture.policy(),
            &facts,
            &account,
            &Request::Attempt(attempt(
                &account,
                "configured-neighbor",
                &mutation,
                &association
            ))
        ),
        Err(Refusal::PathDenied)
    );
    let broad = vec!["**".into()];
    facts.recipient_write = &broad;
    assert!(matches!(
        evaluate(
            &fixture.policy(),
            &facts,
            &account,
            &Request::Attempt(attempt(&account, "configured-all", &mutation, &association))
        ),
        Ok(Decision::Execute { .. })
    ));
    mutation.paths = vec!["outside/parser.py".into()];
    assert_eq!(
        evaluate(
            &fixture.policy(),
            &facts,
            &account,
            &Request::Attempt(attempt(
                &account,
                "mandate-outside",
                &mutation,
                &association
            ))
        ),
        Err(Refusal::PathDenied)
    );
    mutation.paths = vec![".git/config".into()];
    assert_eq!(
        evaluate(
            &fixture.policy(),
            &facts,
            &account,
            &Request::Attempt(attempt(&account, "protected", &mutation, &association))
        ),
        Err(Refusal::InvalidIntent)
    );
}

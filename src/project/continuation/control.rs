//! Pure admission/accounting decisions for associated Work only. The adapter
//! supplies validated current observations and persists a returned account
//! under one lock before executing. None of these values authenticate a host,
//! establish Work acceptance or sandbox arbitrary commands.

use super::model::{bounded, contains_path, validate_path, Mandate, Mode};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

pub(crate) const ASSOCIATION_VERSION: u64 = 1;
const ACCOUNT_VERSION: u64 = 1;
const MAX_OPERATIONS: usize = 65_536;

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct Association {
    pub version: u64,
    pub project: String,
    pub mandate_id: String,
    pub mandate_revision: u64,
    pub specification_sha256: String,
    pub task_id: String,
    /// An exact locator identity. The IO adapter verifies its canonical
    /// confinement before following it; this pure layer never resolves it.
    pub claim_root: String,
}

impl Association {
    pub(crate) fn validate(&self) -> Result<(), Refusal> {
        bounded(&self.project, "association project", 256)
            .map_err(|_| Refusal::InvalidAssociation)?;
        bounded(&self.claim_root, "claim root", 1024).map_err(|_| Refusal::InvalidAssociation)?;
        if self.version != ASSOCIATION_VERSION
            || !id(&self.mandate_id)
            || !id(&self.task_id)
            || self.mandate_revision == 0
            || !digest(&self.specification_sha256)
        {
            return Err(Refusal::InvalidAssociation);
        }
        Ok(())
    }
}

/// Cannot be deserialized or constructed from a caller's marker. Only the
/// continuation adapter may prepare an anticipated admission or compare a
/// parsed association with an actual validated persisted claim.
#[derive(Clone, Debug)]
pub(crate) struct VerifiedAssociation {
    association: Association,
    work: String,
    prepared: bool,
}

impl VerifiedAssociation {
    pub(crate) fn association(&self) -> &Association {
        &self.association
    }

    pub(crate) fn work(&self) -> &str {
        &self.work
    }
}

/// Use only after the adapter validated the current owner selection and its
/// deterministic anticipated claim. The returned association must be recorded
/// atomically with the reservation before creating the Work.
pub(super) fn prepare_association(
    policy: &Policy<'_>,
    task_id: &str,
    claim_root: &str,
    work: &str,
) -> Result<VerifiedAssociation, Refusal> {
    validate_policy(policy)?;
    let association = Association {
        version: ASSOCIATION_VERSION,
        project: policy.project.into(),
        mandate_id: policy.mandate.id.clone(),
        mandate_revision: policy.mandate.revision,
        specification_sha256: policy.specification_sha256.into(),
        task_id: task_id.into(),
        claim_root: claim_root.into(),
    };
    association.validate()?;
    if !work_id(work) {
        return Err(Refusal::InvalidAssociation);
    }
    Ok(VerifiedAssociation {
        association,
        work: work.into(),
        prepared: true,
    })
}

/// `actual_claim` and `work` come from the IO owner's validated claim and
/// exact Work/start readback, never from another caller-supplied marker.
pub(super) fn verify_association(
    marker: &Association,
    actual_claim: &Association,
    work: &str,
) -> Result<VerifiedAssociation, Refusal> {
    marker.validate()?;
    actual_claim.validate()?;
    if marker != actual_claim || !work_id(work) {
        return Err(Refusal::AssociationMismatch);
    }
    Ok(VerifiedAssociation {
        association: marker.clone(),
        work: work.into(),
        prepared: false,
    })
}

pub(crate) struct Policy<'a> {
    pub project: &'a str,
    pub specification_sha256: &'a str,
    pub mandate: &'a Mandate,
}

#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub(crate) enum Role {
    Lead,
    Worker,
    Reviewer,
    Adviser,
}

impl Role {
    fn name(self) -> &'static str {
        match self {
            Self::Lead => "lead",
            Self::Worker => "worker",
            Self::Reviewer => "reviewer",
            Self::Adviser => "adviser",
        }
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub(crate) enum Kind {
    Admission,
    Successor,
    Native,
    Check,
    Mutation,
    Effect,
    Publication,
}

impl Kind {
    fn starts_work(self) -> bool {
        matches!(self, Self::Admission | Self::Successor)
    }
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct Intent {
    pub kind: Kind,
    pub work: String,
    pub role: Role,
    pub paths: Vec<String>,
    pub commands: Vec<String>,
    /// Digest of the owning boundary's complete actual execution parameters,
    /// including any model/runtime/check/grant binding relevant to that call.
    pub input_sha256: String,
    #[serde(deserialize_with = "present_option")]
    pub predecessor: Option<String>,
}

#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub(crate) enum Outcome {
    Reserved,
    Unknown,
    Succeeded,
    Failed,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct Operation {
    pub key: String,
    pub intent: Intent,
    pub association: Association,
    pub outcome: Outcome,
}

/// Stable mandate lifetime. Policy revisions, plan names and failed attempts
/// never replace this account, its origin or its original operations.
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct Account {
    pub version: u64,
    pub revision: u64,
    pub project: String,
    pub mandate_id: String,
    pub started_at: i64,
    pub operations: Vec<Operation>,
}

impl Account {
    /// The adapter calls this only for an absent account under its creation
    /// lock, with the original selected time. Existing history is never reset.
    pub(crate) fn new(policy: &Policy<'_>, started_at: i64) -> Result<Self, Refusal> {
        validate_policy(policy)?;
        Ok(Self {
            version: ACCOUNT_VERSION,
            revision: 0,
            project: policy.project.into(),
            mandate_id: policy.mandate.id.clone(),
            started_at,
            operations: Vec::new(),
        })
    }

    pub(crate) fn actions_spent(&self) -> u64 {
        self.operations.len() as u64
    }

    pub(crate) fn works_spent(&self) -> u64 {
        self.operations
            .iter()
            .filter(|o| o.intent.kind.starts_work())
            .count() as u64
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum WorkState {
    Active,
    Ended,
    Unknown,
}

pub(crate) struct WorkObservation<'a> {
    pub work: &'a str,
    /// Validated canonical Work/lineage and owned-effects observation. Ended
    /// means canonical termination OR durable verified predecessor retirement,
    /// with every owned effect known ended. A caller completion claim is not
    /// retirement; the existing successor owner must verify the exact claim.
    pub state: WorkState,
}

pub(crate) struct Observation<'a> {
    pub operation: &'a str,
    pub intent: &'a Intent,
    pub outcome: Outcome,
    /// The IO owner verified the original process/effect identity, not merely
    /// that a PID disappeared or a caller reported completion.
    pub known_ended: bool,
}

pub(crate) struct Facts<'a> {
    pub now: i64,
    pub stopped: bool,
    pub source_current: bool,
    pub configuration_current: bool,
    pub mandate_current: bool,
    pub memory_current: bool,
    pub recipient_role: Role,
    pub recipient_write: &'a [String],
    pub recipient_commands: &'a [String],
    pub works: &'a [WorkObservation<'a>],
    /// Supplied only after the IO owner verifies the current native journal's
    /// original owned process/assignment identity is alive for this Work and
    /// recipient. A caller string, matching PID or stored "running" label is
    /// insufficient. This permits subordinate controlled boundaries, never a
    /// second native launch or recovery of unknown effects.
    pub active_native_operation: Option<&'a str>,
    /// Exact predecessor already validated by the existing successor owner.
    pub successor_of: Option<&'a str>,
    pub observation: Option<Observation<'a>>,
}

pub(crate) struct Attempt<'a> {
    pub expected_revision: u64,
    pub key: &'a str,
    pub intent: &'a Intent,
    pub association: &'a VerifiedAssociation,
}

pub(crate) enum Request<'a> {
    Inspect,
    RecoverSaved {
        key: &'a str,
        intent: &'a Intent,
        association: &'a VerifiedAssociation,
    },
    Attempt(Attempt<'a>),
    Observe(Attempt<'a>),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum Decision {
    Inspect,
    /// Read-only routing only; this never authorizes another provider call,
    /// check, permit or effect, including when the old result is unavailable.
    RecoverExisting {
        operation: String,
        outcome: Outcome,
    },
    /// Persist by expected revision under the account lock BEFORE one effect.
    Execute {
        next: Account,
    },
    /// Persist an observed outcome; grants no execution or Work acceptance.
    RecordObservation {
        next: Account,
    },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Refusal {
    InvalidPolicy,
    InvalidAssociation,
    AssociationMismatch,
    InvalidAccount,
    InvalidIntent,
    InvalidObservation,
    ObservationConflict,
    StaleAccount,
    OperationConflict,
    MissingOperation,
    WorkAlreadyAdmitted,
    MissingWork,
    UnverifiedClaim,
    InvalidSuccessor,
    ProposeOnly,
    Stopped,
    Expired,
    ClockRegression,
    ElapsedBudget,
    SourceDrift,
    ConfigurationDrift,
    MandateDrift,
    MemoryDrift,
    ActionBudget,
    WorkBudget,
    ConcurrencyBudget,
    WorkNotActive,
    RoleDenied,
    PathDenied,
    CommandDenied,
    PublicationDenied,
}

/// Pure decisions are conditional on actual IO-owner observations. The caller
/// must re-read the account/current sources under its lock and compare the
/// expected revision before persisting `next`; a preview is never a grant.
pub(crate) fn evaluate(
    policy: &Policy<'_>,
    facts: &Facts<'_>,
    account: &Account,
    request: &Request<'_>,
) -> Result<Decision, Refusal> {
    if matches!(request, Request::Inspect) {
        return Ok(Decision::Inspect);
    }
    validate_account(account)?;
    if account.project != policy.project || account.mandate_id != policy.mandate.id {
        return Err(Refusal::AssociationMismatch);
    }
    let (key, intent, association, expected_revision) = match request {
        Request::Inspect => unreachable!(),
        Request::RecoverSaved {
            key,
            intent,
            association,
        } => (*key, *intent, *association, None),
        Request::Attempt(a) | Request::Observe(a) => {
            (a.key, a.intent, a.association, Some(a.expected_revision))
        }
    };
    validate_intent(intent)?;
    if !operation_key(key) || association.work != intent.work {
        return Err(Refusal::InvalidIntent);
    }
    association.association.validate()?;
    if association.association.project != account.project
        || association.association.mandate_id != account.mandate_id
    {
        return Err(Refusal::AssociationMismatch);
    }
    let existing = account.operations.iter().find(|o| o.key == key);
    if let Some(existing) = existing {
        if existing.intent != *intent || existing.association != association.association {
            return Err(Refusal::OperationConflict);
        }
        if !matches!(request, Request::Observe(_)) {
            return Ok(Decision::RecoverExisting {
                operation: key.into(),
                outcome: existing.outcome,
            });
        }
    } else if !matches!(request, Request::Attempt(_)) {
        return Err(Refusal::MissingOperation);
    }
    if expected_revision != Some(account.revision) {
        return Err(Refusal::StaleAccount);
    }
    if matches!(request, Request::Observe(_)) {
        return record_observation(facts, account, existing.ok_or(Refusal::MissingOperation)?);
    }
    validate_policy(policy)?;
    eligibility(policy, facts, account, &association.association)?;
    if policy.mandate.mode == Mode::Propose {
        return Err(Refusal::ProposeOnly);
    }
    if intent.role != facts.recipient_role
        || !policy.mandate.roles.iter().any(|r| r == intent.role.name())
    {
        return Err(Refusal::RoleDenied);
    }
    for path in &intent.paths {
        if !policy
            .mandate
            .write
            .iter()
            .any(|scope| contains_path(scope, path))
            || !facts
                .recipient_write
                .iter()
                .any(|scope| crate::config::boundary::maximum_contains(scope, path))
        {
            return Err(Refusal::PathDenied);
        }
    }
    if intent.commands.iter().any(|command| {
        !policy.mandate.commands.contains(command) || !facts.recipient_commands.contains(command)
    }) {
        return Err(Refusal::CommandDenied);
    }
    if intent.kind == Kind::Publication && !policy.mandate.external_publication {
        return Err(Refusal::PublicationDenied);
    }
    if account.actions_spent() >= policy.mandate.max_actions {
        return Err(Refusal::ActionBudget);
    }
    let admitted = account
        .operations
        .iter()
        .find(|o| o.intent.kind.starts_work() && o.intent.work == intent.work);
    if intent.kind.starts_work() {
        if !association.prepared {
            return Err(Refusal::UnverifiedClaim);
        }
        if admitted.is_some() {
            return Err(Refusal::WorkAlreadyAdmitted);
        }
        let maximum = if policy.mandate.mode == Mode::Cycle {
            1
        } else {
            policy.mandate.max_works
        };
        if account.works_spent() >= maximum {
            return Err(Refusal::WorkBudget);
        }
        if intent.kind == Kind::Successor {
            let predecessor = intent
                .predecessor
                .as_deref()
                .ok_or(Refusal::InvalidSuccessor)?;
            let original = account
                .operations
                .iter()
                .find(|o| o.intent.kind.starts_work() && o.intent.work == predecessor)
                .ok_or(Refusal::InvalidSuccessor)?;
            if facts.successor_of != Some(predecessor)
                || original.association != association.association
                || observed_work(facts, predecessor)? != WorkState::Ended
                || account.operations.iter().any(|o| {
                    o.intent.work == predecessor
                        && matches!(o.outcome, Outcome::Reserved | Outcome::Unknown)
                })
            {
                return Err(Refusal::InvalidSuccessor);
            }
        }
        if live_slots(account, facts)? >= policy.mandate.max_parallel {
            return Err(Refusal::ConcurrencyBudget);
        }
    } else {
        let admitted = admitted.ok_or(Refusal::MissingWork)?;
        if association.prepared || admitted.association != association.association {
            return Err(Refusal::UnverifiedClaim);
        }
        if observed_work(facts, &intent.work)? != WorkState::Active {
            return Err(Refusal::WorkNotActive);
        }
        if account.operations.iter().any(|o| {
            o.intent.work == intent.work
                && match o.outcome {
                    Outcome::Unknown => true,
                    Outcome::Reserved => {
                        !(o.intent.kind == Kind::Native
                            && o.intent.role == intent.role
                            && facts.active_native_operation == Some(o.key.as_str())
                            && matches!(intent.kind, Kind::Check | Kind::Mutation | Kind::Effect))
                    }
                    Outcome::Succeeded | Outcome::Failed => false,
                }
        }) {
            return Err(Refusal::WorkNotActive);
        }
    }
    let mut next = account.clone();
    next.revision = next
        .revision
        .checked_add(1)
        .ok_or(Refusal::InvalidAccount)?;
    next.operations.push(Operation {
        key: key.into(),
        intent: intent.clone(),
        association: association.association.clone(),
        outcome: Outcome::Reserved,
    });
    Ok(Decision::Execute { next })
}

fn eligibility(
    policy: &Policy<'_>,
    facts: &Facts<'_>,
    account: &Account,
    association: &Association,
) -> Result<(), Refusal> {
    if facts.stopped {
        return Err(Refusal::Stopped);
    }
    if !facts.source_current {
        return Err(Refusal::SourceDrift);
    }
    if !facts.configuration_current {
        return Err(Refusal::ConfigurationDrift);
    }
    if !facts.mandate_current
        || association.mandate_revision != policy.mandate.revision
        || association.specification_sha256 != policy.specification_sha256
    {
        return Err(Refusal::MandateDrift);
    }
    if !facts.memory_current {
        return Err(Refusal::MemoryDrift);
    }
    let expiry = chrono::DateTime::parse_from_rfc3339(&policy.mandate.expires_at)
        .map_err(|_| Refusal::InvalidPolicy)?
        .timestamp();
    if facts.now >= expiry {
        return Err(Refusal::Expired);
    }
    let elapsed = facts
        .now
        .checked_sub(account.started_at)
        .ok_or(Refusal::ClockRegression)?;
    if elapsed < 0 {
        return Err(Refusal::ClockRegression);
    }
    if elapsed as u64 >= policy.mandate.max_elapsed_seconds {
        return Err(Refusal::ElapsedBudget);
    }
    Ok(())
}

fn record_observation(
    facts: &Facts<'_>,
    account: &Account,
    existing: &Operation,
) -> Result<Decision, Refusal> {
    let observed = facts
        .observation
        .as_ref()
        .ok_or(Refusal::InvalidObservation)?;
    if observed.operation != existing.key
        || *observed.intent != existing.intent
        || observed.outcome == Outcome::Reserved
        || (matches!(observed.outcome, Outcome::Succeeded | Outcome::Failed)
            && !observed.known_ended)
    {
        return Err(Refusal::InvalidObservation);
    }
    if existing.outcome == observed.outcome {
        return Ok(Decision::RecoverExisting {
            operation: existing.key.clone(),
            outcome: existing.outcome,
        });
    }
    if matches!(existing.outcome, Outcome::Succeeded | Outcome::Failed) {
        return Err(Refusal::ObservationConflict);
    }
    let mut next = account.clone();
    next.revision = next
        .revision
        .checked_add(1)
        .ok_or(Refusal::InvalidAccount)?;
    next.operations
        .iter_mut()
        .find(|o| o.key == existing.key)
        .ok_or(Refusal::MissingOperation)?
        .outcome = observed.outcome;
    Ok(Decision::RecordObservation { next })
}

fn observed_work(facts: &Facts<'_>, work: &str) -> Result<WorkState, Refusal> {
    let mut matching = facts.works.iter().filter(|f| f.work == work);
    let state = matching.next().map_or(WorkState::Unknown, |f| f.state);
    if matching.next().is_some() {
        return Err(Refusal::InvalidObservation);
    }
    Ok(state)
}

fn live_slots(account: &Account, facts: &Facts<'_>) -> Result<u64, Refusal> {
    let mut slots = 0;
    for work in account
        .operations
        .iter()
        .filter(|o| o.intent.kind.starts_work())
        .map(|o| o.intent.work.as_str())
    {
        if observed_work(facts, work)? != WorkState::Ended
            || account.operations.iter().any(|o| {
                o.intent.work == work && matches!(o.outcome, Outcome::Reserved | Outcome::Unknown)
            })
        {
            slots += 1;
        }
    }
    Ok(slots)
}

fn validate_account(account: &Account) -> Result<(), Refusal> {
    if account.version != ACCOUNT_VERSION
        || !id(&account.mandate_id)
        || bounded(&account.project, "account project", 256).is_err()
        || account.operations.len() > MAX_OPERATIONS
        || account.revision < account.operations.len() as u64
    {
        return Err(Refusal::InvalidAccount);
    }
    let mut keys = BTreeSet::new();
    let mut admitted = BTreeMap::new();
    for op in &account.operations {
        op.association
            .validate()
            .map_err(|_| Refusal::InvalidAccount)?;
        validate_intent(&op.intent).map_err(|_| Refusal::InvalidAccount)?;
        if !operation_key(&op.key)
            || !keys.insert(&op.key)
            || op.association.project != account.project
            || op.association.mandate_id != account.mandate_id
        {
            return Err(Refusal::InvalidAccount);
        }
        if op.intent.kind.starts_work() {
            if admitted.contains_key(op.intent.work.as_str()) {
                return Err(Refusal::InvalidAccount);
            }
            if let Some(predecessor) = &op.intent.predecessor {
                if admitted.get(predecessor.as_str()) != Some(&&op.association) {
                    return Err(Refusal::InvalidAccount);
                }
            }
            admitted.insert(op.intent.work.as_str(), &op.association);
        } else if admitted.get(op.intent.work.as_str()) != Some(&&op.association) {
            return Err(Refusal::InvalidAccount);
        }
    }
    Ok(())
}

fn validate_policy(policy: &Policy<'_>) -> Result<(), Refusal> {
    let m = policy.mandate;
    if bounded(policy.project, "policy project", 256).is_err()
        || !digest(policy.specification_sha256)
        || !id(&m.id)
        || m.revision == 0
        || !(1..=64).contains(&m.max_works)
        || !(1..=MAX_OPERATIONS as u64).contains(&m.max_actions)
        || m.max_parallel == 0
        || m.max_parallel > m.max_works
        || !(1..=31_536_000).contains(&m.max_elapsed_seconds)
        || chrono::DateTime::parse_from_rfc3339(&m.expires_at).is_err()
        || m.write.len() > 32
        || m.write.iter().any(|p| validate_path(p).is_err())
        || m.commands.is_empty()
        || m.commands.len() > 32
        || m.commands
            .iter()
            .any(|c| bounded(c, "command", 4096).is_err())
        || m.roles.is_empty()
        || m.roles.len() > 4
        || m.roles
            .iter()
            .any(|r| !matches!(r.as_str(), "lead" | "worker" | "reviewer" | "adviser"))
    {
        return Err(Refusal::InvalidPolicy);
    }
    Ok(())
}

fn validate_intent(intent: &Intent) -> Result<(), Refusal> {
    if !work_id(&intent.work)
        || !digest(&intent.input_sha256)
        || intent.paths.len() > 32
        || intent.commands.len() > 32
        || intent.paths.iter().any(|p| validate_path(p).is_err())
        || intent
            .commands
            .iter()
            .any(|c| bounded(c, "command", 4096).is_err())
        || intent.paths.iter().collect::<BTreeSet<_>>().len() != intent.paths.len()
        || intent.commands.iter().collect::<BTreeSet<_>>().len() != intent.commands.len()
        || (intent.kind == Kind::Successor) != intent.predecessor.is_some()
        || intent
            .predecessor
            .as_ref()
            .is_some_and(|p| !work_id(p) || p == &intent.work)
    {
        return Err(Refusal::InvalidIntent);
    }
    Ok(())
}

fn operation_key(key: &str) -> bool {
    bounded(key, "operation key", 120).is_ok()
}
fn id(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 64
        && value
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b"_-".contains(&b))
}
fn digest(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}
fn work_id(value: &str) -> bool {
    value.strip_prefix("smw_").is_some_and(digest)
}
fn present_option<'de, D: serde::Deserializer<'de>>(
    deserializer: D,
) -> Result<Option<String>, D::Error> {
    Option::<String>::deserialize(deserializer)
}

#[cfg(test)]
#[path = "control_tests.rs"]
mod tests;

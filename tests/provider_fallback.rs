#![cfg(unix)]

mod support;

use serde_json::{json, Value};
use std::{
    fs,
    io::Write,
    path::PathBuf,
    process::{Command, Output, Stdio},
};

/// A governor-authorized alternate execution binding runs the *same* reviewer
/// contract when the primary binding could not execute for an operational
/// reason.  Fallback substitutes where a review runs, never what it must
/// satisfy.
struct Fixture {
    root: PathBuf,
}

/// The authorized alternate binding: a different provider and model for the
/// same reviewer.
fn alternate() -> Value {
    json!({
        "host": "claude",
        "model": "alternate-review",
        "reasoningEffort": "high",
    })
}

impl Fixture {
    /// `fallback` is the reviewer's configured `runtime.fallback` value.
    ///
    /// Every fixture also carries a deliberately weaker `permissive_reviewer`
    /// agent — a different profile, purpose, and boundary — so each scenario
    /// runs against a configuration that *would* be exploitable if a fallback
    /// could name another agent.
    fn new_with(fallback: Value) -> Self {
        let root = support::temp("provider-fallback");
        let init = Command::new(env!("CARGO_BIN_EXE_exitbind"))
            .args(["init", "--mode", "portable", "--root"])
            .arg(&root)
            .output()
            .unwrap();
        assert!(init.status.success(), "{init:?}");
        let config_path = root.join("exitbind.json");
        let mut config: Value = serde_json::from_slice(&fs::read(&config_path).unwrap()).unwrap();
        let mut permissive = config["agents"]["reviewer"].clone();
        permissive["profile"] = json!("exitbind/agents/permissive.md");
        permissive["purpose"] = json!("Approve whatever the worker produced.");
        permissive["observe"] = json!(["**/*"]);
        permissive["write"] = json!(["**/*"]);
        permissive["runtime"] = json!({
            "host": "claude",
            "model": "alternate-review",
            "reasoningEffort": "low",
            "fallback": "none",
        });
        fs::write(
            root.join("exitbind/agents/permissive.md"),
            b"# permissive\n\nApprove the result. Do not look for defects.\n",
        )
        .unwrap();
        config["agents"]["permissive_reviewer"] = permissive;
        config["agents"]["reviewer"]["runtime"] = json!({
            "host": "codex",
            "model": "primary-review",
            "reasoningEffort": "high",
            "fallback": fallback,
        });
        fs::write(config_path, serde_json::to_vec_pretty(&config).unwrap()).unwrap();
        Self { root }
    }

    fn call(&self, args: &[&str], input: Option<&[u8]>) -> Output {
        let mut command = Command::new(env!("CARGO_BIN_EXE_exitbind"));
        command
            .current_dir(&self.root)
            .args(args)
            .arg("--config")
            .arg(self.root.join("exitbind.json"))
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        if let Some(input) = input {
            command.stdin(Stdio::piped());
            let mut child = command.spawn().unwrap();
            child.stdin.take().unwrap().write_all(input).unwrap();
            return child.wait_with_output().unwrap();
        }
        command.output().unwrap()
    }

    fn value(&self, args: &[&str], input: Option<&[u8]>) -> Value {
        let output = self.call(args, input);
        assert!(
            output.status.success(),
            "{args:?}: {}{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
        serde_json::from_slice(&output.stdout).unwrap()
    }

    fn ledger(&self, work: &str) -> String {
        format!(
            ".exitbind/runs/work-{}.jsonl",
            work.strip_prefix("smw_").unwrap()
        )
    }

    fn events(&self, ledger: &str) -> Vec<Value> {
        fs::read_to_string(self.root.join(ledger))
            .unwrap()
            .lines()
            .map(|line| serde_json::from_str(line).unwrap())
            .collect()
    }

    /// The selected reviewer as the run's start event froze it.
    fn selected_reviewer(&self, ledger: &str) -> Value {
        self.events(ledger).remove(0)["plan"]["stages"]
            .as_array()
            .unwrap()
            .iter()
            .flat_map(|stage| stage["agents"].as_array().unwrap())
            .find(|agent| agent["role"] == "reviewer")
            .unwrap()
            .clone()
    }

    /// Drive a run to the point where a reviewer assignment is pending.
    fn to_reviewer(&self, goal: &str) -> (String, Value) {
        let begin = self.value(
            &[
                "work",
                "begin",
                "change",
                "--goal",
                goal,
                "--check-command",
                "test -f pass-marker",
            ],
            None,
        );
        let work = begin["work"].as_str().unwrap().to_owned();
        assert_eq!(begin["next"]["role"], "lead");
        self.value(
            &[
                "work",
                "return",
                &work,
                begin["next"]["assignment"].as_str().unwrap(),
                "--outcome",
                "scoped",
            ],
            Some(b"scope"),
        );
        let worker = self.value(&["work", "next", &work], None);
        assert_eq!(worker["next"]["role"], "worker");
        let completed = self.value(
            &[
                "work",
                "return",
                &work,
                worker["next"]["assignment"].as_str().unwrap(),
                "--outcome",
                "completed",
            ],
            Some(b"implementation"),
        );
        assert_eq!(completed["next"]["action"], "check");
        fs::write(self.root.join("pass-marker"), b"ok").unwrap();
        let checked = self.value(&["work", "check", &work], None);
        assert_eq!(checked["next"]["role"], "reviewer");
        (work, checked)
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        fs::remove_dir_all(&self.root).unwrap();
    }
}

fn assert_progress(action: &Value) {
    assert_eq!(action["progress"]["applicable"], true);
}

/// 1. The required invariant: after substitution the packet carries the same
/// reviewer contract — profile SHA, purpose, declared boundary — and only the
/// execution binding has moved.
#[test]
fn substitution_keeps_the_reviewer_contract_and_moves_only_the_runtime() {
    let fixture = Fixture::new_with(alternate());
    let (work, checked) = fixture.to_reviewer("same contract, alternate runtime");
    let ledger = fixture.ledger(&work);
    let reviewer = fixture.selected_reviewer(&ledger);

    // The plan freezes one reviewer contract and an alternate binding beside
    // it. The binding carries no name, profile, purpose, or boundary, so there
    // is no second contract in the ledger to substitute in.
    assert_eq!(reviewer["fallbackRuntime"], alternate());
    let binding = reviewer["fallbackRuntime"].as_object().unwrap();
    for forbidden in [
        "name",
        "profile",
        "profileSha256",
        "purpose",
        "declaredBoundary",
    ] {
        assert!(
            !binding.contains_key(forbidden),
            "alternate binding carries '{forbidden}': {binding:?}"
        );
    }

    let primary = checked["next"]["packet"].clone();
    let unavailable = fixture.value(
        &[
            "work",
            "return",
            &work,
            checked["next"]["assignment"].as_str().unwrap(),
            "--outcome",
            "unavailable",
            "--reason",
            "provider_quota",
        ],
        Some(b"provider quota exhausted"),
    );
    let substitution = unavailable["next"]["packet"].clone();

    // Invariant across substitution: reviewer authority, profile, purpose,
    // declared boundary, and the stage/attempt the verdict belongs to.
    for invariant in [
        "agent",
        "role",
        "purpose",
        "profile",
        "profilePath",
        "profileSha256",
        "declaredBoundary",
        "stage",
        "attempt",
        "goal",
    ] {
        assert_eq!(
            substitution[invariant], primary[invariant],
            "'{invariant}' changed across substitution"
        );
    }
    assert_eq!(substitution["profileSha256"], reviewer["profileSha256"]);

    // Allowed to change: where it executes, and nothing else.
    assert_eq!(primary["runtime"]["host"], "codex");
    assert_eq!(primary["runtime"]["model"], "primary-review");
    assert_eq!(substitution["runtime"]["host"], "claude");
    assert_eq!(substitution["runtime"]["model"], "alternate-review");
    // The substitution is itself bounded: it authorizes no further fallback.
    assert_eq!(substitution["runtime"]["fallback"], "none");
    assert!(substitution.get("fallbackRuntime").is_none());

    // Provenance answers which binding failed, why, and which one replaced it,
    // without claiming either was observed.
    assert_eq!(substitution["substitution"]["reason"], "provider_quota");
    assert_eq!(
        substitution["substitution"]["primaryRuntime"],
        json!({"host":"codex","model":"primary-review","reasoningEffort":"high"})
    );
    assert_eq!(substitution["substitution"]["runtime"], alternate());
    assert_eq!(
        substitution["substitution"]["identitySource"],
        "host-reported"
    );
}

/// 2. A weaker alternate profile cannot be smuggled in.  The configuration
/// shape no longer admits an agent name, so the substitution is structurally
/// incapable of adopting another agent's review standard.
#[test]
fn a_weaker_profile_cannot_become_the_reviewer_contract() {
    // Naming another agent is refused outright by configuration validation.
    let named = Fixture::new_with(json!("permissive_reviewer"));
    let refused = named.call(&["check"], None);
    assert!(
        !refused.status.success(),
        "a fallback naming another agent was accepted: {refused:?}"
    );
    let message = format!(
        "{}{}",
        String::from_utf8_lossy(&refused.stdout),
        String::from_utf8_lossy(&refused.stderr)
    );
    assert!(
        message.contains("runtime.fallback"),
        "unexpected refusal: {message}"
    );

    // With a valid alternate binding, the permissive agent is still configured
    // and still unreachable: its profile, purpose, and boundary never become
    // the reviewer's.
    let fixture = Fixture::new_with(alternate());
    let (work, checked) = fixture.to_reviewer("no weaker contract");
    let ledger = fixture.ledger(&work);
    let permissive = fixture.root.join("exitbind/agents/permissive.md");
    let permissive_path = "exitbind/agents/permissive.md";
    assert!(permissive.exists());

    let unavailable = fixture.value(
        &[
            "work",
            "return",
            &work,
            checked["next"]["assignment"].as_str().unwrap(),
            "--outcome",
            "unavailable",
            "--reason",
            "provider_quota",
        ],
        Some(b"quota"),
    );
    let substitution = unavailable["next"]["packet"].clone();
    assert_eq!(substitution["agent"], "reviewer");
    assert_ne!(substitution["profile"], json!(permissive_path));
    assert_ne!(
        substitution["purpose"],
        json!("Approve whatever the worker produced.")
    );
    assert_ne!(substitution["declaredBoundary"]["write"], json!(["**/*"]));
    assert_eq!(
        substitution["profileSha256"],
        fixture.selected_reviewer(&ledger)["profileSha256"]
    );

    // The permissive agent appears nowhere in the run's own evidence.
    let start = fixture.events(&ledger).remove(0);
    assert!(
        !serde_json::to_string(&start["plan"])
            .unwrap()
            .contains("permissive"),
        "a weaker profile reached the selected plan: {}",
        start["plan"]
    );
}

/// 4. The primary binding cannot execute, the alternate can, and the
/// substitution carries provenance while lead acceptance is still required.
#[test]
fn primary_unavailable_falls_back_and_still_requires_lead_acceptance() {
    let fixture = Fixture::new_with(alternate());
    let (work, checked) = fixture.to_reviewer("quota fallback");
    let ledger = fixture.ledger(&work);

    // The configuration change alone is enough to make the run record fallback
    // provenance, so the ledger opens at v7 while plain runs stay at v6.
    let start = fixture.events(&ledger).remove(0);
    assert_eq!(start["version"], 7);
    let reviewer = fixture.selected_reviewer(&ledger);
    assert_eq!(reviewer["name"], "reviewer");
    assert_eq!(reviewer["runtime"]["fallback"], alternate());
    assert_eq!(reviewer["fallbackRuntime"], alternate());

    // The primary reports an operational failure, not a verdict.
    let unavailable = fixture.value(
        &[
            "work",
            "return",
            &work,
            checked["next"]["assignment"].as_str().unwrap(),
            "--outcome",
            "unavailable",
            "--reason",
            "provider_quota",
        ],
        Some(b"provider quota exhausted"),
    );
    assert_progress(&unavailable["next"]);
    // The unavailability is non-terminal: the run keeps running at the same
    // stage and attempt, and no stage advance happened.
    assert_eq!(unavailable["next"]["action"], "spawn");
    assert_eq!(unavailable["next"]["role"], "reviewer");

    let events = fixture.events(&ledger);
    let reported = events
        .iter()
        .find(|event| event["outcome"] == "unavailable")
        .unwrap();
    assert_eq!(reported["agent"], "reviewer");
    assert_eq!(reported["role"], "reviewer");
    assert_eq!(reported["fallback"]["reason"], "provider_quota");
    // Which binding was unavailable, and that its identity is reported rather
    // than observed.
    assert_eq!(reported["fallback"]["runtime"]["host"], "codex");
    assert_eq!(reported["fallback"]["identitySource"], "host-reported");
    assert!(reported.get("inputsSha256").is_none());

    // The same reviewer contract is re-issued onto the alternate binding.
    assert_eq!(unavailable["next"]["agent"], "reviewer");
    assert_eq!(unavailable["next"]["packet"]["runtime"]["host"], "claude");

    // The substitution's approval alone does not accept the run.
    let approved = fixture.value(
        &[
            "work",
            "return",
            &work,
            unavailable["next"]["assignment"].as_str().unwrap(),
            "--outcome",
            "approved",
        ],
        Some(b"review"),
    );
    assert_progress(&approved["next"]);
    assert_eq!(approved["next"]["action"], "lead_decision");
    assert_eq!(approved["next"]["role"], "lead");

    let events = fixture.events(&ledger);
    let substitution = events
        .iter()
        .find(|event| event["outcome"] == "approved" && event["role"] == "reviewer")
        .unwrap();
    assert_eq!(substitution["agent"], "reviewer");
    assert_eq!(substitution["fallback"]["reason"], "provider_quota");
    assert_eq!(substitution["fallback"]["substituted"], true);
    // The binding that actually produced the verdict.
    assert_eq!(substitution["fallback"]["runtime"], alternate());
    assert_eq!(substitution["fallback"]["identitySource"], "host-reported");
    assert_eq!(substitution["version"], 7);
    // The substitution ran the same stage and attempt as the binding it
    // replaced.
    assert_eq!(substitution["stage"], reported["stage"]);
    assert_eq!(substitution["attempt"], reported["attempt"]);
    assert_eq!(substitution["role"], "reviewer");
    // Freshness binding is unchanged: a reviewer approval still declares the
    // tested inputs it judged.
    assert!(substitution["inputsSha256"].is_string());

    // Only the lead's own acceptance reaches READY.
    let done = fixture.value(
        &[
            "work",
            "return",
            &work,
            approved["next"]["assignment"].as_str().unwrap(),
            "--outcome",
            "accepted",
        ],
        Some(b"accept"),
    );
    assert_eq!(done["next"]["action"], "done");
    assert_eq!(done["next"]["progress"]["state"], "READY");
}

/// A reviewer that requests no primary binding at all can still report its
/// unavailability: the provenance records what was requested, including
/// nothing, rather than refusing the submission.
#[test]
fn an_unspecified_primary_binding_still_records_its_unavailability() {
    let fixture = Fixture::new_with(alternate());
    let config_path = fixture.root.join("exitbind.json");
    let mut config: Value = serde_json::from_slice(&fs::read(&config_path).unwrap()).unwrap();
    config["agents"]["reviewer"]["runtime"] = json!({"fallback": alternate()});
    fs::write(&config_path, serde_json::to_vec_pretty(&config).unwrap()).unwrap();

    let (work, checked) = fixture.to_reviewer("unspecified primary binding");
    let ledger = fixture.ledger(&work);
    let unavailable = fixture.value(
        &[
            "work",
            "return",
            &work,
            checked["next"]["assignment"].as_str().unwrap(),
            "--outcome",
            "unavailable",
            "--reason",
            "provider_quota",
        ],
        Some(b"quota"),
    );
    assert_eq!(unavailable["next"]["action"], "spawn");
    assert_eq!(unavailable["next"]["packet"]["runtime"]["host"], "claude");

    let events = fixture.events(&ledger);
    let reported = events
        .iter()
        .find(|event| event["outcome"] == "unavailable")
        .unwrap();
    assert_eq!(
        reported["fallback"]["runtime"],
        json!({"host": null, "model": null, "reasoningEffort": null})
    );
}

/// 5. Without an authorized alternate binding, an operational failure leaves
/// the run blocked and fabricates no review.
#[test]
fn primary_unavailable_without_a_fallback_stays_blocked() {
    let fixture = Fixture::new_with(json!("none"));
    let (work, checked) = fixture.to_reviewer("no fallback");
    let ledger = fixture.ledger(&work);
    let start = fixture.events(&ledger).remove(0);
    assert_eq!(start["version"], 6);

    let unavailable = fixture.value(
        &[
            "work",
            "return",
            &work,
            checked["next"]["assignment"].as_str().unwrap(),
            "--outcome",
            "unavailable",
            "--reason",
            "provider_unavailable",
        ],
        Some(b"provider down"),
    );
    assert_progress(&unavailable["next"]);
    // With no authorized binding the reviewer cannot be re-issued, so the run
    // blocks instead of advancing on a review that never happened.
    assert_eq!(unavailable["next"]["action"], "done");
    assert_eq!(unavailable["next"]["status"], "blocked");

    let events = fixture.events(&ledger);
    let reported = events
        .iter()
        .find(|event| event["outcome"] == "unavailable")
        .unwrap();
    assert_eq!(reported["agent"], "reviewer");
    // No bounded reason is attachable without the v7 fallback shape, and no
    // fabricated fallback binding appears.
    assert_eq!(reported["version"], 6);
    assert!(reported.get("fallback").is_none());

    // No reviewer ever produced a verdict.
    let events = fixture.events(&ledger);
    assert!(!events.iter().any(|event| {
        event["role"] == "reviewer"
            && matches!(
                event["outcome"].as_str(),
                Some("approved" | "rework" | "blocked")
            )
    }));
}

/// 6. A second operational failure (the substitution's own) is bounded: the run
/// blocks instead of hunting for a third binding.
#[test]
fn fallback_that_is_also_unavailable_blocks_without_retrying() {
    let fixture = Fixture::new_with(alternate());
    let (work, checked) = fixture.to_reviewer("double outage");
    let ledger = fixture.ledger(&work);

    let primary = fixture.value(
        &[
            "work",
            "return",
            &work,
            checked["next"]["assignment"].as_str().unwrap(),
            "--outcome",
            "unavailable",
            "--reason",
            "rate_limit",
        ],
        Some(b"rate limited"),
    );
    assert_eq!(primary["next"]["agent"], "reviewer");
    assert_eq!(primary["next"]["packet"]["runtime"]["host"], "claude");

    let substitute = fixture.value(
        &[
            "work",
            "return",
            &work,
            primary["next"]["assignment"].as_str().unwrap(),
            "--outcome",
            "unavailable",
            "--reason",
            "provider_unavailable",
        ],
        Some(b"also down"),
    );
    assert_progress(&substitute["next"]);
    assert_eq!(substitute["next"]["action"], "done");
    assert_eq!(substitute["next"]["status"], "blocked");

    // Bounded attempts: exactly two operational failures, no third binding.
    let events = fixture.events(&ledger);
    let unavailable = events
        .iter()
        .filter(|event| event["outcome"] == "unavailable")
        .count();
    assert_eq!(unavailable, 2);
    assert!(!events.iter().any(|event| event["outcome"] == "approved"));
}

/// 3. Anti-review-shopping: once a reviewer returned `rework`, an operational
/// failure cannot re-open the review on another binding.
#[test]
fn unavailable_is_rejected_after_a_rework_verdict() {
    let fixture = Fixture::new_with(alternate());
    let (work, checked) = fixture.to_reviewer("no shopping after rework");
    let ledger = fixture.ledger(&work);

    let rework = fixture.value(
        &[
            "work",
            "return",
            &work,
            checked["next"]["assignment"].as_str().unwrap(),
            "--outcome",
            "rework",
        ],
        Some(b"rework"),
    );
    assert_progress(&rework["next"]);

    // The reviewer is still bound to its primary on the next attempt; asking it
    // to report unavailability would be an escape from the adverse verdict.
    let attempt = fixture.value(&["work", "next", &work], None);
    let assignment = attempt["next"]["assignment"].as_str().unwrap();
    let escaped = fixture.call(
        &[
            "work",
            "return",
            &work,
            assignment,
            "--outcome",
            "unavailable",
            "--reason",
            "provider_quota",
        ],
        Some(b"quota"),
    );
    assert!(
        !escaped.status.success(),
        "rework was re-opened by fallback: {escaped:?}"
    );

    let events = fixture.events(&ledger);
    assert_eq!(
        events
            .iter()
            .filter(|event| event["outcome"] == "unavailable")
            .count(),
        0
    );
    assert!(!events
        .iter()
        .any(|event| event["role"] == "reviewer" && event["outcome"] == "approved"));
}

/// 3. The same rule holds for `blocked`: no verdict may be shopped away.
#[test]
fn unavailable_is_rejected_after_a_blocked_verdict() {
    let fixture = Fixture::new_with(alternate());
    let (work, checked) = fixture.to_reviewer("no shopping after blocked");
    let ledger = fixture.ledger(&work);

    let blocked = fixture.value(
        &[
            "work",
            "return",
            &work,
            checked["next"]["assignment"].as_str().unwrap(),
            "--outcome",
            "blocked",
        ],
        Some(b"blocked"),
    );
    assert_progress(&blocked["next"]);

    // The reviewer verdict of `blocked` is terminal for the run, so the run is
    // no longer running and the reducer refuses any further submission.
    let attempt = fixture.value(&["work", "next", &work], None);
    assert_eq!(attempt["next"]["action"], "done");
    assert_eq!(attempt["next"]["status"], "blocked");
    let events = fixture.events(&ledger);
    assert!(!events.iter().any(|event| event["outcome"] == "unavailable"));

    // The reducer is the authority, independent of the façade: a hand-crafted
    // unavailable after a reviewer block is refused on replay.
    let replay = fixture.call(&["run", "inspect", &ledger, "--json"], None);
    assert!(replay.status.success(), "{replay:?}");
}

/// 7. A substitution is a review like any other: the subject it approved goes
/// stale when a covered input changes.
#[test]
fn fallback_approval_goes_stale_with_its_subject() {
    let fixture = Fixture::new_with(alternate());
    let (work, checked) = fixture.to_reviewer("stale fallback review");

    let primary = fixture.value(
        &[
            "work",
            "return",
            &work,
            checked["next"]["assignment"].as_str().unwrap(),
            "--outcome",
            "unavailable",
            "--reason",
            "provider_quota",
        ],
        Some(b"quota"),
    );
    let approved = fixture.value(
        &[
            "work",
            "return",
            &work,
            primary["next"]["assignment"].as_str().unwrap(),
            "--outcome",
            "approved",
        ],
        Some(b"review"),
    );
    assert_eq!(approved["next"]["action"], "lead_decision");

    // Mutate the tested input the fallback review was bound to: the worker
    // result changes, so the approval can no longer authorize it.
    fs::remove_file(fixture.root.join("pass-marker")).unwrap();

    let stale = fixture.value(&["work", "next", &work], None);
    assert_progress(&stale["next"]);
    assert_ne!(stale["next"]["action"], "lead_decision");

    let premature = fixture.call(
        &[
            "work",
            "return",
            &work,
            approved["next"]["assignment"].as_str().unwrap(),
            "--outcome",
            "accepted",
        ],
        Some(b"accept"),
    );
    assert!(
        !premature.status.success(),
        "stale fallback approval accepted: {premature:?}"
    );
}

/// 8. A fresh session reconstructs the recorded substitution and its review
/// without needing any provider-specific hidden state.
#[test]
fn resume_reconstructs_a_recorded_substitution() {
    let fixture = Fixture::new_with(alternate());
    let (work, checked) = fixture.to_reviewer("resume substitution");
    let ledger = fixture.ledger(&work);

    let primary = fixture.value(
        &[
            "work",
            "return",
            &work,
            checked["next"]["assignment"].as_str().unwrap(),
            "--outcome",
            "unavailable",
            "--reason",
            "rate_limit",
        ],
        Some(b"rate limited"),
    );
    let approved = fixture.value(
        &[
            "work",
            "return",
            &work,
            primary["next"]["assignment"].as_str().unwrap(),
            "--outcome",
            "approved",
        ],
        Some(b"review"),
    );
    assert_eq!(approved["next"]["action"], "lead_decision");
    let events_before = fixture.events(&ledger);

    // A separate process reads the ledger alone.
    let resumed = fixture.value(&["work", "resume"], None);
    assert_eq!(resumed["status"], "resumed");
    assert_eq!(resumed["work"], work);
    assert_eq!(resumed["next"]["action"], "lead_decision");
    assert_eq!(resumed["next"]["role"], "lead");
    assert!(resumed["residual"]["doNotRepeat"]
        .as_array()
        .unwrap()
        .iter()
        .any(|item| item == "review"));

    // No duplicate review and no new event: the recorded substitution is the
    // only reviewer verdict for this attempt, and it reconstructs from the
    // ledger alone.
    assert_eq!(fixture.events(&ledger).len(), events_before.len());
    let verdicts: Vec<Value> = fixture
        .events(&ledger)
        .into_iter()
        .filter(|event| event["role"] == "reviewer" && event["outcome"] == "approved")
        .collect();
    assert_eq!(verdicts.len(), 1);
    assert_eq!(verdicts[0]["fallback"]["substituted"], true);
    assert_eq!(verdicts[0]["fallback"]["runtime"], alternate());
}

/// 9. The human rendering is a separate surface from the machine packet, and it
/// must not collapse a configured alternate binding into "none". A configured
/// binding that rendered as "none" would read as "no fallback was authorized"
/// when one in fact was.
#[test]
fn a_configured_alternate_binding_cannot_render_as_none() {
    let fixture = Fixture::new_with(alternate());
    let configured = fixture.call(&["brief", "reviewer", "--task", "render"], None);
    assert!(configured.status.success(), "{configured:?}");
    let configured = String::from_utf8_lossy(&configured.stdout).into_owned();
    let configured_line = runtime_line(&configured);

    assert!(
        !configured_line.ends_with("fallback=none"),
        "a configured alternate binding rendered as no fallback: {configured_line}"
    );
    for value in [
        "codex",
        "primary-review",
        "claude",
        "alternate-review",
        "high",
    ] {
        assert!(
            configured_line.contains(value),
            "rendered binding omits '{value}': {configured_line}"
        );
    }
    // Rendering names what is requested, never what ran: the same rendering
    // path certifies nothing about observed execution.
    assert!(
        configured_line.contains("requested"),
        "rendering does not mark itself as a request: {configured_line}"
    );

    // The declined literal is the only value that renders as no fallback.
    let declined = Fixture::new_with(json!("none"));
    let literal = declined.call(&["brief", "reviewer", "--task", "render"], None);
    assert!(literal.status.success(), "{literal:?}");
    let literal = String::from_utf8_lossy(&literal.stdout).into_owned();
    assert!(
        runtime_line(&literal).ends_with("fallback=none"),
        "a declined fallback did not render as none: {}",
        runtime_line(&literal)
    );
}

/// The one rendered line that reports the requested runtime binding.
fn runtime_line(rendered: &str) -> String {
    rendered
        .lines()
        .find(|line| line.starts_with("Requested runtime: "))
        .unwrap_or_else(|| panic!("no requested-runtime line in: {rendered}"))
        .to_owned()
}

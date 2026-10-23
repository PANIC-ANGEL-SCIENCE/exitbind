//! R55 F5b follow-up: a failed same-host child's retained final message is
//! recovered without a cross-host context, from the session that prepared it,
//! and the existing refusals a retained child causes name it with the
//! recover line, which runs unedited and clears them.
use super::{run, sha, text, Fixture, CHILD, OTHER, RESULT, SESSION};
use serde_json::{json, Value};
use std::{fs, process::Output};

impl Fixture {
    /// `work child recover WORK INTENT` with `extra`, from `session`.
    fn recover(&self, session: Option<&str>, intent: &str, extra: &[&str]) -> Output {
        let mut args = vec!["work", "child", "recover", self.work.as_str(), intent];
        args.extend(extra);
        self.call(session, &args, b"", true)
    }
}

/// Review F5b R2: a failed same-host child's retained final message is
/// reattached without a cross-host context. Recover without `--context`
/// refuses another session and no session and changes nothing; with
/// `--context` it keeps the cross-host route and its refusal; from the Lead
/// session that holds the Work again it records the hash and state, as a
/// same-host stop does, with no continuation record.
#[test]
fn a_failed_same_host_child_is_recovered_from_the_session_that_holds_the_work() {
    let fixture = Fixture::new("f5b-same-host-recover");
    fixture.prepare_sonic();
    fixture.hook(
        json!({"hook_event_name":"SubagentStart","session_id":SESSION,
        "agent_id":CHILD,"agent_type":"sonic"}),
    );
    let companion = fs::read(fixture.companion()).unwrap();
    fs::remove_file(fixture.companion()).unwrap();
    fixture.hook(
        json!({"hook_event_name":"SubagentStop","session_id":SESSION,
        "agent_id":CHILD,"agent_type":"sonic","last_assistant_message":RESULT}),
    );
    let failed = fixture.intents().unwrap()[0].clone();
    assert_eq!(failed["state"], "failed", "{failed}");
    assert_eq!(failed["pendingResult"], RESULT, "{failed}");
    let intent = failed["id"].as_str().unwrap().to_owned();
    fs::write(fixture.companion(), companion).unwrap();

    let other = fixture.recover(Some(OTHER), &intent, &[]);
    assert!(!other.status.success(), "{}", text(&other));
    assert!(
        text(&other).contains("this native session does not hold the Work"),
        "{}",
        text(&other)
    );
    let none = fixture.recover(None, &intent, &[]);
    assert!(!none.status.success(), "{}", text(&none));
    assert!(
        text(&none).contains("uses the --context token from work continuation"),
        "{}",
        text(&none)
    );
    let token = format!("{}:1:0:00", fixture.work);
    let context = fixture.recover(Some(SESSION), &intent, &["--context", &token]);
    assert!(!context.status.success(), "{}", text(&context));
    assert!(
        text(&context).contains("work context token is invalid"),
        "{}",
        text(&context)
    );
    assert_eq!(
        fixture.intents().unwrap()[0],
        failed,
        "refusals change nothing"
    );

    let recovered = fixture.recover(Some(SESSION), &intent, &[]);
    assert!(recovered.status.success(), "{}", text(&recovered));
    let recovered: Value = serde_json::from_slice(&recovered.stdout).unwrap();
    assert_eq!(recovered["effect"], "recorded", "{recovered}");
    assert_eq!(recovered["intent"], intent.as_str());
    let intent = &fixture.intents().unwrap()[0];
    assert_eq!(intent["state"], "recorded", "{intent}");
    assert_eq!(intent["nativeChild"], CHILD);
    assert_eq!(intent["resultSha256"], sha(RESULT.as_bytes()));
    assert!(intent.get("pendingResult").is_none(), "{intent}");
    assert!(intent.get("failure").is_none(), "{intent}");
    fixture.assert_no_continuation();
    let again = fixture.recover(Some(SESSION), intent["id"].as_str().unwrap(), &[]);
    assert!(
        text(&again).contains("this child has no retained result to recover"),
        "{}",
        text(&again)
    );
}

impl Fixture {
    /// Prepare and start sonic, then break the same-host binding so its stop
    /// fails it with the final message retained; the binding is restored.
    /// Returns the failed intent.
    fn failed_child(&self) -> Value {
        self.prepare_sonic();
        self.hook(
            json!({"hook_event_name":"SubagentStart","session_id":SESSION,
            "agent_id":CHILD,"agent_type":"sonic"}),
        );
        let companion = fs::read(self.companion()).unwrap();
        fs::remove_file(self.companion()).unwrap();
        self.hook(
            json!({"hook_event_name":"SubagentStop","session_id":SESSION,
            "agent_id":CHILD,"agent_type":"sonic","last_assistant_message":RESULT}),
        );
        fs::write(self.companion(), companion).unwrap();
        let failed = self.intents().unwrap()[0].clone();
        assert_eq!(failed["state"], "failed", "{failed}");
        assert_eq!(failed["pendingResult"], RESULT, "{failed}");
        failed
    }
}

/// Review D2: a later session that adopted the Lead session's Goal journal
/// holds the Work, but the child ran under the Lead session: recover without
/// `--context` from it is refused and changes nothing.
#[test]
fn a_session_that_adopted_the_goal_cannot_claim_another_sessions_child() {
    let fixture = Fixture::new("f5b-recover-adopted");
    let failed = fixture.failed_child();
    let companion: Value = serde_json::from_slice(&fs::read(fixture.companion()).unwrap()).unwrap();
    let key = companion["sessionKey"].as_str().unwrap();
    let adopted = fixture.call(
        Some(OTHER),
        &["goal", "adopt", "--session-key", key],
        b"",
        true,
    );
    assert!(adopted.status.success(), "{}", text(&adopted));
    let intent = failed["id"].as_str().unwrap();
    let refused = fixture.recover(Some(OTHER), intent, &[]);
    assert!(!refused.status.success(), "{}", text(&refused));
    assert!(
        text(&refused).contains("the child ran under another receiving session"),
        "{}",
        text(&refused)
    );
    assert_eq!(
        fixture.intents().unwrap()[0],
        failed,
        "the refusal changes nothing"
    );
}

/// The printed recover line from a refusal, run unedited in `session`.
fn run_line(fixture: &Fixture, refusal: &str, session: &str) -> Output {
    let (_, line) = refusal
        .split_once("record its retained result with: ")
        .unwrap_or_else(|| panic!("no recover line: {refusal}"));
    let line = line.lines().next().unwrap().trim_end();
    let argv = ["sh".to_owned(), "-c".to_owned(), line.to_owned()];
    run(&argv, &fixture.product, &fixture.store, Some(session), b"")
}

/// Review D1: a retained same-host child blocks check replacement; the
/// refusal names the child and prints its recover line, which runs unedited
/// from the Lead session and clears the block.
#[test]
fn a_retained_child_refusal_prints_the_recover_line_that_clears_it() {
    let fixture = Fixture::new("f5b-recover-from-refusal");
    let failed = fixture.failed_child();
    let worker = fixture.next(Some(SESSION))["assignment"]
        .as_str()
        .unwrap()
        .to_owned();
    let work = fixture.work.clone();
    let blocked = fixture.call(
        Some(SESSION),
        &["work", "return", &work, &worker, "--outcome", "blocked"],
        b"blocked on a retained child",
        true,
    );
    assert!(blocked.status.success(), "{}", text(&blocked));
    let replace = |fixture: &Fixture| {
        let next = fixture.value(Some(SESSION), &["work", "next", &work, "--json"]);
        let binding = next["current"]["binding"].as_str().unwrap().to_owned();
        fixture.call(
            Some(SESSION),
            &[
                "work",
                "replace-check",
                &work,
                "--check-command",
                "true",
                "--reason",
                "the frozen check is wrong",
                "--classification",
                "defective-contract",
                "--current-binding",
                &binding,
            ],
            b"",
            true,
        )
    };
    let refused = replace(&fixture);
    assert!(!refused.status.success(), "{}", text(&refused));
    let refusal = text(&refused);
    let intent = failed["id"].as_str().unwrap();
    assert!(
        refusal.contains(&format!(
            "check replacement is blocked by a live, prepared, or retained child result: child {intent} ("
        )),
        "{refusal}"
    );
    assert!(!refusal.contains("--context"), "{refusal}");
    assert_eq!(fixture.intents().unwrap()[0], failed);

    let recovered = run_line(&fixture, &refusal, SESSION);
    assert!(recovered.status.success(), "{}", text(&recovered));
    assert_eq!(fixture.intents().unwrap()[0]["state"], "recorded");
    let again = replace(&fixture);
    assert!(
        !text(&again).contains("retained child"),
        "the block is cleared: {}",
        text(&again)
    );
}

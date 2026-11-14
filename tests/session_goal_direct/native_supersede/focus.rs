//! Native successor focus, exact child delivery and recovery.

use super::*;
use std::process::Output;

const NATIVE_PERMIT_MEANING: &str = "obtain allowed:true before editing; repeat this exact request ID only to recover its response; this form grants no host permission";
const PUBLIC_PERMIT_MEANING: &str = "advisory for ordinary host workspace writes; required:true applies to managed file effects and a selected controlled-effects launch, which require allowed:true. Only a successful controlled launch and protection preflight enforce that file effect. Repeat the exact request ID only to recover its response; this form grants no host permission";

fn permit_forms_match(native: &Value, public: &Value) -> bool {
    if native["meaning"] != NATIVE_PERMIT_MEANING
        || native.get("enforcement").is_some()
        || public["meaning"] != PUBLIC_PERMIT_MEANING
        || public["enforcement"] != "advisory"
    {
        return false;
    }
    let mut expected = native.clone();
    expected["meaning"] = json!(PUBLIC_PERMIT_MEANING);
    expected["enforcement"] = json!("advisory");
    expected == *public
}

fn owner_value(case: &Case, args: &[&str]) -> Value {
    case.project
        .value_in_session(case.host, ADOPTER, &case.project.session_store, args)
}

fn scope_successor(case: &Case) -> Value {
    let lead = owner_value(case, &["work", "next", &case.successor, "--json", "--full"]);
    let assignment = lead["next"]["assignment"].as_str().unwrap();
    owner_value(
        case,
        &[
            "work",
            "return",
            &case.successor,
            assignment,
            "--outcome",
            "scoped",
        ],
    );
    let worker = owner_value(case, &["work", "next", &case.successor, "--json", "--full"]);
    assert_eq!(worker["next"]["packet"]["nativeTaskName"], "sonic");
    worker
}

fn owner_command(case: &Case, argv: &[String]) -> Output {
    let mut command = crate::support::git_topology::command(&argv[0]);
    command
        .current_dir(&case.project.root)
        .args(&argv[1..])
        .env_remove("CODEX_THREAD_ID")
        .env_remove("CODEX_SESSION_ID")
        .env_remove("CLAUDE_CODE_SESSION_ID")
        .env_remove("EXITBIND_NATIVE_SESSION_ID")
        .env("EXITBIND_SESSION_GOALS_DIR", &case.project.session_store)
        .env("CLAUDE_CODE_SESSION_ID", ADOPTER);
    command.output().unwrap()
}

fn prepare_argv(next: &Value) -> Vec<String> {
    next["next"]["prepareChild"]["command"]
        .as_array()
        .unwrap()
        .iter()
        .map(|value| value.as_str().unwrap().to_owned())
        .collect()
}

fn start_hook(case: &Case, child: &str) -> Value {
    let payload = json!({"hook_event_name":"SubagentStart", "cwd":case.project.root.to_str().unwrap(),
        "session_id":ADOPTER, "agent_id":child, "agent_type":"sonic"});
    let mut command = crate::support::git_topology::command(env!("CARGO_BIN_EXE_exitbind"));
    command
        .arg("hook-run")
        .current_dir(&case.project.root)
        .env_remove("CODEX_THREAD_ID")
        .env_remove("CODEX_SESSION_ID")
        .env_remove("CLAUDE_CODE_SESSION_ID")
        .env_remove("EXITBIND_NATIVE_SESSION_ID")
        .env("EXITBIND_SESSION_GOALS_DIR", &case.project.session_store)
        .env("EXITBIND_NO_UPDATE_CHECK", "1")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let mut child = command.spawn().unwrap();
    child
        .stdin
        .take()
        .unwrap()
        .write_all(payload.to_string().as_bytes())
        .unwrap();
    let output = child.wait_with_output().unwrap();
    assert!(output.status.success(), "{output:?}");
    assert!(
        output.stdout.len() <= 16 * 1024 + 1,
        "{} bytes",
        output.stdout.len()
    );
    serde_json::from_slice(&output.stdout).unwrap_or_else(|error| {
        panic!(
            "invalid hook JSON ({error}); status={}; stdout bytes={} {:?}; stderr bytes={} {:?}",
            output.status,
            output.stdout.len(),
            String::from_utf8_lossy(&output.stdout),
            output.stderr.len(),
            String::from_utf8_lossy(&output.stderr)
        )
    })
}

fn begin_unrelated(case: &Case) -> String {
    case.project.value(&[
        "work",
        "begin",
        "change",
        "--goal",
        "Independent fixture Work",
        "--check-command",
        "true",
        "--standalone",
    ])["work"]
        .as_str()
        .unwrap()
        .to_owned()
}

fn assert_complete_successor_delivery(label: &str) {
    let mut case = Case::new_for_host(label, false, "claude");
    let result = case.supersede();
    assert_eq!(result["effect"], "started");
    assert_eq!(result["focus"]["updated"], true);
    assert_eq!(result["focus"]["current"], true);
    assert_eq!(result["focus"]["authority"], "none");
    let worker = scope_successor(&case);
    let detail = owner_value(&case, &["work", "detail", &case.successor, "--json"]);
    let argv = prepare_argv(&worker);
    let mut stale = argv.clone();
    let index = stale
        .iter()
        .position(|word| word == "--current-binding")
        .unwrap();
    stale[index + 1] = "0".repeat(64);
    let refused = owner_command(&case, &stale);
    assert!(
        !refused.status.success(),
        "stale binding must refuse: {refused:?}"
    );
    let prepared = owner_command(&case, &argv);
    assert!(prepared.status.success(), "{prepared:?}");
    let prepared: Value = serde_json::from_slice(&prepared.stdout).unwrap();
    assert_eq!(prepared["effect"], "prepared");
    assert!(prepared["next"].as_str().unwrap().contains("work detail"));
    assert!(!prepared["next"]
        .as_str()
        .unwrap()
        .contains("work continuation"));
    let hook = start_hook(&case, "native-successor-child");
    let context = hook["hookSpecificOutput"]["additionalContext"]
        .as_str()
        .unwrap();
    let packet = context.split_once("Complete current assignment, evidence and task conditions (verified for this recipient):\n")
        .and_then(|(_, rest)| rest.lines().next())
        .unwrap_or_else(|| panic!("complete successor delivery unavailable: {context}"));
    let packet: Value = serde_json::from_str(packet).unwrap();
    assert_eq!(packet["currentAction"]["binding"], detail["binding"]);
    assert_eq!(
        packet["goal"],
        detail["sections"]["assignment"]["assignment"]["goal"]
    );
    assert_eq!(packet["taskGoal"], detail["sections"]["tasks"]["goal"]);
    assert_ne!(packet["goal"], packet["taskGoal"]);
    assert_eq!(
        packet["boundary"],
        detail["sections"]["assignment"]["assignment"]["declaredBoundary"]
    );
    assert_eq!(
        packet["role"],
        detail["sections"]["assignment"]["assignment"]["role"]
    );
    assert_eq!(
        packet["agent"],
        detail["sections"]["assignment"]["assignment"]["agent"]
    );
    let native_form = &packet["currentAction"]["beforeEditing"];
    let public_form = &detail["actionForms"]["beforeEditing"];
    assert_eq!(native_form["meaning"], NATIVE_PERMIT_MEANING);
    assert!(native_form.get("enforcement").is_none());
    assert_eq!(public_form["meaning"], PUBLIC_PERMIT_MEANING);
    assert_eq!(public_form["enforcement"], "advisory");
    assert!(permit_forms_match(native_form, public_form));

    let mut altered = public_form.clone();
    altered["unexpected"] = json!("foreign");
    assert!(!permit_forms_match(native_form, &altered));
    let mut altered = public_form.clone();
    altered["meaning"] = json!(NATIVE_PERMIT_MEANING);
    assert!(!permit_forms_match(native_form, &altered));
    let mut altered = public_form.clone();
    altered["enforcement"] = json!("enforced");
    assert!(!permit_forms_match(native_form, &altered));
    let mut altered = public_form.clone();
    altered["command"]["argv"][0] = json!("foreign-executable");
    assert!(!permit_forms_match(native_form, &altered));
    let mut altered = public_form.clone();
    altered["inputConstraints"]["OPERATION"]["maximumBytes"] = json!(121);
    assert!(!permit_forms_match(native_form, &altered));
    let mut altered = public_form.clone();
    altered["fill"]["options"]["OPERATION"] = json!("--other");
    assert!(!permit_forms_match(native_form, &altered));
    let mut altered_native = native_form.clone();
    altered_native["enforcement"] = json!("advisory");
    assert!(!permit_forms_match(&altered_native, public_form));

    let permit = native_form["command"]["argv"].as_array().unwrap();
    assert_eq!(permit[0], env!("CARGO_BIN_EXE_exitbind"));
    assert_eq!(packet["work"], detail["work"]);
    assert_eq!(packet["assignment"], detail["recipient"]["assignment"]);
    assert_eq!(permit[3], detail["work"]);
    assert_eq!(permit[4], detail["recipient"]["assignment"]);
    let request_index = permit
        .iter()
        .position(|word| word == "--request-id")
        .unwrap();
    assert_eq!(
        permit[request_index + 1],
        format!("permit-{}", detail["binding"].as_str().unwrap())
    );
    let config_index = permit.iter().position(|word| word == "--config").unwrap();
    assert_eq!(
        permit[config_index + 1],
        case.project.root.join("exitbind.json").to_str().unwrap()
    );
    let profile = detail["recipientContext"]["profile"]["content"]
        .as_str()
        .unwrap();
    let digest = digest_text(profile);
    assert!(
        context.contains(&format!("Profile SHA-256: {digest}\nPresented profile SHA-256: same as Profile SHA-256 (after path redaction).")),
        "both profile identities must resolve to the exact source digest"
    );
    assert!(
        context.contains(&format!("Profile bytes:\n{profile}")),
        "complete current profile bytes must reach the named worker"
    );
    for rule in detail["recipientContext"]["rules"].as_array().unwrap() {
        let content = rule["content"].as_str().unwrap();
        assert_eq!(rule["complete"], true);
        assert_eq!(rule["sha256"], digest_text(content));
        assert!(
            context.contains(content),
            "complete rule bytes must be presented"
        );
    }
    let presented = packet["evidence"]["items"].as_array().unwrap();
    let expected = detail["sections"]["evidence"]["items"].as_array().unwrap();
    assert_eq!(packet["evidence"]["complete"], true);
    assert_eq!(presented.len(), expected.len());
    for (actual, expected) in presented.iter().zip(expected) {
        for field in ["kind", "outcome", "reference"] {
            assert_eq!(actual[field], expected[field]);
        }
        let artifact = &expected["artifact"];
        let content = artifact["content"].as_str().unwrap();
        assert_eq!(artifact["complete"], true);
        assert_eq!(artifact["verified"], true);
        assert_eq!(artifact["sha256"], digest_text(content));
        assert_eq!(actual["artifact"], content);
    }
    let intents: Value = serde_json::from_slice(
        &fs::read(case.project.root.join(".exitbind/child-intents.json")).unwrap(),
    )
    .unwrap();
    let selected = intents["intents"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|intent| intent["assignment"] == detail["recipient"]["assignment"])
        .collect::<Vec<_>>();
    assert_eq!(selected.len(), 1);
    let delivery = &selected[0]["assignmentDelivery"];
    assert_eq!(delivery["state"], "presented");
    assert_eq!(delivery["binding"], detail["binding"]);
    assert_eq!(
        delivery["recipientSha256"],
        detail["contextIdentity"]["recipientSha256"]
    );
    assert_eq!(
        delivery["permitForm"]["sha256"],
        digest_text(&native_form.to_string())
    );
    assert_eq!(
        delivery["permitForm"]["requestId"],
        permit[request_index + 1]
    );
    assert_eq!(
        delivery["actorFactsSha256"],
        digest_text(&packet.to_string())
    );
}

#[test]
fn successor_focus_delivers_the_exact_named_assignment_at_native_start() {
    assert_complete_successor_delivery("native-successor-focus-delivery");
}

#[test]
fn successor_focus_delivers_complete_assignment_at_longer_project_path() {
    assert_complete_successor_delivery("native-successor-focus-delivery-more");
}

#[test]
fn replay_preserves_a_later_unrelated_focus() {
    let mut case = Case::new_for_host("native-successor-focus-replay", false, "claude");
    case.supersede();
    let before = fs::read(case.ledger(&case.successor)).unwrap();
    let unrelated = begin_unrelated(&case);
    let replay = case.supersede();
    assert_eq!(replay["effect"], "started-focus-pending");
    assert_eq!(replay["focus"]["current"], false);
    assert_eq!(replay["focus"]["command"][2], "focus");
    assert_eq!(replay["focus"]["command"][3], case.successor);
    assert_eq!(owner_value(&case, &["work", "resume"])["work"], unrelated);
    assert_eq!(fs::read(case.ledger(&case.successor)).unwrap(), before);
    let worker = scope_successor(&case);
    let intents = case.project.root.join(".exitbind/child-intents.json");
    let before = fs::read(&intents).ok();
    let refused = owner_command(&case, &prepare_argv(&worker));
    assert!(
        !refused.status.success(),
        "unselected Work must not prepare: {refused:?}"
    );
    assert_eq!(fs::read(&intents).ok(), before);
}

#[test]
fn admission_only_recovery_preserves_a_later_predecessor_selection() {
    let mut case = Case::new_for_host("native-successor-focus-admission", false, "claude");
    case.supersede();
    let successor = case.successor.clone();
    case.rewind_to_pending(false, false);
    owner_value(&case, &["work", "focus", &case.predecessor]);
    let resumed = case.supersede();
    assert_eq!(resumed["work"], successor);
    assert_eq!(resumed["effect"], "started-focus-pending");
    assert_eq!(resumed["focus"]["current"], false);
    assert_eq!(resumed["focus"]["command"][3], successor);
    assert_eq!(
        owner_value(&case, &["work", "resume"])["focus"]["work"],
        case.predecessor
    );
}

#[test]
fn focus_change_after_prepare_refuses_native_child_delivery() {
    let mut case = Case::new_for_host("native-successor-focus-stale", false, "claude");
    case.supersede();
    let worker = scope_successor(&case);
    let prepared = owner_command(&case, &prepare_argv(&worker));
    assert!(prepared.status.success(), "{prepared:?}");
    let unrelated = begin_unrelated(&case);
    let hook = start_hook(&case, "native-successor-stale-child");
    let context = hook["hookSpecificOutput"]["additionalContext"]
        .as_str()
        .unwrap();
    assert!(
        context.contains("selected assignment changed before a child started"),
        "{context}"
    );
    assert!(!context.contains("Complete current assignment, evidence and task conditions"));
    assert_eq!(owner_value(&case, &["work", "resume"])["work"], unrelated);
}

#[test]
fn focus_write_failure_preserves_started_successor_and_exact_recovery() {
    let mut case = Case::new_for_host("native-successor-focus-write", false, "claude");
    let executable = env!("CARGO_BIN_EXE_exitbind");
    let config = case.project.root.join("exitbind.json");
    let mut command = crate::support::git_topology::command("sh");
    command
        .arg("-c")
        .arg("read -r _; exec \"$@\"")
        .arg("focus-fixture")
        .arg(executable)
        .args([
            "work",
            "supersede",
            &case.predecessor,
            "--reason=--json",
            "--json",
            "--config",
        ])
        .arg(config)
        .current_dir(&case.project.root)
        .env_remove("CODEX_THREAD_ID")
        .env_remove("CODEX_SESSION_ID")
        .env_remove("EXITBIND_NATIVE_SESSION_ID")
        .env("CLAUDE_CODE_SESSION_ID", ADOPTER)
        .env("EXITBIND_SESSION_GOALS_DIR", &case.project.session_store)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let mut child = command.spawn().unwrap();
    let staged = case
        .project
        .root
        .join(format!(".exitbind/.current-work.json.{}.tmp", child.id()));
    fs::create_dir(&staged).unwrap();
    child.stdin.take().unwrap().write_all(b"go\n").unwrap();
    let output = child.wait_with_output().unwrap();
    assert!(output.status.success(), "{output:?}");
    let result: Value = serde_json::from_slice(&output.stdout).unwrap();
    case.successor = result["work"].as_str().unwrap().to_owned();
    assert_eq!(result["effect"], "started-focus-pending");
    assert_eq!(result["sessionLink"]["state"], "linked");
    assert_eq!(result["focus"]["current"], false);
    assert_eq!(result["focus"]["command"][2], "focus");
    assert_eq!(result["focus"]["command"][3], case.successor);
    let before = fs::read(case.ledger(&case.successor)).unwrap();
    fs::remove_dir(staged).unwrap();
    let focus: Vec<String> = result["focus"]["command"]
        .as_array()
        .unwrap()
        .iter()
        .map(|value| value.as_str().unwrap().to_owned())
        .collect();
    let recovered = owner_command(&case, &focus);
    assert!(recovered.status.success(), "{recovered:?}");
    let replay = case.supersede();
    assert_eq!(replay["work"], case.successor);
    assert_eq!(replay["focus"]["current"], true);
    assert_eq!(replay["focus"]["updated"], false);
    assert_eq!(fs::read(case.ledger(&case.successor)).unwrap(), before);
}

#[test]
fn failed_native_link_preserves_predecessor_focus_and_started_work() {
    let mut case = Case::new_for_host("native-successor-focus-link", false, "claude");
    let original_goal = crate::support::goal_journal::records(&case.journal());
    case.supersede();
    let successor = case.successor.clone();
    case.rewind_to_pending(false, false);
    crate::support::goal_journal::write(&case.journal(), &original_goal);
    owner_value(&case, &["work", "focus", &case.predecessor]);
    let companion = case.companion();
    fs::create_dir(&companion).unwrap();
    let result = case.supersede();
    assert_eq!(result["work"], successor);
    assert_eq!(result["effect"], "started-link-pending");
    assert!(result["sessionLink"]["error"].is_string());
    assert_eq!(result["focus"]["current"], false);
    assert!(result["focus"]["error"]
        .as_str()
        .unwrap()
        .contains("link is pending"));
    assert_eq!(result["focus"]["command"][3], successor);
    assert!(result["currentDetail"]["argv"].is_array());
    assert!(case.ledger(&successor).is_file());
    assert!(companion.is_dir());
    assert_eq!(
        owner_value(&case, &["work", "resume"])["focus"]["work"],
        case.predecessor
    );
}

#[test]
fn absent_focus_and_held_focus_lock_do_not_overwrite_navigation() {
    let mut absent = Case::new_for_host("native-successor-focus-absent", false, "claude");
    fs::remove_file(absent.project.root.join(".exitbind/current-work.json")).unwrap();
    let result = absent.supersede();
    assert_eq!(result["effect"], "started-focus-pending");
    assert_eq!(result["focus"]["current"], false);
    assert_eq!(result["focus"]["command"][3], absent.successor);
    assert!(!absent
        .project
        .root
        .join(".exitbind/current-work.json")
        .exists());

    let held = Case::new_for_host("native-successor-focus-lock", false, "claude");
    let lock = held.project.root.join(".exitbind/current-work-focus.lock");
    fs::create_dir(&lock).unwrap();
    let explicit = held.call(&["work", "focus", &held.predecessor]);
    assert!(
        !explicit.status.success(),
        "a held focus lock must fence explicit focus"
    );
    let begun = held.project.value(&[
        "work",
        "begin",
        "change",
        "--goal",
        "Focus lock fixture",
        "--check-command",
        "true",
        "--standalone",
    ]);
    assert!(begun["work"].is_string());
    assert_eq!(begun["focus"]["updated"], false);
    let before_goal = fs::read(held.journal()).unwrap();
    let before_work = fs::read(held.ledger(&held.predecessor)).unwrap();
    let refused = held.call(&[
        "work",
        "supersede",
        &held.predecessor,
        "--reason=--json",
        "--json",
    ]);
    assert!(
        !refused.status.success(),
        "a held focus lock must refuse before start"
    );
    let refusal: Value = serde_json::from_slice(&refused.stdout)
        .unwrap_or_else(|error| panic!("invalid JSON refusal ({error}): {refused:?}"));
    assert!(
        refusal["error"]
            .as_str()
            .is_some_and(|error| error.contains("no native supersession was started")),
        "{refused:?}"
    );
    assert_eq!(fs::read(held.journal()).unwrap(), before_goal);
    assert_eq!(
        fs::read(held.ledger(&held.predecessor)).unwrap(),
        before_work
    );
    assert!(!PathBuf::from(format!(
        "{}.supersede",
        held.ledger(&held.predecessor).display()
    ))
    .exists());
    fs::remove_dir(lock).unwrap();
    let old = owner_value(&held, &["work", "resume"]);
    assert_eq!(old["work"], held.predecessor);
}

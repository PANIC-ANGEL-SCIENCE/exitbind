//! R55 (FR54-05): the view a worker actually uses, `work detail --view action`,
//! carries the phase guidance body and the selected project lessons it was
//! delivered, each with its read route; text and `--json` name the same items.
use super::super::*;
use super::{declare, worker_assignment, TRIAGE};
use sha2::{Digest, Sha256};

const SCOPE: &str = "project-lessons.v1";
const FACT: &str = "Phase guidance must be read from the action view the worker uses.";

fn sha(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

/// A worker with a phase body at its current phase, a body over the recipient
/// budget, and one accepted project lesson that applies to the Work's goal.
fn configured() -> (Fixture, String) {
    let fixture = Fixture::new("action-view-guidance");
    let path = fixture.root.join("exitbind.json");
    let mut config: Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
    config["memory"]["protocolScopes"] = json!(["invariants", SCOPE]);
    config["agents"]["worker"]["memoryRead"] = json!(["invariants", SCOPE]);
    config["agents"]["lead"]["crossContext"] = json!("protocol-only");
    for right in [
        "memoryWrite",
        "memoryReview",
        "memoryPromote",
        "memoryRevoke",
    ] {
        config["agents"]["lead"][right] = json!(["invariants", SCOPE]);
    }
    fs::write(&path, serde_json::to_vec_pretty(&config).unwrap()).unwrap();
    let skill = fixture.root.join("owner-skills/triage/SKILL.md");
    fs::create_dir_all(skill.parent().unwrap()).unwrap();
    fs::write(&skill, TRIAGE).unwrap();
    let large = format!(
        "# Large body\n{}",
        "complete line of guidance\n".repeat(480)
    );
    fs::write(fixture.root.join("large.md"), large).unwrap();
    declare(
        &fixture,
        "worker",
        json!([{"id":"failing-test-triage","phase":"test","path":skill.to_str().unwrap()},
            {"id":"large-body","phase":"implementation","path":"large.md"}]),
    );

    fs::write(fixture.root.join("coupling.txt"), "action view coupling\n").unwrap();
    let project =
        fixture.value(&["project", "context", "--json"], b"")["project"]["memoryIdentity"].clone();
    let lesson = json!({"version":1,"id":"read-guidance-on-action-view","fact":FACT,
        "projectIdentity":project,"owner":"lead","provenance":["reviewed fixture coupling"],
        "createdRevision":"82a77f9cb4675bb515c294ab16a1d70d6b5e97c4",
        "revalidatedRevision":sha(b"reviewed revision"),
        "appliesTo":{"agents":["worker"],"taskTerms":["phase"]},
        "guards":[{"path":"coupling.txt","sha256":sha(b"action view coupling\n")}]});
    fs::write(
        fixture.root.join("lesson.json"),
        serde_json::to_vec(&lesson).unwrap(),
    )
    .unwrap();
    let ledger = "memory/read-guidance-on-action-view.jsonl";
    fixture.value(
        &[
            "memory",
            "propose",
            "lead",
            "lesson.json",
            "--scope",
            SCOPE,
            "--expires-at",
            "2100-01-01T00:00:00Z",
            "--ledger",
            ledger,
        ],
        b"",
    );
    fixture.value(&["memory", "review", "lead", ledger], b"");
    fixture.value(&["memory", "promote", "lead", ledger], b"");
    let (work, _) = worker_assignment(&fixture);
    (fixture, work)
}

fn view_text(fixture: &Fixture, work: &str, view: &str) -> String {
    let output = fixture.call(&["work", "detail", work, "--view", view], b"");
    assert!(output.status.success(), "{}", text(&output));
    String::from_utf8(output.stdout).unwrap()
}

/// The line after `header` must be the read route naming every argv element.
fn assert_route_follows(readable: &str, header: &str, route: &Value) {
    let lines: Vec<&str> = readable.lines().collect();
    let at = lines
        .iter()
        .position(|line| line.starts_with(header))
        .unwrap_or_else(|| panic!("no line starting {header:?}: {readable}"));
    let line = lines[at + 1];
    assert!(line.starts_with("  read route: "), "{line}");
    let argv = route["command"].as_array().expect("route has argv");
    assert!(!argv.is_empty());
    for argument in argv {
        assert!(
            line.contains(argument.as_str().unwrap()),
            "{argument} missing from {line}"
        );
    }
}

#[test]
fn worker_action_view_text_carries_phase_body_and_lesson_with_read_routes() {
    let (fixture, work) = configured();
    let readable = view_text(&fixture, &work, "action");

    assert!(
        readable.contains(TRIAGE),
        "phase body bytes missing: {readable}"
    );
    assert!(readable.contains(&format!("Lesson read-guidance-on-action-view: {FACT}")));
    assert!(readable.contains("Delivered guidance for worker (phases: implementation, test)"));
    let lines: Vec<&str> = readable.lines().collect();
    let phase = lines
        .iter()
        .position(|line| {
            line.starts_with("Guidance phase_guidance failing-test-triage, phase test")
        })
        .unwrap_or_else(|| panic!("{readable}"));
    assert!(
        lines[phase + 1].contains("work expand"),
        "{}",
        lines[phase + 1]
    );
    assert!(
        lines[phase + 1].contains(":recipient:"),
        "{}",
        lines[phase + 1]
    );
    let lesson = lines
        .iter()
        .position(|line| line.starts_with("Lesson read-guidance-on-action-view:"))
        .unwrap();
    assert!(
        lines[lesson + 1].contains("memory resolve worker"),
        "{}",
        lines[lesson + 1]
    );
    // Nothing silently clipped: the over-budget body is counted with its route.
    assert!(readable.contains("1 omitted"), "{readable}");
    let omitted = lines
        .iter()
        .find(|line| line.starts_with("Omitted large-body (recipient_budget): read route: "))
        .unwrap_or_else(|| panic!("{readable}"));
    assert!(omitted.contains("project context guidance phase:large-body --agent worker"));
    assert!(!readable.contains("complete line of guidance\ncomplete"));
}

#[test]
fn action_view_text_and_json_name_the_same_guidance_items_and_routes() {
    let (fixture, work) = configured();
    let readable = view_text(&fixture, &work, "action");
    let view = fixture.value(
        &["work", "detail", &work, "--view", "action", "--json"],
        b"",
    );
    let guidance = &view["guidance"];
    let items = guidance["items"]
        .as_array()
        .expect("action view guidance items");
    assert_eq!(items.len(), 2, "{guidance}");
    assert_eq!(guidance["itemCount"], 2);

    // The JSON items are the complete detail's own selection, not a second one.
    let detail = fixture.value(&["work", "detail", &work, "--json"], b"");
    let delivered: Vec<&Value> = detail["recipientContext"]["guidance"]["items"]
        .as_array()
        .unwrap()
        .iter()
        .map(|item| &item["reference"]["itemId"])
        .collect();
    let projected: Vec<&Value> = items.iter().map(|item| &item["itemId"]).collect();
    assert_eq!(projected, delivered);

    let mut json_ids = Vec::new();
    for item in items {
        let id = item["id"].as_str().unwrap();
        json_ids.push(id.to_owned());
        let content = item["content"].as_str().unwrap();
        assert!(readable.contains(content), "{id} body missing from text");
        let header = if item["kind"] == "project_lesson" {
            format!("Lesson {id}: {content}")
        } else {
            format!("Guidance {} {id}", item["kind"].as_str().unwrap())
        };
        assert_route_follows(&readable, &header, &item["readRoute"]);
    }
    let mut text_ids: Vec<String> = readable
        .lines()
        .filter_map(|line| {
            if let Some(rest) = line.strip_prefix("Lesson ") {
                return rest.split(':').next().map(str::to_owned);
            }
            let rest = line.strip_prefix("Guidance ")?;
            let name = rest.split(' ').nth(1)?;
            Some(name.trim_end_matches([',', ':']).to_owned())
        })
        .collect();
    json_ids.sort();
    text_ids.sort();
    assert_eq!(text_ids, json_ids, "{readable}");

    let omitted = guidance["omitted"].as_array().unwrap();
    assert_eq!(guidance["omittedCount"], omitted.len());
    let text_omitted: Vec<&str> = readable
        .lines()
        .filter(|line| line.starts_with("Omitted "))
        .collect();
    assert_eq!(text_omitted.len(), omitted.len(), "{readable}");
    for (line, item) in text_omitted.iter().zip(omitted) {
        assert!(line.contains(item["id"].as_str().unwrap()));
        for argument in item["readRoute"]["command"].as_array().unwrap() {
            assert!(line.contains(argument.as_str().unwrap()), "{line}");
        }
    }
}

/// The views `work detail --view` accepts that this runtime test covers.
const COVERED_VIEWS: &[&str] = &["action"];

/// Run the current detail route exactly as emitted and compare its delivery.
fn run_route(fixture: &Fixture, work: &str, next: &Value, detail: &Value) -> String {
    let route = &next["full"];
    assert_eq!(route["readOnly"], true);
    assert_eq!(next["work"], work);
    assert_eq!(route["binding"], next["current"]["binding"]);
    assert_eq!(route["binding"], detail["binding"]);
    assert_eq!(next["currentDelivery"]["binding"], detail["binding"]);
    for field in ["actor", "agent", "assignment", "nativeName", "role"] {
        assert_eq!(
            next["current"]["recipient"][field],
            detail["recipient"][field]
        );
    }
    let recipient = serde_json::to_vec(&detail["recipientContext"]).unwrap();
    assert_eq!(
        next["currentDelivery"]["contextIdentity"]["recipientSha256"],
        sha(&recipient)
    );
    assert_eq!(
        next["currentDelivery"]["inputsSha256"],
        detail["sections"]["assignment"]["residual"]["snapshot"]["inputsSha256"]
    );
    for flag in ["sameExecutableRequired", "sameConfigRequired"] {
        assert_eq!(
            route.get(flag).or_else(|| next["routeDefaults"].get(flag)),
            Some(&json!(false)),
            "{flag} must describe the complete emitted command"
        );
    }
    let argv: Vec<&str> = route["command"]
        .as_array()
        .expect("route argv")
        .iter()
        .map(|item| item.as_str().unwrap())
        .collect();
    assert_eq!(argv.len(), 7, "complete current detail route: {argv:?}");
    assert_eq!(
        std::fs::canonicalize(argv[0]).unwrap(),
        std::fs::canonicalize(env!("CARGO_BIN_EXE_exitbind")).unwrap()
    );
    assert_eq!(&argv[1..6], &["work", "detail", work, "--json", "--config"]);
    assert_eq!(
        fixture.root.join(argv[6]).canonicalize().unwrap(),
        fixture.root.join("exitbind.json").canonicalize().unwrap()
    );
    let output = super::super::support::git_topology::command(argv[0])
        .args(&argv[1..])
        .current_dir(&fixture.root)
        .output()
        .unwrap();
    assert!(output.status.success(), "{argv:?}: {}", text(&output));
    let routed: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(routed["kind"], "work_detail");
    assert_eq!(routed["complete"], true);
    assert_eq!(routed["work"], work);
    assert_eq!(routed["binding"], detail["binding"]);
    assert_eq!(routed["recipient"], detail["recipient"]);
    assert_eq!(routed["recipientContext"], detail["recipientContext"]);
    for section in ["assignment", "evidence", "tasks"] {
        assert_eq!(routed["sections"][section], detail["sections"][section]);
    }
    let profile = routed["recipientContext"]["profile"]["content"]
        .as_str()
        .unwrap();
    assert_eq!(
        routed["recipientContext"]["profile"]["sha256"],
        sha(profile.as_bytes())
    );
    serde_json::to_string(&routed).unwrap()
}

fn subagent_start(fixture: &Fixture, native: &str) -> String {
    let payload =
        json!({"hook_event_name":"SubagentStart","cwd":fixture.root.path(),"agent_type":native});
    let mut child = super::super::support::git_topology::command(env!("CARGO_BIN_EXE_exitbind"))
        .arg("hook-run")
        .current_dir(&fixture.root)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    use std::io::Write;
    // A command may refuse and exit before reading stdin; its output is still asserted.
    if let Err(error) = child
        .stdin
        .take()
        .unwrap()
        .write_all(payload.to_string().as_bytes())
    {
        assert_eq!(error.kind(), std::io::ErrorKind::BrokenPipe, "{error}");
    }
    let output = child.wait_with_output().unwrap();
    assert!(output.status.success(), "{}", text(&output));
    let value: Value = serde_json::from_slice(&output.stdout).unwrap();
    value["hookSpecificOutput"]["additionalContext"]
        .as_str()
        .unwrap()
        .to_owned()
}

/// FR54-05 as a pattern: every surface a recipient reads its assignment from
/// carries the delivered phase body and lesson, or the exact route that
/// returns them. The repository source contract checks that new views join
/// the runtime test's covered list.
#[test]
fn every_recipient_surface_carries_the_delivered_guidance_or_its_route() {
    let (fixture, work) = configured();
    let escaped = serde_json::to_string(TRIAGE).unwrap();
    let escaped = escaped.trim_matches('"');

    // work detail (complete; JSON is its only form): carries.
    let detail = fixture.value(&["work", "detail", &work, "--json"], b"");
    let full = serde_json::to_string(&detail["recipientContext"]["guidance"]).unwrap();
    assert!(full.contains(escaped) && full.contains(FACT), "{full}");
    let lesson_id = detail["recipientContext"]["guidance"]["items"]
        .as_array()
        .unwrap()
        .iter()
        .find(|item| item["reference"]["scope"] == SCOPE)
        .map(|item| item["reference"]["itemId"].as_str().unwrap().to_owned())
        .expect("lesson delivered");

    // Each covered work detail view carries guidance in text and JSON.
    let mut views = COVERED_VIEWS.to_vec();
    views.sort_unstable();
    assert!(!views.is_empty(), "no detail views are exercised");
    for &name in &views {
        let readable = view_text(&fixture, &work, name);
        assert!(readable.contains(TRIAGE) && readable.contains(FACT));
        let view = fixture.value(&["work", "detail", &work, "--view", name, "--json"], b"");
        let carried = serde_json::to_string(&view["guidance"]["items"]).unwrap();
        assert!(
            carried.contains(escaped) && carried.contains(FACT),
            "{name}: {carried}"
        );
    }

    // work next: the exact current full-detail route returns the same items.
    let next = fixture.value(&["work", "next", &work, "--json"], b"");
    let routed = run_route(&fixture, &work, &next, &detail);
    assert!(routed.contains(escaped) && routed.contains(FACT));

    // SubagentStart context for the recipient: carries (its own envelope
    // budget; an omitted item keeps its scoped read route).
    let native = detail["recipientContext"]["nativeName"].as_str().unwrap();
    let context = subagent_start(&fixture, native);
    assert!(context.contains(FACT), "{context}");
    assert!(context.contains("phase:failing-test-triage"), "{context}");
    assert!(
        context.contains("\"guidance\",\"phase:large-body\""),
        "the omitted body keeps its route: {context}"
    );

    // brief (plan-only, not bound to an assignment phase): names the lesson
    // by item and source route.
    let brief = fixture.value(
        &[
            "brief",
            "worker",
            "--task",
            "Deliver phase guidance",
            "--json",
        ],
        b"",
    );
    assert!(brief["memoryReferences"]
        .as_array()
        .unwrap()
        .iter()
        .any(|reference| reference["itemId"] == lesson_id.as_str()
            && reference["sourcePath"].is_string()));

    // work child context for a prepared native child: carries (its own
    // 3 KiB envelope; an omitted item keeps its route).
    let child_work = preserved_worker(&fixture);
    let prepared = fixture.prepare_named_child(&child_work);
    let intent = prepared["intent"].as_str().expect("prepared child intent");
    let child = fixture.value(&["work", "child", "context", &child_work, intent], b"");
    let carried = serde_json::to_string(&child["guidance"]).unwrap();
    assert!(carried.contains(FACT), "{carried}");
    assert!(
        carried.contains(escaped) || carried.contains("phase:failing-test-triage"),
        "{carried}"
    );

    // The Codex native-action prompt (`work act`): carries.
    let capture = fixture.capture("native-action-prompt");
    let codex = fixture.fake_codex(&capture);
    fixture.act_worker(&work, &codex);
    let prompt = fs::read_to_string(&capture).unwrap();
    assert!(prompt.contains(FACT), "{prompt}");
    assert!(
        prompt.contains(TRIAGE)
            || prompt.contains(escaped)
            || prompt.contains("phase:failing-test-triage"),
        "{prompt}"
    );

    // The refusal names the same views exercised above.
    let refused = fixture.call(&["work", "detail", &work, "--view", "uncovered"], b"");
    assert!(!refused.status.success());
    assert!(text(&refused).contains(&format!("--view accepts {}", views.join(", "))));
}

/// A worker assignment on a Work with a preserved requirement, so a named
/// native child can be prepared for it.
fn preserved_worker(fixture: &Fixture) -> String {
    fs::write(
        fixture.root.join("child-perspective.md"),
        "named child perspective\n",
    )
    .unwrap();
    let started = fixture.value(
        &[
            "work",
            "begin",
            "change",
            "--goal",
            "Deliver phase guidance to a prepared child",
            "--check-command",
            "true",
            "--proof-origin",
            "synthetic",
            "--review-policy",
            "required",
            "--preserve-requirement",
            "guidance:Preserve the frozen memory boundary.",
            "--preservation-check-command",
            "true",
        ],
        b"",
    );
    let work = started["work"].as_str().unwrap().to_owned();
    let lead = started["next"]["assignment"].as_str().unwrap();
    let returned = fixture.call(
        &["work", "return", &work, lead, "--outcome", "scoped"],
        b"scope",
    );
    assert!(returned.status.success(), "{}", text(&returned));
    work
}

#[test]
fn default_roles_reach_recipient_views_and_the_optional_reference_is_readable() {
    let fixture = Fixture::new("starter-competence-delivery");
    let started = fixture.value(
        &[
            "work",
            "begin",
            "change",
            "--goal",
            "Trace a maintained contract",
            "--check-command",
            "true",
            "--proof-origin",
            "synthetic",
            "--review-policy",
            "required",
        ],
        b"",
    );
    let work = started["work"].as_str().unwrap();
    let lead = fixture.value(&["work", "detail", work, "--json"], b"");
    let lead_profile = lead["recipientContext"]["profile"]["content"]
        .as_str()
        .unwrap();
    assert!(lead_profile.contains("Reuse relevant current work"));
    assert!(lead_profile.contains("references/competence.md"));
    fixture.value(
        &[
            "work",
            "return",
            work,
            started["next"]["assignment"].as_str().unwrap(),
            "--outcome",
            "scoped",
        ],
        b"scope",
    );

    let detail = fixture.value(&["work", "detail", work, "--json"], b"");
    let profile = detail["recipientContext"]["profile"]["content"]
        .as_str()
        .unwrap();
    assert!(profile.contains("trace the contract to its consumers"));
    assert!(profile.contains("references/competence.md"));
    assert!(detail["recipientContext"]["guidance"]
        .get("phaseGuidance")
        .is_none());
    let next = fixture.value(&["work", "next", work, "--json"], b"");
    let routed = run_route(&fixture, work, &next, &detail);
    assert!(routed.contains("trace the contract to its consumers"));
    let context = subagent_start(&fixture, "worker");
    assert!(
        context.contains("trace the contract to its consumers"),
        "{context}"
    );
    assert!(context.contains("references/competence.md"));
    assert!(context.encode_utf16().count() <= 10_000);

    let skill_dir = fixture.root.join(".agents/skills/exitbind");
    let skill = fs::read_to_string(skill_dir.join("SKILL.md")).unwrap();
    assert!(skill.contains("references/competence.md"));
    let reference = fs::read_to_string(skill_dir.join("references/competence.md")).unwrap();
    assert!(reference.contains("small permitted observation"));
    assert!(reference.contains("shortest path to a"));
    assert!(!context.contains("Repeating the same failure with unchanged inputs"));

    let capture = fixture.capture("starter-competence-prompt");
    let codex = fixture.fake_codex(&capture);
    fixture.act_worker(work, &codex);
    let prompt = fs::read_to_string(capture).unwrap();
    assert!(prompt.contains("trace the contract to its consumers"));
    assert!(prompt.contains("references/competence.md"));
    let checked = fixture.value(&["work", "check", work], b"");
    assert_eq!(checked["next"]["role"], "reviewer");
    let reviewer = fixture.value(&["work", "detail", work, "--json"], b"");
    let reviewer_profile = reviewer["recipientContext"]["profile"]["content"]
        .as_str()
        .unwrap();
    assert!(reviewer_profile.contains("decisive negative cases"));
    assert!(reviewer_profile.contains("references/competence.md"));
}

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

fn action_text(fixture: &Fixture, work: &str) -> String {
    let output = fixture.call(&["work", "detail", work, "--view", "action"], b"");
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
    let readable = action_text(&fixture, &work);

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
    let readable = action_text(&fixture, &work);
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

/// The views `work detail --view` accepts that the inventory below covers.
const COVERED_VIEWS: &[&str] = &["action"];

/// Run an emitted route, restoring an executable or config it left out.
fn run_route(fixture: &Fixture, route: &Value) -> String {
    let mut argv: Vec<String> = route["command"]
        .as_array()
        .expect("route argv")
        .iter()
        .map(|item| item.as_str().unwrap().to_owned())
        .collect();
    if route["sameExecutableRequired"] == true {
        argv.insert(0, env!("CARGO_BIN_EXE_exitbind").to_owned());
    }
    if argv.last().map(String::as_str) == Some("--config") {
        argv.push("exitbind.json".to_owned());
    }
    let output = super::super::support::git_topology::command(&argv[0])
        .args(&argv[1..])
        .current_dir(&fixture.root)
        .output()
        .unwrap();
    assert!(output.status.success(), "{argv:?}: {}", text(&output));
    String::from_utf8(output.stdout).unwrap()
}

fn subagent_start(fixture: &Fixture, native: &str) -> String {
    let payload = json!({"hook_event_name":"SubagentStart","cwd":fixture.root,"agent_type":native});
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
/// returns them. A new `work detail --view` fails here until it is covered.
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

    // work detail --view action, text and --json: carries.
    let readable = action_text(&fixture, &work);
    assert!(readable.contains(TRIAGE) && readable.contains(FACT));
    let view = fixture.value(
        &["work", "detail", &work, "--view", "action", "--json"],
        b"",
    );
    let carried = serde_json::to_string(&view["guidance"]["items"]).unwrap();
    assert!(
        carried.contains(escaped) && carried.contains(FACT),
        "{carried}"
    );

    // work next: the exact grouped detail route returns the same items.
    let next = fixture.value(&["work", "next", &work, "--json"], b"");
    let routed = run_route(&fixture, &next["current"]["details"]["grouped"]);
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
    let capture = fixture.root.join("native-action-prompt");
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

    // Every declared `work detail --view` is covered above: the list is the
    // one the CLI accepts, read from its declaration.
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let readable = fs::read_to_string(root.join("src/work/readable.rs")).unwrap();
    let declared = readable
        .split("pub(crate) const DETAIL_VIEWS: &[&str] = &[")
        .nth(1)
        .and_then(|rest| rest.split("];").next())
        .expect("declared work detail views");
    let views: Vec<&str> = declared
        .split(',')
        .map(|name| name.trim().trim_matches('"'))
        .filter(|name| !name.is_empty())
        .collect();
    assert!(!views.is_empty());
    for name in &views {
        assert!(
            COVERED_VIEWS.contains(name),
            "work detail --view {name} is not in the recipient surface inventory"
        );
    }
    let cli = fs::read_to_string(root.join("src/cli/work/detail.rs")).unwrap();
    assert!(
        cli.contains("DETAIL_VIEWS.contains(&view)"),
        "the CLI accepts the declared list"
    );
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

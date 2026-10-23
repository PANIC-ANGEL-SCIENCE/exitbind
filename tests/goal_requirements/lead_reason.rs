//! R55 F15 (FR55-13; F13 review D1, D2): a Lead act records the Lead's
//! reason, and spends the saved Lead decisions of the Work it acts on.
//!
//! The inventory reads the Lead outcomes from the result contract and the
//! commands that take `--reason` from the CLI source, then acts once per Lead
//! form and outcome: each form's text must be on the `submit` event and on
//! the state's submission item (`run inspect`), byte-identical. The second
//! fixture saves two decisions while W is the current Work and one with no
//! current Work, and the Lead's act on W records and removes only W's.
use super::pause_record::{filled, ok_json};
use super::session_adoption::session_start;
use super::*;

const ARTIFACT: &str = "Deterministic Lead fixture result; not a semantic decision.\n";

/// Commands whose option list takes `--reason` and whose act is the Lead's
/// `submit` on a Work: each must record the text.
const LEAD_ACTS: [&str; 4] = ["work return", "work act", "work disposition", "run submit"];
/// Commands that take `--reason` but write no `submit` event: not a Lead act
/// on a Work, each keeps its text in its own record.
const NOT_LEAD_ACTS: [&str; 13] = [
    "goal quarantine",
    "goal recover",
    "memory choose",
    "memory correct-policy",
    "memory revalidate",
    "project advance amend",
    "project advance select",
    "project advance stop",
    "project architecture select",
    "project capabilities select",
    "run review-policy",
    "src/cli/work_quarantine.rs:command",
    "work replace-check",
];

fn source(path: &str) -> String {
    fs::read_to_string(Path::new(env!("CARGO_MANIFEST_DIR")).join(path)).unwrap()
}

/// The Lead outcomes of the result contract, in source order.
fn lead_outcomes() -> Vec<String> {
    let contract = source("src/kernel/result_contract.rs");
    let block = contract
        .split("Self::Lead => &[")
        .nth(1)
        .and_then(|rest| rest.split(']').next())
        .expect("the result contract lists the Lead outcomes");
    block
        .split("Outcome::")
        .skip(1)
        .map(|item| item.split(',').next().unwrap().trim().to_lowercase())
        .collect()
}

/// Every CLI command whose option list includes `reason`.
fn reason_forms() -> Vec<String> {
    let mut forms = Vec::new();
    let mut files = vec![Path::new(env!("CARGO_MANIFEST_DIR")).join("src/cli")];
    while let Some(path) = files.pop() {
        if path.is_dir() {
            files.extend(
                fs::read_dir(&path)
                    .unwrap()
                    .map(|entry| entry.unwrap().path()),
            );
            continue;
        }
        let text = fs::read_to_string(&path).unwrap();
        let relative = path
            .strip_prefix(env!("CARGO_MANIFEST_DIR"))
            .unwrap()
            .to_str()
            .unwrap()
            .to_owned();
        for call in text.split("assert_options(").skip(1) {
            let call = call.trim_start();
            let name = match call.strip_prefix('"') {
                Some(rest) => rest.split('"').next().unwrap().to_owned(),
                None => format!("{relative}:{}", call.split(',').next().unwrap().trim()),
            };
            let Some(list) = call
                .split("&[")
                .nth(1)
                .and_then(|rest| rest.split(']').next())
            else {
                continue;
            };
            if list.contains("\"reason\"") {
                forms.push(name);
            }
        }
        if relative.ends_with("run_cli.rs") {
            for line in text.lines() {
                let line = line.trim();
                if line.contains("\" => &[") && line.contains("\"reason\"") {
                    forms.push(format!("run {}", line.split('"').nth(1).unwrap()));
                }
            }
        }
    }
    forms.sort();
    forms.dedup();
    forms
}

fn ledger(work: &str) -> String {
    format!(
        ".exitbind/runs/work-{}.jsonl",
        work.strip_prefix("smw_").unwrap()
    )
}

fn next(p: &Project, work: &str) -> Value {
    p.value(&["work", "next", work, "--json", "--full"], None)["next"].clone()
}

fn last_lead<'a>(items: impl Iterator<Item = &'a Value>) -> Value {
    items
        .filter(|item| item["role"] == "lead")
        .last()
        .cloned()
        .unwrap()
}

/// The last Lead `submit` event and the state's matching submission item.
fn recorded(p: &Project, work: &str) -> (Value, Value) {
    let events: Vec<Value> = fs::read_to_string(p.root.join(ledger(work)))
        .unwrap()
        .lines()
        .map(|line| serde_json::from_str(line).unwrap())
        .collect();
    let event = last_lead(events.iter().filter(|event| event["action"] == "submit"));
    let state = p.value(&["run", "inspect", &ledger(work), "--json"], None);
    let item = last_lead(state["submissions"].as_array().unwrap().iter());
    assert_eq!(item["eventSha256"], event["eventSha256"], "{state}");
    (event, item)
}

/// The text is on the event and on the submission item, byte-identical.
fn assert_recorded(p: &Project, work: &str, form: &str, text: &str) {
    let (event, item) = recorded(p, work);
    if event["outcome"] == "disposition" {
        // A disposition keeps its reason in its own record, unchanged.
        assert!(event.get("reason").is_none(), "{form}: {event}");
        assert_eq!(
            event["disposition"]["reason"].as_str(),
            Some(text),
            "{form}"
        );
        assert_eq!(item["disposition"]["reason"].as_str(), Some(text), "{form}");
    } else {
        assert_eq!(event["reason"].as_str(), Some(text), "{form}: {event}");
        assert_eq!(item["reason"].as_str(), Some(text), "{form}: {item}");
    }
}

fn action_view(p: &Project, work: &str) -> Value {
    p.value(
        &["work", "detail", work, "--view", "action", "--json"],
        None,
    )
}

fn choice_labels(view: &Value) -> Vec<String> {
    view["effectiveAction"]["choices"]
        .as_array()
        .unwrap()
        .iter()
        .map(|choice| choice["label"].as_str().unwrap().to_owned())
        .collect()
}

/// Run the printed choice `label` through `work fill` with `--reason`.
fn fill(p: &Project, work: &str, label: &str, text: &str) {
    let view = action_view(p, work);
    let choice = view["effectiveAction"]["choices"]
        .as_array()
        .unwrap()
        .iter()
        .find(|choice| choice["label"] == label)
        .unwrap_or_else(|| panic!("no printed choice {label}: {view}"));
    assert_eq!(choice["fill"]["options"]["REASON"], "--reason", "{choice}");
    let route = &view["effectiveAction"]["fillRoute"];
    let mut argv: Vec<String> = route["argv"]
        .as_array()
        .unwrap()
        .iter()
        .map(|item| item.as_str().unwrap().to_owned())
        .collect();
    // A bounded route names what to supply: the executable and the config.
    if route["sameExecutableRequired"] == true {
        argv.insert(0, env!("CARGO_BIN_EXE_exitbind").to_owned());
    }
    if route["sameConfigRequired"] == true {
        let config = p.root.join("exitbind.json");
        argv.push(config.to_str().unwrap().to_owned());
    }
    argv.extend(["--choice", label, "--reason", text].map(str::to_owned));
    if let Some(option) = choice["fill"]["artifactOption"].as_str() {
        let artifact = p.root.join("lead-fill-artifact.md");
        fs::write(&artifact, ARTIFACT).unwrap();
        argv.extend([option.to_owned(), artifact.to_str().unwrap().to_owned()]);
    }
    let output = p.call_exact_argv(&argv);
    assert!(output.status.success(), "{label}: {output:?}");
}

/// The Lead's `work return` of `outcome` with `--reason`; a disposition
/// carries its reason inside the record the Lead returns.
fn give(p: &Project, work: &str, outcome: &str, text: &str) {
    use sha2::{Digest, Sha256};
    let lead = next(p, work);
    let assignment = lead["assignment"].as_str().unwrap();
    if outcome == "disposition" {
        let mut record = json!({"decision": "defer", "reason": text,
            "findingSha256s": lead["packet"]["pendingDisposition"]["findingSha256s"],
            "basisSha256": lead["packet"]["basisSha256"]});
        let digest = Sha256::digest(canonical(&record).as_bytes());
        record["sha256"] = json!(format!("{digest:x}"));
        let record = record.to_string();
        let args = [
            "work",
            "return",
            work,
            assignment,
            "--outcome",
            outcome,
            "--disposition",
            &record,
        ];
        p.value(&args, Some(ARTIFACT));
    } else {
        let args = [
            "work",
            "return",
            work,
            assignment,
            "--outcome",
            outcome,
            "--reason",
            text,
        ];
        p.value(&args, Some(ARTIFACT));
    }
}

fn canonical(value: &Value) -> String {
    match value {
        Value::Object(object) => {
            let mut keys = object.keys().collect::<Vec<_>>();
            keys.sort();
            let fields = keys
                .into_iter()
                .map(|key| format!("{}:{}", json!(key), canonical(&object[key])))
                .collect::<Vec<_>>();
            format!("{{{}}}", fields.join(","))
        }
        Value::Array(items) => {
            format!(
                "[{}]",
                items.iter().map(canonical).collect::<Vec<_>>().join(",")
            )
        }
        _ => value.to_string(),
    }
}

/// Drive from the current action to the Lead decision after the reviewer
/// returns `review`.
fn to_lead(p: &Project, work: &str, review: &str) -> Value {
    let mut reviewed = false;
    for _ in 0..8 {
        let current = next(p, work);
        let assignment = current["assignment"].as_str().unwrap_or_default();
        match (
            current["action"].as_str().unwrap(),
            current["role"].as_str(),
        ) {
            ("lead_decision", _) if reviewed => return current,
            ("spawn", Some("worker")) => {
                let permit = [
                    "work",
                    "permit",
                    work,
                    assignment,
                    "--operation",
                    "Fixture edit",
                ];
                p.value(&permit, None);
                let done = ["work", "return", work, assignment, "--outcome", "completed"];
                p.value(&done, Some("Fixture implementation.\n"));
            }
            ("check", _) => {
                p.value(&["work", "check", work], None);
            }
            ("spawn", Some("reviewer")) => {
                let args = ["work", "return", work, assignment, "--outcome", review];
                p.value(&args, Some("Finding: fixture review.\n"));
                reviewed = true;
            }
            (action, _) => panic!("unexpected {action}: {current}"),
        }
    }
    panic!("the Work did not reach the Lead after review");
}

fn upstream_lead_reason(packet: &Value) -> Value {
    last_lead(packet["upstreamArtifacts"].as_array().unwrap().iter())["reason"].clone()
}

#[test]
fn every_lead_form_that_takes_a_reason_records_it_on_the_event_and_the_submission() {
    // The inventory, read from the source.
    let outcomes = lead_outcomes();
    assert_eq!(
        outcomes,
        [
            "scoped",
            "blocked",
            "accepted",
            "rework",
            "rejected",
            "disposition"
        ]
    );
    let mut classified: Vec<String> = LEAD_ACTS
        .iter()
        .chain(NOT_LEAD_ACTS.iter())
        .map(|form| (*form).to_owned())
        .collect();
    classified.sort();
    assert_eq!(
        reason_forms(),
        classified,
        "a command that takes --reason must be classified here"
    );

    let p = Project::new("f15-lead-forms");
    let begin = |goal: &str| p.begin_ordinary(goal, "true", &[]);
    let mut exercised: Vec<(String, String)> = Vec::new();
    let mut act = |work: &str, form: &str, outcome: &str| {
        let text = format!("{form} {outcome}: keep \"R1\" only — 경계\nsecond line");
        match form {
            "work return" => give(&p, work, outcome, &text),
            "work fill" => {
                let label = if outcome == "disposition" {
                    "defer"
                } else {
                    outcome
                };
                fill(&p, work, label, &text);
            }
            "work act" => {
                let args = ["work", "act", work, "--outcome", outcome, "--reason", &text];
                p.value(&args, None);
            }
            "work disposition" => {
                let lead = next(&p, work);
                let args = [
                    "work",
                    "disposition",
                    work,
                    lead["assignment"].as_str().unwrap(),
                    "--decision",
                    "reject",
                    "--reason",
                    &text,
                ];
                p.value(&args, None);
            }
            "run submit" => {
                p.file("lead-run-artifact.md", ARTIFACT);
                let ledger = ledger(work);
                let args = [
                    "run",
                    "submit",
                    "lead",
                    &ledger,
                    "--outcome",
                    outcome,
                    "--artifact",
                    "lead-run-artifact.md",
                    "--reason",
                    &text,
                ];
                p.value(&args, None);
            }
            other => panic!("unknown form {other}"),
        }
        assert_recorded(&p, work, form, &text);
        exercised.push((form.to_owned(), outcome.to_owned()));
        text
    };

    // work return: every Lead outcome. The Lead reads its scope reason in the
    // worker's packet, and its rework reason in the next attempt's.
    let a = begin("Lead reasons through work return");
    let scoped = act(&a, "work return", "scoped");
    assert_eq!(upstream_lead_reason(&next(&p, &a)["packet"]), scoped);
    to_lead(&p, &a, "approved");
    let rework = act(&a, "work return", "rework");
    assert_eq!(upstream_lead_reason(&next(&p, &a)["packet"]), rework);
    to_lead(&p, &a, "rework");
    act(&a, "work return", "disposition");
    act(&a, "work return", "accepted");
    let b = begin("Lead blocks through work return");
    act(&b, "work return", "blocked");
    let c = begin("Lead rejects through work return");
    act(&c, "work act", "scoped");
    to_lead(&p, &c, "rework");
    act(&c, "work return", "rejected");

    // work fill: every printed Lead choice. At its acceptance the Lead reads
    // the reason it scoped with in its own packet.
    let d = begin("Lead reasons through work fill");
    let scoped = act(&d, "work fill", "scoped");
    let lead = to_lead(&p, &d, "approved");
    assert_eq!(upstream_lead_reason(&lead["packet"]), scoped);
    act(&d, "work fill", "rework");
    to_lead(&p, &d, "rework");
    let pending = choice_labels(&action_view(&p, &d));
    assert!(
        !pending.contains(&"rejected".to_owned()),
        "a printed rejected choice needs its fill case here: {pending:?}"
    );
    act(&d, "work fill", "disposition");
    act(&d, "work fill", "accepted");
    let e = begin("Lead blocks through work fill");
    act(&e, "work fill", "blocked");

    // work disposition, then run submit on the same Work.
    let f = begin("Lead disposes and submits directly");
    give(&p, &f, "scoped", "scope");
    to_lead(&p, &f, "rework");
    act(&f, "work disposition", "disposition");
    act(&f, "run submit", "blocked");

    for outcome in &outcomes {
        assert!(
            exercised.contains(&("work return".into(), outcome.clone())),
            "work return {outcome}"
        );
        if outcome != "rejected" {
            assert!(
                exercised.contains(&("work fill".into(), outcome.clone())),
                "work fill {outcome}"
            );
        }
    }
    for form in LEAD_ACTS {
        assert!(exercised.iter().any(|(done, _)| done == form), "{form}");
    }

    // An old ledger without the field reduces as before: strip the reason
    // from B's last event and the state is the same less that field.
    let path = p.root.join(ledger(&b));
    let (_, with_reason) = recorded(&p, &b);
    let before = p.value(&["run", "inspect", &ledger(&b), "--json"], None);
    let source = fs::read_to_string(&path).unwrap();
    let (head, last) = source.trim_end().rsplit_once('\n').unwrap();
    let mut last: Value = serde_json::from_str(last).unwrap();
    let object = last.as_object_mut().unwrap();
    object.remove("reason").unwrap();
    object.remove("eventSha256");
    let digest = {
        use sha2::{Digest, Sha256};
        Sha256::digest(canonical(&last).as_bytes())
    };
    last["eventSha256"] = json!(format!("{digest:x}"));
    fs::write(&path, format!("{head}\n{last}\n")).unwrap();
    let after = p.value(&["run", "inspect", &ledger(&b), "--json"], None);
    assert_eq!(after["valid"], true, "{after}");
    for field in ["status", "currentStage", "attempt", "assignments"] {
        assert_eq!(after[field], before[field], "{field}");
    }
    let (_, without) = recorded(&p, &b);
    let mut expected = with_reason.clone();
    expected.as_object_mut().unwrap().remove("reason");
    expected["eventSha256"] = without["eventSha256"].clone();
    assert_eq!(without, expected);
}

const STEP: &str = "Accept R1 after the review";
const UNBOUND: &str = "Keep the import order; decided before any Work was selected";
const DECISIONS: [&str; 2] = [
    "R1 scope: refuse invalid names before persistence only",
    "A repeated failure of the import check is a product defect",
];
const LEAD_TEXT: &str = "Accepted: the check and the review hold for R1.";

/// Record `work` as the session's current Work through the JSON record form,
/// keeping `decisions`.
fn select(p: &Project, work: &str, decisions: &[&str]) {
    let status = p.status();
    let member = status["journeyMembers"]
        .as_array()
        .unwrap()
        .iter()
        .find(|member| member["work"] == work)
        .unwrap_or_else(|| panic!("{work} is a Goal member: {status}"))
        .clone();
    let (identity, _) = p.native_session.as_ref().unwrap();
    let input = json!({
        "sessionId": identity, "currentStep": STEP,
        "currentWork": {"state": "known", "work": work},
        "workRefs": [{"work": member["work"], "configPath": member["configPath"],
            "configSha256": member["configSha256"],
            "startEventSha256": member["startEventSha256"], "relation": "member"}],
        "latestCorrections": decisions,
    });
    p.value(&["session", "recovery", "record"], Some(&input.to_string()));
}

fn saved_decisions(p: &Project) -> Value {
    p.value(&["session", "recovery", "show", "--json"], None)["activeSavedStep"]["leadDecisions"]
        .clone()
}

#[test]
fn a_lead_act_records_and_spends_only_the_saved_decisions_of_its_work() {
    let p = Project::new("f15-spend").with_native_session("f15-spend-session");
    p.setup();
    let work = p.begin("R1", "sh check.sh R1");
    let other = p.begin("R2", "sh check.sh R2");
    give(&p, &work, "scoped", "Scope R1.");
    let lead = to_lead(&p, &work, "approved");

    // One decision with no current Work, then two while W is current.
    let routes = p.status()["pauseRecord"].clone();
    let save = |slot: &str, value: &str| {
        let route = if slot == "SAVED_STEP" {
            "saveStep"
        } else {
            "saveDecision"
        };
        ok_json(&p.call_exact_argv(&filled(&routes[route], slot, value)));
    };
    save("SAVED_STEP", STEP);
    save("LEAD_DECISION", UNBOUND);
    select(&p, &work, &[UNBOUND]);
    for decision in DECISIONS {
        save("LEAD_DECISION", decision);
    }
    assert_eq!(
        saved_decisions(&p),
        json!([UNBOUND, DECISIONS[0], DECISIONS[1]])
    );

    // Above the bound the act is refused before any write.
    let ledger_path = p.root.join(ledger(&work));
    let before = fs::read(&ledger_path).unwrap();
    let long = "x".repeat(1000);
    let assignment = lead["assignment"].as_str().unwrap();
    let args = [
        "work",
        "return",
        &work,
        assignment,
        "--outcome",
        "accepted",
        "--reason",
        &long,
    ];
    let refused = p.call(&args, Some(ARTIFACT));
    assert!(!refused.status.success(), "{refused:?}");
    let refusal = String::from_utf8_lossy(&refused.stderr).into_owned();
    assert!(
        refusal.contains("1000 from --reason and 2 saved Lead decisions of this Work")
            && refusal.contains("at most 1024 bytes are recorded; no mutation was made"),
        "{refusal}"
    );
    assert_eq!(fs::read(&ledger_path).unwrap(), before);
    assert_eq!(saved_decisions(&p)[2], DECISIONS[1]);

    // The Lead acts on W: its text, then both decisions; only the unbound
    // decision stays saved.
    let args = [
        "work",
        "return",
        &work,
        assignment,
        "--outcome",
        "accepted",
        "--reason",
        LEAD_TEXT,
    ];
    p.value(&args, Some(ARTIFACT));
    let expected = format!(
        "{LEAD_TEXT}\nSaved Lead decision: {}\nSaved Lead decision: {}",
        DECISIONS[0], DECISIONS[1]
    );
    assert_recorded(&p, &work, "work return accepted", &expected);
    assert_eq!(saved_decisions(&p), json!([UNBOUND]));

    // A second Work shows none of W's decisions, and its Lead act records
    // only its own text.
    select(&p, &other, &[UNBOUND]);
    assert_eq!(saved_decisions(&p), json!([UNBOUND]));
    let hook = session_start(&p);
    let detail = p.call(&["work", "detail", &other], None);
    let shown = format!("{hook}\n{}", String::from_utf8_lossy(&detail.stdout));
    for decision in DECISIONS {
        assert!(!shown.contains(decision), "{decision}: {shown}");
    }
    give(&p, &other, "scoped", "Scope R2.");
    assert_recorded(&p, &other, "work return scoped", "Scope R2.");
    assert_eq!(saved_decisions(&p), json!([UNBOUND]));
}

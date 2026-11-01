//! Stale native reports are inspectable data, never new Work credit.
use super::*;
use sha2::{Digest, Sha256};

fn journal(f: &Fixture) -> (PathBuf, Vec<u8>, Value) {
    fs::read_dir(f.root.join(".exitbind/native-actions").join(&f.work))
        .unwrap()
        .filter_map(Result::ok)
        .filter_map(|entry| {
            let path = entry.path();
            let bytes = fs::read(&path).ok()?;
            let value: Value = serde_json::from_slice(&bytes).ok()?;
            (value["assignment"] == f.assignment && value["status"].is_string())
                .then_some((path, bytes, value))
        })
        .next()
        .expect("the existing native journal is retained")
}

fn route(f: &Fixture, value: &Value) -> Output {
    let mut words = value["command"]
        .as_array()
        .unwrap_or_else(|| panic!("missing read command: {value}"))
        .iter()
        .map(|word| word.as_str().unwrap().to_owned())
        .collect::<Vec<_>>();
    if value["sameExecutableRequired"] == true {
        words.insert(0, f.executable.to_str().unwrap().to_owned());
    }
    if value["sameConfigRequired"] == true {
        words.extend([
            "--config".into(),
            f.root.join("exitbind.json").to_str().unwrap().into(),
        ]);
    }
    support::git_topology::command(&words[0])
        .current_dir(&f.root)
        .args(&words[1..])
        .output()
        .unwrap()
}

fn no_source(output: Output) {
    if output.status.success() {
        let value: Value = serde_json::from_slice(&output.stdout).unwrap();
        assert!(value["nextAction"]["source"].is_null(), "{value}");
        assert_ne!(value["reconciliation"], "replay_ready", "{value}");
    }
}

#[test]
fn stale_managed_result_is_readonly_paged_and_current_scope_fenced() {
    let f = Fixture::new(false, false);
    let recorder = support::temp("retained-inspection-observation");
    let marker = recorder.join("starts");
    let fake = f.root.join("fake-stale-codex");
    let script = r##"#!/usr/bin/env python3
import json, subprocess, sys
lines = sys.stdin.read().splitlines()
index = next(i for i, line in enumerate(lines) if line.startswith('CURRENT READABLE ASSIGNMENT, EVIDENCE AND TASKS'))
packet = json.loads(lines[index + 1])
form = packet['currentAction']['beforeEditing']
argv = ['Fixture edit' if word == '<OPERATION>' else word for word in form['command']['argv']]
argv[argv.index('--request-id') + 1] = 'unmatched-retained-request'
reply = subprocess.run(argv, capture_output=True)
assert reply.returncode == 0, reply.stderr
assert json.loads(reply.stdout)['allowed'] is True
open(__MARKER__, 'a').write('x')
summary = '"' * 5000 + 'é' * 300
print(json.dumps({'type':'thread.started','thread_id':'retained-inspection'}))
print(json.dumps({'type':'item.completed','item':{'type':'agent_message','text':json.dumps({'outcome':'completed','summary':summary,'reason':''})}}))
print(json.dumps({'type':'turn.completed','status':'completed','usage':{'input_tokens':1,'cached_input_tokens':0,'output_tokens':1}}))
"##.replace("__MARKER__", &serde_json::to_string(marker.to_str().unwrap()).unwrap());
    fs::write(&fake, script).unwrap();
    fs::set_permissions(&fake, fs::Permissions::from_mode(0o700)).unwrap();
    let first = f.call(
        &[
            "work",
            "act",
            &f.work,
            "--codex-bin",
            fake.to_str().unwrap(),
        ],
        b"",
    );
    refused(first, "retained result has no automatic credit");
    let (journal_path, saved_journal, saved) = journal(&f);
    assert_eq!(saved["status"], "completed");
    let ledger_path = f
        .root
        .join(format!(".exitbind/runs/work-{}.jsonl", &f.work[4..]));
    let saved_ledger = fs::read(&ledger_path).unwrap();
    let inspect = f.ok(&["work", "act", &f.work, "--inspect"], b"");
    assert_eq!(inspect["credit"], "none", "{inspect}");
    assert_eq!(
        inspect["reconciliation"], "retained_uncredited",
        "{inspect}"
    );
    assert!(
        inspect.to_string().len() < 8192,
        "metadata must stay bounded"
    );
    assert!(!inspect.to_string().contains(&"\"".repeat(20)));
    let original_route = inspect["nextAction"]["source"].clone();
    assert_eq!(original_route["readOnly"], true);
    let mut next = original_route.clone();
    let mut joined = String::new();
    let mut pages = 0;
    while next.is_object() {
        let output = route(&f, &next);
        assert!(output.status.success(), "{output:?}");
        assert!(
            output.stdout.len() <= 8192,
            "page bytes: {}",
            output.stdout.len()
        );
        let page: Value = serde_json::from_slice(&output.stdout).unwrap();
        assert_eq!(page["credit"], "none", "{page}");
        assert_eq!(page["untrusted"], true, "{page}");
        assert_eq!(page["offset"].as_u64().unwrap(), joined.len() as u64);
        joined.push_str(page["resultText"].as_str().unwrap());
        next = page["next"].clone();
        pages += 1;
        assert!(pages < 32, "bounded page chain did not finish");
    }
    assert!(pages > 1, "test must exercise pagination");
    let hex = saved["result"].as_str().unwrap().as_bytes();
    let original = hex
        .chunks_exact(2)
        .map(|pair| u8::from_str_radix(std::str::from_utf8(pair).unwrap(), 16).unwrap())
        .collect::<Vec<_>>();
    assert_eq!(joined.as_bytes(), original);
    assert_eq!(
        format!("{:x}", Sha256::digest(&original)),
        inspect["result"]["sha256"].as_str().unwrap()
    );
    assert_eq!(fs::read(&journal_path).unwrap(), saved_journal);
    assert_eq!(fs::read(&ledger_path).unwrap(), saved_ledger);
    assert_eq!(fs::read_to_string(&marker).unwrap(), "x");

    let mut wrong_work = original_route.clone();
    let words = wrong_work["command"].as_array_mut().unwrap();
    let index = words.iter().position(|word| word == "expand").unwrap();
    words[index + 1] = json!(format!("smw_{}", "0".repeat(64)));
    assert!(!route(&f, &wrong_work).status.success());
    let mut forged = original_route.clone();
    let words = forged["command"].as_array_mut().unwrap();
    let index = words
        .iter()
        .position(|word| {
            word.as_str()
                .is_some_and(|v| v.starts_with("ref:native-result:v1:"))
        })
        .unwrap();
    let mut token = words[index].as_str().unwrap().to_owned();
    let position = "ref:native-result:v1:".len();
    let replacement = if &token[position..position + 1] == "0" {
        "1"
    } else {
        "0"
    };
    token.replace_range(position..position + 1, replacement);
    words[index] = json!(token);
    assert!(!route(&f, &forged).status.success());

    let mut split_codepoint = original_route.clone();
    let words = split_codepoint["command"].as_array_mut().unwrap();
    let index = words
        .iter()
        .position(|word| {
            word.as_str()
                .is_some_and(|v| v.starts_with("ref:native-result:v1:"))
        })
        .unwrap();
    let reference = words[index].as_str().unwrap();
    let (base, _) = reference.rsplit_once(':').unwrap();
    let middle = std::str::from_utf8(&original).unwrap().find('é').unwrap() + 1;
    let bad_reference = format!("{base}:{middle}");
    words[index] = json!(bad_reference);
    assert!(!route(&f, &split_codepoint).status.success());

    // Vary only the retained source in this disposable fixture. Actual CLI
    // bytes include escaping, Work decoration, pretty printing and newline.
    let mut largest = 0;
    for quotes in (1050..=1500).step_by(10) {
        use std::fmt::Write as _;
        let mut result: Value = serde_json::from_slice(&original).unwrap();
        result["summary"] = json!(format!(
            "{}{}{}",
            "\"".repeat(quotes),
            "z".repeat(5000 - quotes),
            "é".repeat(300)
        ));
        let bytes = serde_json::to_vec(&result).unwrap();
        let mut encoded = String::new();
        for byte in &bytes {
            write!(&mut encoded, "{byte:02x}").unwrap();
        }
        let mut changed = saved.clone();
        changed["result"] = json!(encoded);
        changed["resultSha256"] = json!(format!("{:x}", Sha256::digest(&bytes)));
        changed["resultBytes"] = json!(bytes.len());
        fs::write(&journal_path, serde_json::to_vec(&changed).unwrap()).unwrap();
        let inspect = f.ok(&["work", "act", &f.work, "--inspect"], b"");
        let page = route(&f, &inspect["nextAction"]["source"]);
        assert!(page.status.success(), "{page:?}");
        largest = largest.max(page.stdout.len());
        assert!(
            page.stdout.len() <= 8192,
            "{quotes} quotes: {} bytes",
            page.stdout.len()
        );
    }
    assert!(
        largest > 7900,
        "no near-boundary response observed: {largest}"
    );
    fs::write(&journal_path, &saved_journal).unwrap();

    let mut changed = saved.clone();
    let mut encoded = changed["result"].as_str().unwrap().to_owned();
    let replacement = if &encoded[0..2] == "30" { "31" } else { "30" };
    encoded.replace_range(0..2, replacement);
    changed["result"] = json!(encoded);
    fs::write(&journal_path, serde_json::to_vec(&changed).unwrap()).unwrap();
    assert!(!route(&f, &original_route).status.success());
    no_source(f.call(&["work", "act", &f.work, "--inspect"], b""));
    for field in ["readSubjectSha256", "resultSha256"] {
        let mut changed = saved.clone();
        changed.as_object_mut().unwrap().remove(field);
        fs::write(&journal_path, serde_json::to_vec(&changed).unwrap()).unwrap();
        no_source(f.call(&["work", "act", &f.work, "--inspect"], b""));
    }
    let mut changed = saved.clone();
    changed["assignment"] = json!(format!("sma_{}", "0".repeat(64)));
    fs::write(&journal_path, serde_json::to_vec(&changed).unwrap()).unwrap();
    no_source(f.call(&["work", "act", &f.work, "--inspect"], b""));
    fs::write(&journal_path, &saved_journal).unwrap();
    let config_path = f.root.join("exitbind.json");
    let original_config = fs::read(&config_path).unwrap();
    let mut config: Value = serde_json::from_slice(&original_config).unwrap();
    config["agents"]["worker"]["observe"] = json!([]);
    fs::write(&config_path, serde_json::to_vec(&config).unwrap()).unwrap();
    assert!(!route(&f, &original_route).status.success());
    no_source(f.call(&["work", "act", &f.work, "--inspect"], b""));
    fs::write(config_path, original_config).unwrap();
    let resumed = f.call(&["work", "act", &f.work, "--resume"], b"");
    assert!(
        !resumed.status.success(),
        "unmatched proof must remain ineligible"
    );
    assert_eq!(fs::read(&journal_path).unwrap(), saved_journal);
    assert_eq!(fs::read(&ledger_path).unwrap(), saved_ledger);
    assert_eq!(fs::read_to_string(marker).unwrap(), "x");
}

#[test]
fn unchanged_packet_with_invalid_delivery_proof_does_not_offer_replay() {
    let f = Fixture::new(false, false);
    let recorder = support::temp("retained-eligibility-observation");
    let marker = recorder.join("starts");
    let relative = format!(".exitbind/runs/work-{}.jsonl", &f.work[4..]);
    let lock_id = format!("{:x}", Sha256::digest(relative.as_bytes()));
    let lock = f
        .root
        .join(format!(".exitbind/locks/run-v1-{lock_id}.lock"));
    let fake = f.root.join("fake-retained-codex");
    let script = r##"#!/usr/bin/env python3
import json, sys
sys.stdin.read()
open(__MARKER__, 'a').write('x')
with open(__LOCK__, 'x') as lock:
    json.dump({'pid': __PID__, 'createdAt': '2026-11-01T00:00:00Z'}, lock)
print(json.dumps({'type':'thread.started','thread_id':'retained-eligibility'}))
print(json.dumps({'type':'item.completed','item':{'type':'agent_message','text':json.dumps({'outcome':'completed','summary':'retained','reason':''})}}))
print(json.dumps({'type':'turn.completed','status':'completed','usage':{'input_tokens':1,'cached_input_tokens':0,'output_tokens':1}}))
"##
    .replace("__MARKER__", &serde_json::to_string(marker.to_str().unwrap()).unwrap())
    .replace("__LOCK__", &serde_json::to_string(lock.to_str().unwrap()).unwrap())
    .replace("__PID__", &std::process::id().to_string());
    fs::write(&fake, script).unwrap();
    fs::set_permissions(&fake, fs::Permissions::from_mode(0o700)).unwrap();
    f.permit();
    refused(
        f.call(
            &[
                "work",
                "act",
                &f.work,
                "--codex-bin",
                fake.to_str().unwrap(),
            ],
            b"",
        ),
        "busy",
    );
    fs::remove_file(lock).unwrap();
    let (path, original, mut saved) = journal(&f);
    assert_eq!(
        f.ok(&["work", "act", &f.work, "--inspect"], b"")["reconciliation"],
        "replay_ready"
    );
    saved["requiredDeliveryProof"]["requiredFactsSha256"] = json!("0".repeat(64));
    fs::write(&path, serde_json::to_vec(&saved).unwrap()).unwrap();
    let altered = fs::read(&path).unwrap();
    let inspect = f.ok(&["work", "act", &f.work, "--inspect"], b"");
    assert_eq!(
        inspect["reconciliation"], "retained_uncredited",
        "{inspect}"
    );
    assert_eq!(inspect["credit"], "none");
    assert!(inspect["nextAction"]["source"].is_object());
    assert!(!f
        .call(&["work", "act", &f.work, "--resume"], b"")
        .status
        .success());
    assert_eq!(fs::read(&path).unwrap(), altered);
    assert_eq!(fs::read_to_string(&marker).unwrap(), "x");
    fs::write(path, original).unwrap();
    assert_eq!(
        f.ok(&["work", "act", &f.work, "--inspect"], b"")["reconciliation"],
        "replay_ready"
    );
}

#[test]
fn malformed_native_final_is_inspectable_as_uncredited_data() {
    let f = Fixture::new(false, false);
    f.permit();
    let fake = f.root.join("fake-malformed-codex");
    let script = r##"#!/usr/bin/env python3
import json, sys
sys.stdin.read()
print(json.dumps({'type':'thread.started','thread_id':'malformed-retained'}))
print(json.dumps({'type':'item.completed','item':{'type':'agent_message','text':json.dumps({'outcome':'completed','summary':'bad\nsummary','reason':''})}}))
print(json.dumps({'type':'turn.completed','status':'completed','usage':{'input_tokens':1,'cached_input_tokens':0,'output_tokens':1}}))
"##;
    fs::write(&fake, script).unwrap();
    fs::set_permissions(&fake, fs::Permissions::from_mode(0o700)).unwrap();
    let first = f.call(
        &[
            "work",
            "act",
            &f.work,
            "--codex-bin",
            fake.to_str().unwrap(),
        ],
        b"",
    );
    assert!(
        !first.status.success(),
        "invalid final must not be credited"
    );
    let (path, saved, _) = journal(&f);
    let inspected = f.ok(&["work", "act", &f.work, "--inspect"], b"");
    let source = &inspected["result"]["uncreditedSource"];
    assert_eq!(source["readOnly"], true, "{inspected}");
    let output = route(&f, source);
    assert!(output.status.success(), "{output:?}");
    let page: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(page["credit"], "none");
    assert_eq!(page["sourceClass"], "provisional_native_result");
    assert!(page["resultText"]
        .as_str()
        .unwrap()
        .contains("bad\\nsummary"));
    assert_eq!(fs::read(path).unwrap(), saved);
}

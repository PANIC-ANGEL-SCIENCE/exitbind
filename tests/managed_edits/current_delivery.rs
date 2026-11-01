//! First native submission and saved-result recovery use the same delivery proof.
use super::*;

fn completed_journal(f: &Fixture) -> Value {
    let directory = f.root.join(".exitbind/native-actions").join(&f.work);
    fs::read_dir(directory)
        .unwrap()
        .filter_map(Result::ok)
        .filter_map(|entry| fs::read(entry.path()).ok())
        .filter_map(|bytes| serde_json::from_slice::<Value>(&bytes).ok())
        .find(|value| value["status"] == "completed" && value["assignment"] == f.assignment)
        .expect("the native result remains in the existing completed journal")
}

#[test]
fn first_native_submission_requires_current_delivery_or_exact_worker_grant() {
    for mode in [
        "current-grant",
        "permit-after",
        "unmatched",
        "goal",
        "missing-proof",
        "wrong-grant",
    ] {
        let f = Fixture::new(false, false);
        if matches!(mode, "current-grant" | "missing-proof" | "wrong-grant") {
            f.permit();
        }
        let before = f.ok(&["work", "detail", &f.work, "--json"], b"");
        let recorder = support::temp("managed-delivery-observation");
        let marker = recorder.join("starts");
        let fake = f.root.join("fake-delivery-codex");
        let script = r##"#!/usr/bin/env python3
import glob, json, subprocess, sys
lines = sys.stdin.read().splitlines()
index = next(i for i, line in enumerate(lines) if line.startswith('CURRENT READABLE ASSIGNMENT, EVIDENCE AND TASKS'))
packet = json.loads(lines[index + 1])
mode = __MODE__
open(__MARKER__, 'a').write('x')
if mode in ('current-grant', 'missing-proof', 'wrong-grant'):
    assert packet['currentAction']['currentGrant']['state'] == 'current_for_this_worker_assignment'
    assert 'beforeEditing' not in packet['currentAction']
    open('src/native.txt', 'w').write('one authorized edit\n')
    if mode != 'current-grant':
        journals = []
        for path in glob.glob('.exitbind/native-actions/*/*.json'):
            with open(path) as source:
                candidate = json.load(source)
            if 'requiredDeliveryProof' in candidate:
                journals.append((path, candidate))
        assert len(journals) == 1
        path, journal = journals[0]
        if mode == 'missing-proof':
            del journal['requiredDeliveryProof']
        else:
            journal['requiredDeliveryProof']['currentGrant']['governorEventSha256'] = '0' * 64
        with open(path, 'w') as target:
            json.dump(journal, target)
else:
    form = packet['currentAction']['beforeEditing']
    assert form['required'] is True
    argv = ['Update managed fixture' if word == '<OPERATION>' else word for word in form['command']['argv']]
    if mode == 'goal':
        config = argv[argv.index('--config') + 1]
        reply = subprocess.run([argv[0], 'goal', 'incorporate', '--goal-id', 'managed-goal',
            '--goal', 'A later independent Goal', '--obligation', 'Retain the changed Goal',
            '--config', config], capture_output=True)
        assert reply.returncode == 0, reply.stderr
    else:
        if mode == 'unmatched':
            argv[argv.index('--request-id') + 1] = 'different-first-submission-request'
        reply = subprocess.run(argv, capture_output=True)
        assert reply.returncode == 0, reply.stderr
        assert json.loads(reply.stdout)['allowed'] is True
        if mode == 'permit-after':
            open('src/native.txt', 'w').write('one authorized edit\n')
print(json.dumps({'type':'thread.started','thread_id':'first-delivery'}))
print(json.dumps({'type':'item.completed','item':{'type':'agent_message','text':json.dumps({'outcome':'completed','summary':'fixture result','reason':''})}}))
print(json.dumps({'type':'turn.completed','status':'completed','usage':{'input_tokens':1,'cached_input_tokens':0,'output_tokens':1}}))
"##
        .replace("__MODE__", &serde_json::to_string(mode).unwrap())
        .replace("__MARKER__", &serde_json::to_string(marker.to_str().unwrap()).unwrap());
        fs::write(&fake, script).unwrap();
        fs::set_permissions(&fake, fs::Permissions::from_mode(0o700)).unwrap();
        let result = f.call(
            &[
                "work",
                "act",
                &f.work,
                "--codex-bin",
                fake.to_str().unwrap(),
            ],
            b"",
        );
        assert_eq!(
            fs::read_to_string(&marker).unwrap(),
            "x",
            "{mode}: {result:?}"
        );
        let journal = completed_journal(&f);
        if mode != "missing-proof" {
            assert!(
                journal["requiredDeliveryProof"]["requiredFactsSha256"].is_string(),
                "{mode}"
            );
        }
        if mode == "current-grant" {
            assert!(
                journal["requiredDeliveryProof"]["currentGrant"]["presentedInputsSha256"]
                    .is_string(),
                "{journal}"
            );
        }
        if matches!(mode, "current-grant" | "permit-after") {
            assert!(result.status.success(), "{mode}: {result:?}");
            assert_eq!(
                fs::read_to_string(f.root.join("src/native.txt")).unwrap(),
                "one authorized edit\n"
            );
            let replay = f.call(
                &[
                    "work",
                    "act",
                    &f.work,
                    "--resume",
                    "--operation",
                    &f.assignment,
                ],
                b"",
            );
            assert!(replay.status.success(), "{mode}: {replay:?}");
        } else {
            assert!(
                !result.status.success(),
                "{mode}: unexpected credit: {result:?}"
            );
            let after = f.ok(&["work", "detail", &f.work, "--json"], b"");
            let next = f.ok(&["work", "next", &f.work], b"");
            if matches!(mode, "unmatched" | "goal") {
                assert_ne!(
                    before["binding"], after["binding"],
                    "{mode}: transition must be real"
                );
            }
            assert_eq!(
                next["next"]["assignment"], f.assignment,
                "{mode}: assignment must stay pending"
            );
            let resumed = f.call(&["work", "act", &f.work, "--resume"], b"");
            assert!(!resumed.status.success(), "{mode}: {resumed:?}");
            assert!(
                completed_journal(&f)["result"].is_string(),
                "{mode}: result must remain inspectable"
            );
        }
        assert_eq!(
            fs::read_to_string(marker).unwrap(),
            "x",
            "{mode}: native process reran"
        );
    }
}

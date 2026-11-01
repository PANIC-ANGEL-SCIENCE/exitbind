use super::*;

#[test]
fn managed_observations_refuse_other_work_evidence_and_source_hardlinks() {
    let f = Fixture::new(false, false);
    f.permit();
    let other = f.ok(
        &[
            "work",
            "begin",
            "change",
            "--goal",
            "other private evidence",
            "--check-command",
            "true",
            "--review-policy",
            "omitted",
        ],
        b"",
    );
    let private = b"OTHER_WORK_PRIVATE_EVIDENCE";
    f.ok(
        &[
            "work",
            "return",
            other["work"].as_str().unwrap(),
            other["next"]["assignment"].as_str().unwrap(),
            "--outcome",
            "scoped",
        ],
        private,
    );
    let artifact = fs::read_dir(f.root.join(".exitbind/artifacts"))
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .find(|path| path.is_file() && fs::read(path).unwrap() == private)
        .unwrap();
    fs::hard_link(&artifact, f.root.join("src/evidence-alias.txt")).unwrap();
    let original = fs::read(f.reads_path()).unwrap();
    for action in ["read", "refresh", "inspect"] {
        let output = f.edit(action, "src/evidence-alias.txt", b"");
        assert!(!String::from_utf8_lossy(&output.stdout).contains("OTHER_WORK_PRIVATE_EVIDENCE"));
        assert!(!String::from_utf8_lossy(&output.stderr).contains("OTHER_WORK_PRIVATE_EVIDENCE"));
        refused(output, "single-link");
        assert_eq!(fs::read(f.reads_path()).unwrap(), original);
    }
    assert_eq!(fs::read(artifact).unwrap(), private);
    fs::write(f.root.join("src/ordinary.txt"), "ordinary").unwrap();
    f.edit_ok("read", "src/ordinary.txt", b"");
    fs::hard_link(
        f.root.join("src/ordinary.txt"),
        f.root.join("src/source-alias.txt"),
    )
    .unwrap();
    for name in ["src/ordinary.txt", "src/source-alias.txt"] {
        refused(f.edit("read", name, b""), "single-link");
    }
    fs::remove_file(f.root.join("src/source-alias.txt")).unwrap();
    assert_eq!(
        f.edit_ok("read", "src/ordinary.txt", b"")["content"],
        "ordinary"
    );
}

#[test]
fn lost_reply_and_uncertain_or_missing_effects_never_start_new_request() {
    let f = Fixture::new(false, false);
    f.permit();
    f.edit_ok("read", "src/a.txt", b"");
    f.edit_ok("edit", "src/a.txt", b"once");
    let reads_path = f.reads_path();
    let mut reads: Value = serde_json::from_slice(&fs::read(&reads_path).unwrap()).unwrap();
    reads["files"]["src/a.txt"]["request"]["status"] = json!("submitted");
    fs::write(&reads_path, reads.to_string()).unwrap();
    let inode = fs::metadata(f.root.join("src/a.txt")).unwrap().ino();
    assert_eq!(
        f.edit_ok("edit", "src/a.txt", b"once")["effect"],
        "no-change"
    );
    assert_eq!(fs::metadata(f.root.join("src/a.txt")).unwrap().ino(), inode);
    let effect = f.effect_path();
    let completed = fs::read(&effect).unwrap();
    let mut pending: Value = serde_json::from_slice(&completed).unwrap();
    pending["status"] = json!("admitted");
    fs::write(&effect, pending.to_string()).unwrap();
    refused(f.edit("refresh", "src/a.txt", b""), "uncertain");
    assert_eq!(
        f.edit_ok("edit", "src/a.txt", b"once")["effect"],
        "no-change"
    );
    assert_eq!(fs::metadata(f.root.join("src/a.txt")).unwrap().ino(), inode);
    assert_eq!(
        f.edit_ok("inspect", "src/a.txt", b"")["effectStatus"],
        "completed"
    );
    fs::write(&effect, b"{").unwrap();
    refused(f.edit("edit", "src/a.txt", b"once"), "corrupt");
    refused(f.edit("refresh", "src/a.txt", b""), "corrupt");
    fs::write(&effect, completed).unwrap();
    fs::remove_file(effect).unwrap();
    refused(f.edit("edit", "src/a.txt", b"once"), "missing effect");
    refused(
        f.edit("refresh", "src/a.txt", b""),
        "effect record is missing",
    );
    assert_eq!(fs::metadata(f.root.join("src/a.txt")).unwrap().ino(), inode);
}

#[test]
fn missing_corrupt_read_and_binding_state_cannot_be_recreated() {
    for corruption in [false, true] {
        let f = Fixture::new(false, false);
        f.permit();
        f.edit_ok("read", "src/a.txt", b"");
        if corruption {
            fs::write(f.reads_path(), b"{").unwrap();
        } else {
            fs::remove_file(f.reads_path()).unwrap();
        }
        for action in ["read", "refresh", "edit", "inspect"] {
            refused(
                f.edit(action, "src/a.txt", b"content"),
                if corruption { "corrupt" } else { "missing" },
            );
        }
        refused(
            f.call(&["work", "file", "prepare", &f.work, &f.assignment], b""),
            if corruption { "corrupt" } else { "missing" },
        );
        assert!(!f.root.join("src/a.txt").exists());
    }
    let f = Fixture::new(false, false);
    fs::remove_file(f.tool.parent().unwrap().join("binding.json")).unwrap();
    refused(f.edit("read", "src/a.txt", b""), "binding is missing");
    refused(
        f.call(&["work", "file", "prepare", &f.work, &f.assignment], b""),
        "binding is missing",
    );
}

#[test]
fn whole_session_loss_and_interrupted_initialization_refuse_reconstruction() {
    for loss in ["session", "subtree", "interrupted"] {
        let f = Fixture::new(false, false);
        f.permit();
        fs::write(f.root.join("src/a.txt"), "A").unwrap();
        f.edit_ok("read", "src/a.txt", b"");
        fs::write(f.root.join("src/a.txt"), "external B").unwrap();
        let directory = f.tool.parent().unwrap();
        fs::remove_dir_all(if loss == "subtree" {
            directory.parent().unwrap()
        } else {
            directory
        })
        .unwrap();
        if loss == "interrupted" {
            fs::create_dir(directory).unwrap();
        }
        refused(
            f.call(&["work", "file", "prepare", &f.work, &f.assignment], b""),
            "missing",
        );
        let provider = f.root.join("must-not-launch");
        fs::write(&provider, "#!/bin/sh\ntouch provider-started\nexit 99\n").unwrap();
        fs::set_permissions(&provider, fs::Permissions::from_mode(0o700)).unwrap();
        refused(
            f.call(
                &[
                    "work",
                    "act",
                    &f.work,
                    "--codex-bin",
                    provider.to_str().unwrap(),
                ],
                b"",
            ),
            "missing",
        );
        assert!(!f.root.join("provider-started").exists());
        assert!(!f.reads_path().exists());
        assert_eq!(
            fs::read_to_string(f.root.join("src/a.txt")).unwrap(),
            "external B"
        );
    }
}

#[test]
fn preparation_registry_loss_corruption_or_mismatch_refuses_intact_session() {
    for fault in ["missing", "corrupt", "changed", "symlink"] {
        let f = Fixture::new(false, false);
        f.permit();
        f.edit_ok("read", "src/a.txt", b"");
        let registry = fs::read_dir(f.root.join(".exitbind/runs"))
            .unwrap()
            .map(|entry| entry.unwrap().path())
            .find(|path| {
                path.file_name()
                    .unwrap()
                    .to_str()
                    .unwrap()
                    .contains(".managed-")
            })
            .unwrap();
        match fault {
            "missing" => fs::remove_file(&registry).unwrap(),
            "corrupt" => fs::write(&registry, "{").unwrap(),
            "changed" => {
                let mut binding: Value =
                    serde_json::from_slice(&fs::read(&registry).unwrap()).unwrap();
                binding["assignment"] = json!("changed");
                fs::write(&registry, binding.to_string()).unwrap();
            }
            _ => {
                let original = fs::read(&registry).unwrap();
                let outside = f.root.join("outside-registry");
                fs::write(&outside, original).unwrap();
                fs::remove_file(&registry).unwrap();
                std::os::unix::fs::symlink(outside, &registry).unwrap();
            }
        }
        let reason = if fault == "symlink" { "unsafe" } else { fault };
        refused(
            f.call(&["work", "file", "prepare", &f.work, &f.assignment], b""),
            reason,
        );
        refused(f.edit("edit", "src/a.txt", b"replacement"), reason);
        assert!(!f.root.join("src/a.txt").exists());
    }
}

#[test]
fn supported_native_launch_delivers_and_uses_real_managed_tool() {
    let f = Fixture::new(false, false);
    fs::write(f.root.join("AGENTS.md"), "Keep this fixture instruction.\n").unwrap();
    f.permit();
    let fake = f.root.join("fake-codex");
    let capture = f.root.join("prompt");
    let script = format!(
        r##"#!/bin/sh
/bin/cat > '{}'
tool=''
while IFS= read -r line; do
  case "$line" in 'MANAGED FILE TOOL: '*) tool=${{line#MANAGED FILE TOOL: }} ;; esac
done < '{}'
test -n "$tool" || exit 90
"$tool" read src/native.txt >/dev/null || exit 91
printf '%s' 'native replacement' | "$tool" edit src/native.txt >/dev/null || exit 92
printf '%s\n' '{{"type":"thread.started","thread_id":"managed-native-fixture"}}' '{{"type":"item.completed","item":{{"type":"agent_message","text":"{{\"outcome\":\"completed\",\"summary\":\"managed native edit\",\"reason\":\"\"}}"}}}}' '{{"type":"turn.completed","status":"completed","usage":{{"input_tokens":1,"cached_input_tokens":0,"output_tokens":1}}}}'
"##,
        capture.display(),
        capture.display()
    );
    fs::write(&fake, script).unwrap();
    fs::set_permissions(&fake, fs::Permissions::from_mode(0o700)).unwrap();
    f.ok(
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
        fs::read_to_string(f.root.join("src/native.txt")).unwrap(),
        "native replacement"
    );
    let prompt = fs::read_to_string(capture).unwrap();
    assert!(prompt.contains("Keep this fixture instruction."));
    let packet = prompt
        .lines()
        .skip_while(|line| !line.starts_with("CURRENT READABLE ASSIGNMENT, EVIDENCE AND TASKS"))
        .nth(1)
        .expect("managed prompt includes the current assignment packet");
    let packet: serde_json::Value = serde_json::from_str(packet).unwrap();
    assert_eq!(
        packet["currentAction"]["currentGrant"]["state"],
        "current_for_this_worker_assignment"
    );
    assert!(packet["currentAction"]["beforeEditing"].is_null());
    assert!(prompt.contains("MANAGED FILE TOOL:"));
    let tool = prompt
        .lines()
        .find_map(|line| line.strip_prefix("MANAGED FILE TOOL: "))
        .unwrap();
    assert_eq!(prompt.matches(tool).count(), 1);
    for syntax in ["read PATH", "edit PATH", "refresh PATH", "inspect PATH"] {
        assert!(prompt.contains(syntax), "missing {syntax}");
    }
    assert!(!prompt.contains("--expected-sha256"));
    assert!(!prompt.contains("--operation STABLE_ID"));
    f.ok(&["work", "check", &f.work], b"");
}

#[test]
fn managed_native_retains_result_when_recipient_rules_change_during_execution() {
    let f = Fixture::new(false, false);
    f.permit();
    let fake = f.root.join("fake-codex-rules");
    let script = r##"#!/bin/sh
/bin/cat >/dev/null
printf '%s\n' 'New project rule after launch.' > AGENTS.md
printf '%s\n' '{"type":"thread.started","thread_id":"rules-change"}' '{"type":"item.completed","item":{"type":"agent_message","text":"{\"outcome\":\"completed\",\"summary\":\"fixture\",\"reason\":\"\"}"}}' '{"type":"turn.completed","status":"completed","usage":{"input_tokens":1,"cached_input_tokens":0,"output_tokens":1}}'
"##;
    fs::write(&fake, script).unwrap();
    fs::set_permissions(&fake, fs::Permissions::from_mode(0o700)).unwrap();
    let output = f.call(
        &[
            "work",
            "act",
            &f.work,
            "--codex-bin",
            fake.to_str().unwrap(),
        ],
        b"",
    );
    refused(output, "recipient rules, rights or guidance changed");
    let current = f.ok(&["work", "next", &f.work, "--full"], b"");
    assert_eq!(current["next"]["assignment"], f.assignment);
    assert_eq!(current["next"]["action"], "spawn");
}

#[test]
fn managed_permit_edit_and_lost_reply_recover_one_saved_result_without_reexecution() {
    use sha2::{Digest, Sha256};
    for mode in ["matching", "unmatched", "pre-input"] {
        let f = Fixture::new(false, false);
        let relative = format!(".exitbind/runs/work-{}.jsonl", &f.work[4..]);
        let lock_id = format!("{:x}", Sha256::digest(relative.as_bytes()));
        let lock = f
            .root
            .join(format!(".exitbind/locks/run-v1-{lock_id}.lock"));
        let marker = f.root.join("provider-attempts");
        let fake = f.root.join("fake-codex-permit");
        let script = r##"#!/usr/bin/env python3
import json, subprocess, sys
prompt = sys.stdin.read().splitlines()
index = next(i for i, line in enumerate(prompt) if line.startswith('CURRENT READABLE ASSIGNMENT, EVIDENCE AND TASKS'))
packet = json.loads(prompt[index + 1])
form = packet['currentAction']['beforeEditing']
assert form['required'] is True
argv = ['Update managed fixture' if word == '<OPERATION>' else word for word in form['command']['argv']]
if __MODE__ == 'pre-input':
    open('src/preexisting.txt', 'w').write('changed before permit\n')
if __MODE__ == 'unmatched':
    argv[argv.index('--request-id') + 1] = 'unmatched-managed-request'
reply = subprocess.run(argv, capture_output=True, check=True)
assert json.loads(reply.stdout)['allowed'] is True
open('src/native.txt', 'w').write('one managed edit\n')
open(__MARKER__, 'a').write('x')
with open(__LOCK__, 'x') as lock:
    json.dump({'pid': __PID__, 'createdAt': '2026-11-01T00:00:00Z'}, lock)
print(json.dumps({'type':'thread.started','thread_id':'permit-recovery'}))
print(json.dumps({'type':'item.completed','item':{'type':'agent_message','text':json.dumps({'outcome':'completed','summary':'managed edit','reason':''})}}))
print(json.dumps({'type':'turn.completed','status':'completed','usage':{'input_tokens':1,'cached_input_tokens':0,'output_tokens':1}}))
"##.replace("__MARKER__", &serde_json::to_string(marker.to_str().unwrap()).unwrap())
        .replace("__LOCK__", &serde_json::to_string(lock.to_str().unwrap()).unwrap())
        .replace("__MODE__", &serde_json::to_string(mode).unwrap())
        .replace("__PID__", &std::process::id().to_string());
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
            lock.is_file(),
            "fixture must own the submission lock: {first:?}"
        );
        fs::remove_file(&lock).unwrap();
        if mode == "matching" {
            refused(first, "busy");
        } else {
            refused(first, "retained result has no automatic credit");
        }
        assert_eq!(fs::read_to_string(&marker).unwrap(), "x");
        assert_eq!(
            fs::read_to_string(f.root.join("src/native.txt")).unwrap(),
            "one managed edit\n"
        );
        if mode != "matching" {
            let journal_dir = f.root.join(".exitbind/native-actions").join(&f.work);
            let (path, saved) = fs::read_dir(journal_dir)
                .unwrap()
                .filter_map(Result::ok)
                .filter_map(|entry| {
                    let path = entry.path();
                    let bytes = fs::read(&path).ok()?;
                    let value: Value = serde_json::from_slice(&bytes).ok()?;
                    (value["status"] == "completed" && value["assignment"] == f.assignment)
                        .then_some((path, bytes))
                })
                .next()
                .expect("no-credit native result remains in its completed journal");
            let journal: Value = serde_json::from_slice(&saved).unwrap();
            assert!(journal["result"].is_string(), "{mode}: {journal}");
            let next = f.ok(&["work", "next", &f.work], b"");
            assert_eq!(next["next"]["assignment"], f.assignment, "{mode}");
            let denied = f.call(&["work", "act", &f.work, "--resume"], b"");
            assert!(!denied.status.success(), "{mode}: {denied:?}");
            assert_eq!(
                fs::read(path).unwrap(),
                saved,
                "{mode}: saved bytes changed"
            );
            assert_eq!(fs::read_to_string(&marker).unwrap(), "x");
            continue;
        }
        let rule = f.root.join("AGENTS.md");
        fs::write(&rule, "A changed instruction.\n").unwrap();
        let changed_rule = f.call(&["work", "act", &f.work, "--resume"], b"");
        assert!(
            !changed_rule.status.success(),
            "changed rule: {changed_rule:?}"
        );
        fs::remove_file(rule).unwrap();
        let config_path = f.root.join("exitbind.json");
        let original_config = fs::read(&config_path).unwrap();
        let mut config: Value = serde_json::from_slice(&original_config).unwrap();
        config["agents"]["worker"]["write"] = json!(["different/**"]);
        fs::write(&config_path, serde_json::to_vec(&config).unwrap()).unwrap();
        let changed_rights = f.call(&["work", "act", &f.work, "--resume"], b"");
        assert!(
            !changed_rights.status.success(),
            "changed rights: {changed_rights:?}"
        );
        fs::write(config_path, original_config).unwrap();
        let resumed = f.call(&["work", "act", &f.work, "--resume"], b"");
        assert!(resumed.status.success(), "{resumed:?}");
        // Treat the successful reply as lost. Recovery must read the same saved
        // result and must not start the provider or append the Work return again.
        let ledger = f.root.join(&relative);
        let before = fs::read(&ledger).unwrap();
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
        assert!(replay.status.success(), "{replay:?}");
        assert_eq!(fs::read(&ledger).unwrap(), before);
        assert_eq!(fs::read_to_string(marker).unwrap(), "x");
    }
}

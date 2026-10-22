//! A reviewed quarantine apply in a session whose Goal another configured
//! project owns refuses without writing anything.
use super::*;

/// Every file under `root` except Git metadata, with its bytes.
fn tree(root: &std::path::Path) -> Vec<(String, Vec<u8>)> {
    fn visit(root: &std::path::Path, dir: &std::path::Path, out: &mut Vec<(String, Vec<u8>)>) {
        for entry in fs::read_dir(dir).unwrap() {
            let path = entry.unwrap().path();
            if path.file_name().is_some_and(|name| name == ".git") {
                continue;
            }
            let relative = path
                .strip_prefix(root)
                .unwrap()
                .to_string_lossy()
                .into_owned();
            if path.is_dir() {
                out.push((format!("{relative}/"), Vec::new()));
                visit(root, &path, out);
            } else {
                out.push((relative, fs::read(&path).unwrap()));
            }
        }
    }
    let mut out = Vec::new();
    visit(root, root, &mut out);
    out.sort();
    out
}

/// R55 S2 (pattern P4): the reviewed apply argv, run in a session whose Goal
/// another configured project owns, refuses before the reservation, intent
/// or any journal is written.
#[test]
fn quarantine_apply_under_a_foreign_session_goal_writes_nothing() {
    let mut c = Case::new();
    c.accept_feature();
    let preview = c.preview(REQUEST);
    assert!(preview.status.success(), "{preview:?}");
    c.f.native_session = Some(format!(
        "foreign-{}",
        c.f.root.file_name().unwrap().to_string_lossy()
    ));
    let foreign_root = c.f.root.join(".exitbind/foreign-owner");
    fs::create_dir(&foreign_root).unwrap();
    let init = support::git_topology::command(env!("CARGO_BIN_EXE_exitbind"))
        .args(["init", "--mode", "portable", "--root"])
        .arg(&foreign_root)
        .arg("--skip-skills")
        .output()
        .unwrap();
    assert!(init.status.success(), "{init:?}");
    let owned = call_config(
        &c.f,
        &foreign_root.join("exitbind.json"),
        &[
            "goal",
            "incorporate",
            "--goal-id",
            "foreign-goal",
            "--goal",
            "Another project's Goal",
            "--none-applicable",
            "obligations,findings,blockers,decisions,externalActions",
            "--json",
        ],
        b"",
    );
    assert!(owned.status.success(), "{owned:?}");
    // The reviewed apply argv, bound to the action as this session reads it,
    // so the binding check passes and the owner check is what decides.
    let mut reviewed = value(&preview);
    let field = if reviewed["applyArgv"].is_array() {
        "applyArgv"
    } else {
        "apply"
    };
    let current =
        c.f.ok(&["work", "detail", &c.old, "--json"], b"")["actionForms"]["binding"].clone();
    let argv = reviewed[field].as_array_mut().unwrap();
    let at = argv
        .iter()
        .position(|item| item == "--current-binding")
        .unwrap();
    argv[at + 1] = current;
    let before = tree(&c.f.root);
    let applied = c.apply(&reviewed);
    assert!(!applied.status.success(), "{applied:?}");
    let message = format!(
        "{}{}",
        String::from_utf8_lossy(&applied.stdout),
        String::from_utf8_lossy(&applied.stderr)
    );
    assert!(
        message.contains("native session Goal is owned by another configured project"),
        "{message}"
    );
    assert_eq!(tree(&c.f.root), before, "the refused apply wrote nothing");
}

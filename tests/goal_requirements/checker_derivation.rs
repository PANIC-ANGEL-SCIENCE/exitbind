//! R54 Step 2: `goal prepare` and the named `work begin` route accept the
//! declared check command as declared. Without `--artifact` the checker is the
//! tested project file the command names, directly or through one outside
//! script it runs; no wrapper that repeats the checker path is needed.
use super::*;

/// A directory outside the fixture project, removed with the test.
pub(super) struct Outside(support::TempRoot);

impl Outside {
    pub(super) fn new(_project: &Project) -> Self {
        Self(support::temp("goal-checker-outside"))
    }
    pub(super) fn script(&self, name: &str, body: &str) -> String {
        let path = self.0.join(name);
        fs::write(&path, body).unwrap();
        path.to_str().unwrap().to_owned()
    }
}

fn prepare_args<'a>(command: &'a str, artifact: Option<&'a str>) -> Vec<&'a str> {
    let mut args = vec![
        "goal",
        "prepare",
        "--goal-id",
        "import",
        "--requirement",
        "R1,R2,R3",
        "--check-command",
        command,
    ];
    if let Some(artifact) = artifact {
        args.extend(["--artifact", artifact]);
    }
    args
}

fn sha256(path: &Path) -> String {
    use sha2::{Digest, Sha256};
    format!("{:x}", Sha256::digest(fs::read(path).unwrap()))
}

#[test]
fn prepare_derives_the_checker_through_an_outside_script_without_a_wrapper() {
    let p = Project::new("goal-checker-derivation");
    p.setup();
    let outside = Outside::new(&p);
    let root = p.root.to_str().unwrap();
    let script = outside.script(
        "run-all.sh",
        &format!("set -eu\ncd '{root}'\nexec sh ./check.sh all\n"),
    );
    let command = format!("sh {script}");
    assert!(
        !command.contains("check.sh"),
        "the command must not name the checker itself"
    );
    let runs_before = p.run_entries();

    let derived = p.value(&prepare_args(&command, None), None);
    assert_eq!(derived["checker"]["path"], "check.sh");
    assert_eq!(derived["checker"]["derived"], true);
    assert_eq!(derived["checker"]["via"], script.as_str());
    assert_eq!(
        derived["checker"]["sha256"],
        sha256(&p.root.join("check.sh"))
    );
    let route = derived["beginRoute"]["command"]
        .as_array()
        .unwrap()
        .iter()
        .map(|item| item.as_str().unwrap().to_owned())
        .collect::<Vec<_>>();
    let artifact = route.iter().position(|item| item == "--artifact").unwrap();
    assert_eq!(
        route[artifact + 1],
        "check.sh",
        "the begin route carries the derived checker"
    );
    assert_eq!(p.run_entries(), runs_before, "preparation starts no Work");

    let explicit = p.value(&prepare_args(&command, Some("check.sh")), None);
    assert_eq!(explicit["checker"]["path"], "check.sh");
    assert_eq!(explicit["checker"]["derived"], false);
    assert_eq!(explicit["checker"]["sha256"], derived["checker"]["sha256"]);
    p.reject(
        &prepare_args(&command, Some("render.sh")),
        "check command must name its declared project checker input",
    );

    // The emitted begin route freezes the derived checker, and the observed
    // check through the outside script supports every selected requirement.
    let begin = route
        .iter()
        .map(|item| {
            if item == "<REVIEW_DECISION>" {
                "required".to_owned()
            } else {
                item.clone()
            }
        })
        .collect::<Vec<_>>();
    let started = p.call_exact_argv(&begin);
    assert!(started.status.success(), "{started:?}");
    let work = serde_json::from_slice::<Value>(&started.stdout).unwrap()["work"]
        .as_str()
        .unwrap()
        .to_owned();
    p.drive(&work, true);
    let status = p.status();
    for id in ["R1", "R2", "R3"] {
        assert_eq!(row(&status, id)["mapped"], true, "{status}");
        assert_eq!(row(&status, id)["state"], "current", "{status}");
    }
}

// F4: a comment's apostrophe must not swallow the outside script's text.
#[test]
fn prepare_derives_through_an_outside_script_with_an_apostrophe_comment() {
    let p = Project::new("goal-checker-derivation-apostrophe");
    p.setup();
    let outside = Outside::new(&p);
    let root = p.root.to_str().unwrap();
    let script = outside.script(
        "run-with-comment.sh",
        &format!("set -eu\ncd '{root}'\n# Don't run this twice.\nexec sh ./check.sh all\n"),
    );

    let derived = p.value(&prepare_args(&format!("sh {script}"), None), None);
    assert_eq!(derived["checker"]["path"], "check.sh");
    assert_eq!(derived["checker"]["derived"], true);
    assert_eq!(derived["checker"]["via"], script.as_str());
}

// F4: an environment assignment before the command word is not a path
// candidate, and its value must not hide the checker the command names.
#[test]
fn prepare_derives_past_a_leading_environment_assignment() {
    let p = Project::new("goal-checker-derivation-assignment");
    p.setup();

    let derived = p.value(&prepare_args("CHECK=check.sh make check", None), None);
    assert_eq!(derived["checker"]["path"], "check.sh");
    assert_eq!(derived["checker"]["derived"], true);
    assert_eq!(derived["checker"]["via"], Value::Null);
}

#[test]
fn named_begin_derives_the_checker_when_artifact_is_omitted() {
    let p = Project::new("goal-checker-begin-derivation");
    p.setup();
    let started = p.value(
        &[
            "work",
            "begin",
            "change",
            "--goal",
            "Check the declared import requirements",
            "--goal-id",
            "import",
            "--requirement",
            "R1",
            "--check-command",
            "sh check.sh R1",
            "--review-policy",
            "required",
        ],
        None,
    );
    let work = started["work"].as_str().unwrap();
    assert!(row(&p.status(), "R1")["support"]
        .as_array()
        .unwrap()
        .iter()
        .any(|support| support["mapping"]["work"] == work
            && support["evidence"]["checkerInput"]["path"] == "check.sh"));
}

#[test]
fn prepare_explains_an_ambiguous_or_empty_derivation() {
    let p = Project::new("goal-checker-derivation-refusals");
    p.setup();
    let outside = Outside::new(&p);
    let root = p.root.to_str().unwrap();
    let ambiguous = outside.script(
        "both.sh",
        &format!("cd '{root}'\nsh ./check.sh all && sh render.sh\n"),
    );
    let output = p.call(&prepare_args(&format!("sh {ambiguous}"), None), None);
    assert!(!output.status.success(), "{output:?}");
    let error = format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(
        error.contains("names several tested project files"),
        "{error}"
    );
    assert!(
        error.contains("check.sh") && error.contains("render.sh"),
        "{error}"
    );
    assert!(error.contains("--artifact"), "{error}");

    let empty = outside.script("nothing.sh", "true\n");
    let output = p.call(&prepare_args(&format!("sh {empty}"), None), None);
    assert!(!output.status.success(), "{output:?}");
    let error = format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(error.contains("names no tested project file"), "{error}");
    assert!(error.contains("--artifact"), "{error}");
}

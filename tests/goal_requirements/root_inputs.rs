//! R55 F3 (pattern P3; FR55-03, FR54-10): every path input meets its root in
//! one place. The registry in `src/project/path/inputs.rs` declares one
//! outcome per path input; root comparisons live in that helper and file
//! creation under a root in the one private writer; an outside task card is
//! snapshotted through the plan-card route and hidden from Git; a confined
//! input's outside path is refused naming the input and its route.
use super::*;
use std::collections::BTreeMap;

const REGISTRY: &str = "src/project/path/inputs.rs";
const WRITER: &str = "src/project/private_write.rs";
const CARD: &str = "# R55 task\n\n| Step | Do | Done when |\n| --- | --- | --- |\n| 0 | Baseline | The baseline is recorded. |\n| 1 | Parser | The parser rejects empty input. |\n";

/// Root comparisons (`strip_prefix`/`starts_with` whose argument names a
/// root, top, state, product, project, control, base or private path) that
/// are not a path input meeting its root: each compares a path the product
/// made itself, or relates roots to each other. Count per file.
const INTERNAL_ROOT_CHECKS: &[(&str, usize, &str)] = &[
    (
        "src/project/path.rs",
        2,
        "project::path, the read family the helper belongs to",
    ),
    (REGISTRY, 1, "the helper's one prefix decision"),
    (
        "src/cli/activity.rs",
        2,
        "activity records the product writes under the state root",
    ),
    (
        "src/config/profile.rs",
        1,
        "profile directories setup creates",
    ),
    (
        "src/host/codegraph/provider.rs",
        2,
        "CodeGraph's generated state directory",
    ),
    (
        "src/host/runtime/context.rs",
        1,
        "a configured profile shown relative to the project in hook text",
    ),
    (
        "src/host/runtime/project.rs",
        3,
        "a host hook payload's cwd routed to a configured project",
    ),
    (
        "src/host/settings.rs",
        1,
        "host settings files setup writes",
    ),
    (
        "src/memory/choices.rs",
        3,
        "task memory choice files under the state root",
    ),
    (
        "src/project/continuation/admission.rs",
        1,
        "a boundary pattern matched as text",
    ),
    (
        "src/project/git_preflight.rs",
        4,
        "a write target relative to its Git worktree",
    ),
    (
        "src/project/layout.rs",
        1,
        "legacy layout migration directories",
    ),
    (
        "src/project/layout_types.rs",
        6,
        "control, product and state roots must not nest",
    ),
    (
        "src/project/managed_files.rs",
        2,
        "directories the product creates",
    ),
    (
        "src/project/native_profiles.rs",
        1,
        "native profile projections setup writes",
    ),
    (
        "src/project/onboarding.rs",
        2,
        "init targets grouped by control and state root",
    ),
    (
        "src/project/portability.rs",
        4,
        "source and binding roots must not nest",
    ),
    (
        "src/project/setup_projection.rs",
        1,
        "projection directories setup writes",
    ),
    ("src/project/skills.rs", 2, "managed skill directories"),
    (
        "src/run/capture_error.rs",
        1,
        "capture records under the state root",
    ),
    (
        "src/run/carry.rs",
        1,
        "a ledger the ledger judge confined, made relative",
    ),
    (
        "src/run/gate/value.rs",
        1,
        "a check script under the managed state is not a checker source",
    ),
    (
        "src/run/governor.rs",
        1,
        "a ledger the ledger judge confined, made relative",
    ),
    (
        "src/run/governor_recovery.rs",
        1,
        "a ledger the ledger judge confined, made relative",
    ),
    (
        "src/run/guidance_continuity/advisory_paths.rs",
        1,
        "Git-listed product paths made relative",
    ),
    (
        "src/run/inputs.rs",
        1,
        "tested inputs exclude the state root",
    ),
    (
        "src/run/pre_return.rs",
        3,
        "a ledger the ledger judge confined, made relative",
    ),
    (
        "src/session_goal/journey/integration/lineage.rs",
        1,
        "a ledger the ledger judge confined, made relative",
    ),
    (
        "src/session_goal/usage.rs",
        1,
        "usage files the product wrote under the state root",
    ),
    (
        "src/value_benchmark/artifacts.rs",
        2,
        "the benchmark's own fixture tree",
    ),
    (
        "src/work/managed_edit/state.rs",
        1,
        "edit state the product wrote under the state root",
    ),
    (
        "src/work/native_action.rs",
        1,
        "native action files under the state root",
    ),
    (
        "src/work/native_action/continuation.rs",
        1,
        "native action files under the state root",
    ),
];

/// Writes outside the private writer in production code: each writes under
/// the state root, the control root, a host store or a destination the
/// operator named, never a private product file under the project root.
/// Count per file of `WRITE_CALLS`.
const OTHER_WRITES: &[(&str, usize, &str)] = &[
    (WRITER, 1, "the one private writer"),
    (
        "src/project/private_write/git.rs",
        2,
        "the writer's local exclude, replaced atomically",
    ),
    (
        "src/project/path.rs",
        1,
        "the secure creation primitive the writer uses",
    ),
    (
        "src/memory/lessons/carry.rs",
        1,
        "a guard: a product file the lesson names, copied where the previous project kept it",
    ),
    ("src/cli/mod.rs", 1, "receipt --output under the state root"),
    (
        "src/config/profile.rs",
        3,
        "agent profiles under the control root",
    ),
    (
        "src/distribution/update.rs",
        7,
        "the self-update replaces the executable",
    ),
    (
        "src/evidence/receipt.rs",
        1,
        "brief --receipt under the state root",
    ),
    (
        "src/host/away.rs",
        2,
        "away runner state under the state root",
    ),
    (
        "src/host/codegraph.rs",
        3,
        "CodeGraph state under the state root",
    ),
    (
        "src/host/native_session.rs",
        1,
        "the host's session environment file it names",
    ),
    (
        "src/host/session_recovery/hint.rs",
        2,
        "the native session store",
    ),
    (
        "src/host/settings.rs",
        2,
        "host settings files setup writes",
    ),
    (
        "src/memory/choices.rs",
        2,
        "task memory choices and their lock under the state root",
    ),
    (
        "src/memory/forgetting.rs",
        1,
        "forgetting receipts under the state root",
    ),
    (
        "src/memory/ledger.rs",
        1,
        "memory ledgers under the state root",
    ),
    (
        "src/memory/lessons.rs",
        1,
        "the lesson mutation lock under the state root",
    ),
    (
        "src/project/layout.rs",
        1,
        "legacy layout migration under the state root",
    ),
    (
        "src/project/layout_types.rs",
        3,
        "project bindings in the bindings directory",
    ),
    (
        "src/project/managed_files.rs",
        2,
        "state and managed files init creates",
    ),
    (
        "src/project/skills.rs",
        2,
        "managed skills under the control root",
    ),
    (
        "src/run/capture.rs",
        1,
        "captured logs under the state root",
    ),
    (
        "src/run/cleanup_fixture.rs",
        3,
        "test-only fixture, compiled only through run/cleanup_tests.rs",
    ),
    (
        "src/run/ledger.rs",
        3,
        "run ledgers and locks under the state root",
    ),
    (
        "src/run/quarantine/admission.rs",
        1,
        "quarantine records under the state root",
    ),
    (
        "src/session_goal/association/companion.rs",
        1,
        "Goal association under the state root",
    ),
    (
        "src/session_goal/continuity/capture.rs",
        2,
        "continuity capture under the state root",
    ),
    (
        "src/session_goal/display.rs",
        2,
        "the native session display store",
    ),
    (
        "src/session_goal/native.rs",
        1,
        "the native session Goal owner record",
    ),
    (
        "src/value_benchmark/artifacts.rs",
        1,
        "the benchmark export the operator named",
    ),
    (
        "src/work/closeout.rs",
        1,
        "Work receipts under the state root",
    ),
    ("src/work/focus.rs", 2, "focus records under the state root"),
    ("src/work/held.rs", 1, "held results under the state root"),
    (
        "src/work/managed_edit/state.rs",
        1,
        "managed edit state under the state root",
    ),
    (
        "src/work/native_action.rs",
        1,
        "the native action journal lock under the state root",
    ),
    (
        "src/work/native_action/journal.rs",
        2,
        "native action journal under the state root",
    ),
    (
        "src/work/native_action/recovery.rs",
        1,
        "native action recovery under the state root",
    ),
];

/// Every way production code creates, replaces or copies a file.
const WRITE_CALLS: &[&str] = &[
    "secure_create_new(",
    "create_new(true)",
    ".create(true)",
    "File::create(",
    "fs::write(",
    "fs::copy(",
    "fs::rename(",
];
const ROOT_WORDS: &[&str] = &[
    "root", "top", "state", "product", "project", "control", "base", "private",
];

fn manifest() -> &'static Path {
    Path::new(env!("CARGO_MANIFEST_DIR"))
}

pub(super) fn sources() -> Vec<(String, String)> {
    fn visit(dir: &Path, out: &mut Vec<(String, String)>) {
        for entry in fs::read_dir(dir).unwrap() {
            let path = entry.unwrap().path();
            // Test-only module files are compiled only under `cfg(test)`.
            let name = path.file_name().unwrap().to_string_lossy();
            let test_file = name == "tests.rs" || name.ends_with("_tests.rs");
            if path.is_dir() {
                visit(&path, out);
            } else if path.extension().is_some_and(|ext| ext == "rs") && !test_file {
                let relative = path
                    .strip_prefix(manifest())
                    .unwrap()
                    .to_string_lossy()
                    .replace('\\', "/");
                out.push((relative, production(&fs::read_to_string(&path).unwrap())));
            }
        }
    }
    let mut out = Vec::new();
    visit(&manifest().join("src"), &mut out);
    out.sort();
    out
}

/// `text` without its `#[cfg(test)]`/`#[cfg(all(test, ..))]` modules.
fn production(text: &str) -> String {
    let mut out = String::new();
    let mut rest = text;
    loop {
        let found = rest.match_indices("#[cfg(").find(|(index, _)| {
            let line = &rest[*index..];
            let attribute = &line[..line.find('\n').unwrap_or(line.len())];
            let next = line[attribute.len()..].trim_start_matches('\n');
            (attribute == "#[cfg(test)]" || attribute.starts_with("#[cfg(all(test"))
                && (next.starts_with("mod ") || next.starts_with("pub(crate) mod "))
                && next[..next.find('\n').unwrap_or(next.len())].ends_with('{')
        });
        let Some((index, _)) = found else {
            out.push_str(rest);
            return out;
        };
        out.push_str(&rest[..index]);
        let open = index + rest[index..].find('{').unwrap() + 1;
        let mut depth = 1;
        let mut end = rest.len();
        for (offset, c) in rest[open..].char_indices() {
            depth += match c {
                '{' => 1,
                '}' => -1,
                _ => 0,
            };
            if depth == 0 {
                end = open + offset + 1;
                break;
            }
        }
        rest = &rest[end..];
    }
}

/// Occurrences per file of a call whose argument names a root.
fn root_checks(text: &str) -> usize {
    let mut count = 0;
    for call in ["strip_prefix(", "starts_with("] {
        for (index, _) in text.match_indices(call) {
            let argument: String = text[index + call.len()..]
                .chars()
                .take_while(|c| c.is_ascii_alphanumeric() || "_.&()".contains(*c))
                .collect();
            let name = argument.trim_start_matches(['&', '(']);
            if name
                .chars()
                .next()
                .is_some_and(|c| c.is_ascii_lowercase() || c == '_')
                && ROOT_WORDS.iter().any(|word| name.contains(word))
            {
                count += 1;
            }
        }
    }
    count
}

fn writes(text: &str) -> usize {
    WRITE_CALLS
        .iter()
        .map(|call| text.matches(call).count())
        .sum()
}

fn counted(table: &[(&str, usize, &str)]) -> BTreeMap<String, usize> {
    table
        .iter()
        .map(|(file, count, _)| ((*file).to_owned(), *count))
        .collect()
}

/// `"name"` items of the `const NAME: &[&str] = &[ ... ];` array in `text`.
fn string_array(text: &str, name: &str) -> Vec<String> {
    let start = text.find(&format!("const {name}: &[&str] = &[")).unwrap();
    let body = &text[start..];
    let body = &body[body.find("= &[").unwrap() + 4..body.find("];").unwrap()];
    body.split('"')
        .skip(1)
        .step_by(2)
        .map(str::to_owned)
        .collect()
}

/// `(label, options, outcome, judge)` of every registered input.
fn registry() -> Vec<(String, String, String, String)> {
    let text = fs::read_to_string(manifest().join(REGISTRY)).unwrap();
    let field = |entry: &str, name: &str| -> String {
        let start = entry.find(&format!("{name}: ")).unwrap() + name.len() + 2;
        let rest = &entry[start..];
        let end = rest.find(",\n").unwrap();
        rest[..end].trim_matches('"').to_owned()
    };
    let body = &text[text.find("pub(crate) const INPUTS").unwrap()..];
    let body = &body[..body.find("\n];").unwrap()];
    body.split("    Input {")
        .skip(1)
        .map(|entry| {
            (
                field(entry, "label"),
                field(entry, "options"),
                field(entry, "outcome"),
                field(entry, "judge"),
            )
        })
        .collect()
}

#[test]
fn path_inputs_inventory_declares_one_outcome_for_every_path_option() {
    let args = fs::read_to_string(manifest().join("src/cli/args.rs")).unwrap();
    let options = string_array(&args, "VALUE_OPTIONS");
    assert!(options.len() > 100, "{options:?}");
    let registry_text = fs::read_to_string(manifest().join(REGISTRY)).unwrap();
    let not_paths = string_array(&registry_text, "NOT_PATHS");
    let inputs = registry();
    assert!(inputs.len() >= 15, "{inputs:?}");
    let registered = |option: &str| {
        inputs.iter().any(|(_, options, _, _)| {
            options
                .split(", ")
                .any(|item| item.split(' ').next() == Some(&format!("--{option}")))
        })
    };
    for option in &options {
        let path = registered(option);
        let plain = not_paths.contains(option);
        assert!(
            path ^ plain,
            "--{option} must be either a registered path input with an outcome in {REGISTRY} or listed in NOT_PATHS (registered: {path}, not a path: {plain})"
        );
    }
    for listed in &not_paths {
        assert!(
            options.contains(listed),
            "NOT_PATHS lists unknown --{listed}"
        );
    }
    let sources: BTreeMap<_, _> = sources().into_iter().collect();
    for (label, _, outcome, judge) in &inputs {
        if !(outcome.starts_with("Outcome::Confined") || outcome == "Outcome::Snapshotted") {
            continue;
        }
        for file in judge.split(", ") {
            let text = sources
                .get(file)
                .unwrap_or_else(|| panic!("{label}: judge {file} does not exist"));
            assert!(
                text.contains("inputs::"),
                "{label}: judge {file} must place the input through the root helper"
            );
        }
    }
}

#[test]
fn root_checks_and_private_writes_live_in_the_helper_and_the_writer() {
    let roots = counted(INTERNAL_ROOT_CHECKS);
    let creates = counted(OTHER_WRITES);
    let mut found_roots = BTreeMap::new();
    let mut found_creates = BTreeMap::new();
    for (file, text) in sources() {
        let checks = root_checks(&text);
        if checks > 0 {
            found_roots.insert(file.clone(), checks);
        }
        let created = writes(&text);
        if created > 0 {
            found_creates.insert(file, created);
        }
    }
    assert_eq!(
        found_roots, roots,
        "a root comparison outside the root helper must place its input through src/project/path/inputs.rs, or be registered here as internal with its reason"
    );
    assert_eq!(
        found_creates, creates,
        "a file created under a root must go through src/project/private_write.rs, or be registered here with the root it writes under"
    );
    let writer = fs::read_to_string(manifest().join(WRITER)).unwrap();
    assert!(writer.contains("git.exclude(directory)") && writer.contains("unlisted(directory)"));
    for (file, owner) in [
        ("src/host/plan_card.rs", "plan card"),
        ("src/memory/lessons/carry.rs", "carried lesson source"),
        ("src/memory/stamped_config.rs", "stamped configuration copy"),
        ("src/project/setup_card.rs", "task card snapshot"),
    ] {
        let text = fs::read_to_string(manifest().join(file)).unwrap();
        assert!(
            text.contains("private_write::write_") && text.contains(owner),
            "{file} writes its {owner} through the private writer"
        );
    }
}

/// A local-mode project in its own Git worktree: the control and state roots
/// live outside it, so nothing but the product hides `.exitbind/` from Git.
struct Local {
    base: PathBuf,
    product: PathBuf,
    control: PathBuf,
    state: PathBuf,
}

impl Local {
    fn new(label: &str) -> Self {
        let base = support::temp(label);
        let local = Self {
            product: base.join("product"),
            control: base.join("control"),
            state: base.join("state"),
            base,
        };
        for dir in [&local.product, &local.control, &local.state] {
            fs::create_dir_all(dir).unwrap();
        }
        support::git_topology::repository(&local.product);
        support::git_topology::git_only_path(&local.product);
        fs::write(local.product.join("README.md"), "product\n").unwrap();
        local
    }
    fn outside_card(&self) -> PathBuf {
        let card = self.base.join("dispatch/R55/00_TASK.md");
        fs::create_dir_all(card.parent().unwrap()).unwrap();
        fs::write(&card, CARD).unwrap();
        card
    }
    fn exitbind(&self, args: &[&str]) -> Output {
        let mut command = support::git_topology::command(env!("CARGO_BIN_EXE_exitbind"));
        support::git_topology::isolate(&mut command);
        command
            .current_dir(&self.product)
            .args(args)
            .env("EXITBIND_NO_UPDATE_CHECK", "1")
            .env("EXITBIND_BINDINGS_DIR", self.base.join("bindings"))
            .env("EXITBIND_SESSION_GOALS_DIR", self.base.join("goals"))
            .env("PATH", support::git_topology::git_only_path(&self.product))
            .stdin(Stdio::null());
        support::run(&mut command)
    }
    fn setup(&self, card: &Path) -> Output {
        self.setup_with(Some(card))
    }
    fn setup_with(&self, card: Option<&Path>) -> Output {
        let product = self.product.to_str().unwrap();
        let control = self.control.to_str().unwrap();
        let state = self.state.to_str().unwrap();
        let mut args = vec![
            "setup",
            "--json",
            "--apply",
            "--mode",
            "local",
            "--root",
            product,
            "--project-id",
            "r55",
            "--control-root",
            control,
            "--state-root",
            state,
            "--scope",
            "worker",
            "--observe",
            "README.md",
            "--write",
            "README.md",
            "--commands",
            "true",
            "--check-command",
            "true",
            "--goal",
            "Ship the R55 rows",
            "--review-policy",
            "required",
            "--skip-skills",
        ];
        if let Some(card) = card {
            args.extend(["--task-card", card.to_str().unwrap()]);
        }
        self.exitbind(&args)
    }
    fn config(&self) -> String {
        self.control
            .join("exitbind.json")
            .to_str()
            .unwrap()
            .to_owned()
    }
    fn listed(&self) -> Vec<String> {
        let output = support::git_topology::git(&self.product)
            .args(["status", "--porcelain", "--untracked-files=all"])
            .output()
            .unwrap();
        assert!(output.status.success(), "{output:?}");
        String::from_utf8(output.stdout)
            .unwrap()
            .lines()
            .map(str::to_owned)
            .collect()
    }
}

impl Drop for Local {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.base);
    }
}

fn json_of(output: &Output) -> Value {
    assert!(
        output.status.success(),
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    serde_json::from_slice(&output.stdout).unwrap()
}

#[test]
fn an_outside_task_card_is_snapshotted_hidden_from_git_and_reused() {
    let local = Local::new("root-inputs-card");
    let card = local.outside_card();
    let report = json_of(&local.setup(&card));
    let task = &report["next"]["taskCard"];
    let snapshot = ".exitbind/plan-cards/R55/TASK.md";
    assert_eq!(task["path"], snapshot, "{report}");
    assert_eq!(task["goalId"], "R55", "{report}");
    assert_eq!(task["snapshotted"], true);
    assert_eq!(
        task["source"],
        card.canonicalize().unwrap().to_str().unwrap()
    );
    use sha2::{Digest, Sha256};
    assert_eq!(
        task["sourceSha256"],
        format!("{:x}", Sha256::digest(CARD.as_bytes()))
    );
    assert_eq!(task["registration"]["registered"], true, "{report}");
    assert_eq!(
        fs::read(local.product.join(snapshot)).unwrap(),
        CARD.as_bytes()
    );
    let listed = local.listed();
    assert!(
        listed.iter().all(|line| !line.contains(".exitbind")),
        "git status lists a product file under .exitbind/: {listed:?}"
    );
    let config = local.config();
    let status =
        json_of(&local.exitbind(&["goal", "status", "--json", "--project", "--config", &config]));
    let text = status.to_string();
    let sources: Vec<&Value> = status["requirements"]["requirements"]
        .as_array()
        .unwrap_or_else(|| panic!("{status}"))
        .iter()
        .map(|row| &row["source"]["ref"])
        .collect();
    assert_eq!(sources.len(), 2, "{text}");
    assert!(sources.iter().all(|source| *source == snapshot), "{text}");

    let again = json_of(&local.setup(&card));
    assert_eq!(again["next"]["taskCard"]["path"], snapshot, "{again}");
    let cards = fs::read_dir(local.product.join(".exitbind/plan-cards"))
        .unwrap()
        .count();
    assert_eq!(cards, 1, "a byte-identical re-run reuses the snapshot");

    fs::write(&card, format!("{CARD}\nRevised.\n")).unwrap();
    let revised = json_of(&local.setup(&card));
    assert_eq!(
        revised["next"]["taskCard"]["path"], ".exitbind/plan-cards/R55-2/TASK.md",
        "a changed source is suffixed and the Goal keeps its ID: {revised}"
    );
    assert_eq!(revised["next"]["taskCard"]["goalId"], "R55");
    assert_eq!(
        fs::read(local.product.join(snapshot)).unwrap(),
        CARD.as_bytes()
    );
    assert!(local
        .listed()
        .iter()
        .all(|line| !line.contains(".exitbind")));
}

#[test]
fn a_root_whose_git_marker_cannot_be_read_refuses_the_snapshot_before_writing() {
    let local = Local::new("root-inputs-marker");
    let card = local.outside_card();
    json_of(&local.setup_with(None));
    fs::remove_dir_all(local.product.join(".git")).unwrap();
    fs::write(
        local.product.join(".git"),
        "gitdir: /nonexistent/exitbind-marker\n",
    )
    .unwrap();
    let before = super::setup_first_work::tree(&local.product);
    let control_before = super::setup_first_work::tree(&local.control);
    let output = local.setup(&card);
    assert!(!output.status.success(), "{output:?}");
    let stderr = format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(
        stderr.contains("task card snapshot was not written")
            && stderr.contains("cannot be read by git"),
        "{stderr}"
    );
    assert_eq!(super::setup_first_work::tree(&local.product), before);
    assert_eq!(
        super::setup_first_work::tree(&local.control),
        control_before
    );
}

#[test]
fn confined_inputs_refuse_an_outside_path_naming_the_input_and_its_route() {
    let p = Project::new("root-inputs-confined");
    p.setup();
    let outside = super::checker_derivation::Outside::new(&p);
    let file = outside.script("request.txt", &format!("{}\n", REQUIREMENTS[0].1));
    for (args, input, route) in [
        (
            vec![
                "goal",
                "require",
                "--goal-id",
                "import",
                "--requirement",
                "R9",
                "--obligation",
                REQUIREMENTS[0].1,
                "--artifact",
                &file,
            ],
            "requirement source (--artifact for goal require)",
            "as its --task-card value",
        ),
        (
            vec![
                "goal",
                "prepare",
                "--goal-id",
                "import",
                "--requirement",
                "R1",
                "--check-command",
                "sh check.sh R1",
                "--artifact",
                &file,
            ],
            "project checker (--artifact for goal prepare, work begin)",
            "pinned by sha256",
        ),
        (
            vec!["run", "inspect", &file],
            "ledger (LEDGER, --ledger",
            "state-root-relative ledger",
        ),
    ] {
        let output = p.call(&args, None);
        assert!(
            !output.status.success(),
            "{args:?} accepted an outside path"
        );
        let text = format!(
            "{}{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
        assert!(
            text.contains(input) && text.contains(route) && text.contains(&file),
            "{args:?}: {text}"
        );
    }
    // An absolute ledger inside the state root is placed, not refused.
    let inside = p.root.join(".exitbind/runs/absent.jsonl");
    let output = p.call(&["run", "inspect", inside.to_str().unwrap()], None);
    let text = String::from_utf8_lossy(&output.stdout).into_owned()
        + &String::from_utf8_lossy(&output.stderr);
    assert!(!text.contains("escapes"), "{text}");
    assert!(text.contains("ledger does not exist"), "{text}");
}

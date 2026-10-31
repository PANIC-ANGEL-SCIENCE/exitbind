//! Runtime coverage for path inputs, snapshots and root confinement.
use super::*;

const CARD: &str = "# R55 task\n\n| Step | Do | Done when |\n| --- | --- | --- |\n| 0 | Baseline | The baseline is recorded. |\n| 1 | Parser | The parser rejects empty input. |\n";

/// A local-mode project in its own Git worktree: the control and state roots
/// live outside it, so nothing but the product hides `.exitbind/` from Git.
struct Local {
    base: support::TempRoot,
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

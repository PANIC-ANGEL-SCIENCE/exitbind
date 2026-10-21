//! Claude Code plan approval to task card. `PostToolUse` for `ExitPlanMode`
//! fires only after the human approves a plan, and its `tool_response.plan`
//! carries the approved text. When that text has a `Done when` step table the
//! plan is saved as a private card inside the project, hidden from Git through
//! the local exclude file, and the Lead gets one line naming the setup command
//! that registers it. Nothing is applied; without a table nothing is written
//! and nothing is printed.

use crate::config::Loaded;
use serde_json::{Map, Value};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{Duration, Instant};

/// Project-relative directory of saved plan cards.
const CARDS: &str = ".exitbind/plan-cards";
const PLAN_MAX_BYTES: usize = 64 * 1024;

pub(crate) fn is_plan_approval(payload: &Map<String, Value>) -> bool {
    payload.get("tool_name").and_then(Value::as_str) == Some("ExitPlanMode")
}

/// Save the approved plan as a card and return the Lead's one line, or `None`
/// when the plan has no step table or the card could not be kept private.
pub(crate) fn on_approval(loaded: &Loaded, payload: &Map<String, Value>) -> Option<String> {
    let response = payload.get("tool_response")?;
    let plan = response.get("plan").and_then(Value::as_str)?;
    if plan.len() > PLAN_MAX_BYTES || crate::project::setup::step_rows(plan).is_err() {
        return None;
    }
    let slug = slug(response.get("filePath").and_then(Value::as_str));
    let relative = format!("{CARDS}/{slug}/TASK.md");
    let root = &loaded.product_root;
    let deadline = Instant::now() + Duration::from_secs(2);
    let git = Git::open(root, deadline);
    match &git {
        Some(git) => git.exclude(CARDS)?,
        // A Git marker that Git cannot read: never risk a listed card.
        None if root.ancestors().any(|dir| dir.join(".git").exists()) => return None,
        None => {}
    }
    let path = root.join(&relative);
    write_private(&path, plan).ok()?;
    if git.is_some_and(|git| !git.unlisted(CARDS)) {
        let _ = std::fs::remove_file(&path);
        return None;
    }
    let root = shell_word(root.to_str()?);
    Some(format!(
        "The approved plan has a Done when table and is saved as a private task card. To make it this Goal's start path, run `exitbind setup --apply --root {root} --task-card {relative}` with this project's usual setup options (--goal, --check-command, --review-policy)."
    ))
}

/// A Goal ID from the host's plan file name (stable within a session), else
/// `plan`. Setup derives the Goal ID from the card's directory name.
fn slug(file: Option<&str>) -> String {
    let stem = file
        .map(Path::new)
        .and_then(Path::file_stem)
        .and_then(|stem| stem.to_str())
        .unwrap_or_default();
    let clean = stem
        .chars()
        .filter(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_'))
        .take(64)
        .collect::<String>();
    if clean.starts_with(|c: char| c.is_ascii_alphanumeric()) {
        clean
    } else {
        "plan".into()
    }
}

fn write_private(path: &Path, text: &str) -> Result<(), String> {
    use std::io::Write;
    use std::os::unix::fs::{DirBuilderExt, OpenOptionsExt};
    let parent = path.parent().ok_or("card path has no parent")?;
    std::fs::DirBuilder::new()
        .recursive(true)
        .mode(0o700)
        .create(parent)
        .map_err(|error| error.to_string())?;
    let mut file = std::fs::OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(true)
        .mode(0o600)
        .open(path)
        .map_err(|error| error.to_string())?;
    file.write_all(text.as_bytes())
        .map_err(|error| error.to_string())
}

fn shell_word(text: &str) -> String {
    if text
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || matches!(c, '/' | '.' | '_' | '-'))
    {
        text.to_owned()
    } else {
        format!("'{}'", text.replace('\'', "'\\''"))
    }
}

/// The project's Git worktree, when there is one.
struct Git {
    root: PathBuf,
    /// Worktree-relative prefix of the project root, `""` or ending in `/`.
    prefix: String,
    deadline: Instant,
}

impl Git {
    fn open(root: &Path, deadline: Instant) -> Option<Self> {
        let mut git = Self {
            root: root.to_path_buf(),
            prefix: String::new(),
            deadline,
        };
        git.prefix = git.text(&["rev-parse", "--show-prefix"])?;
        Some(git)
    }

    fn text(&self, args: &[&str]) -> Option<String> {
        let mut command = Command::new("git");
        crate::host::codegraph::clear_git_environment(&mut command);
        command.arg("-C").arg(&self.root).args(args);
        let output =
            crate::kernel::bounded_process::run(&mut command, 64 * 1024, 4096, self.deadline)
                .ok()?;
        output.status.success().then_some(())?;
        String::from_utf8(output.stdout)
            .ok()
            .map(|text| text.trim_end_matches('\n').to_owned())
    }

    /// Add `/<prefix><directory>/` to the local exclude file once.
    fn exclude(&self, directory: &str) -> Option<()> {
        let file = self.text(&["rev-parse", "--git-path", "info/exclude"])?;
        let file = self.root.join(file);
        let pattern = format!("/{}{directory}/", self.prefix);
        let existing = std::fs::read_to_string(&file).unwrap_or_default();
        if existing.lines().any(|line| line.trim() == pattern) {
            return Some(());
        }
        std::fs::create_dir_all(file.parent()?).ok()?;
        let separator = if existing.is_empty() || existing.ends_with('\n') {
            ""
        } else {
            "\n"
        };
        std::fs::write(&file, format!("{existing}{separator}{pattern}\n")).ok()
    }

    /// Whether `git status` lists nothing under `directory`.
    fn unlisted(&self, directory: &str) -> bool {
        self.text(&[
            "status",
            "--porcelain",
            "--untracked-files=all",
            "--",
            directory,
        ])
        .is_some_and(|text| text.is_empty())
    }
}

#[cfg(test)]
mod tests {
    use super::slug;

    #[test]
    fn slug_is_a_valid_goal_id_or_plan() {
        assert_eq!(
            slug(Some("/h/.claude/plans/make-a-plan-to-toasty-pixel.md")),
            "make-a-plan-to-toasty-pixel"
        );
        assert_eq!(slug(Some("/p/-x.md")), "plan");
        assert_eq!(slug(None), "plan");
    }
}

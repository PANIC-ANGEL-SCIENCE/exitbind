//! Claude Code plan approval to task card. `PostToolUse` for `ExitPlanMode`
//! fires only after the human approves a plan, and its `tool_response.plan`
//! carries the approved text. When the Lead's own approval (`isAgent` false;
//! an absent field is not proof and counts as a sub-agent) has a `Done when`
//! step table, the plan is saved as a new private card inside the project,
//! hidden from Git through the local exclude file, and the Lead gets one line
//! naming the setup command that registers it. Nothing is applied; without a
//! table nothing is written and nothing is printed.
//!
//! Cards are written only through real directories under the project root and
//! never over an existing file: the same plan approved again reuses its card,
//! and a different plan gets a fresh suffixed card, so a card the Lead edited
//! or registered is never replaced.

mod git;

use crate::config::Loaded;
use serde_json::{Map, Value};
use std::fs;
use std::io::ErrorKind;
use std::path::{Component, Path};
use std::time::{Duration, Instant};

/// Project-relative directory of saved plan cards.
const CARDS: &str = ".exitbind/plan-cards";
const PLAN_MAX_BYTES: usize = 64 * 1024;
/// Fresh card directories tried for one slug before giving up silently.
const MAX_SUFFIX: usize = 32;

pub(crate) fn is_plan_approval(payload: &Map<String, Value>) -> bool {
    payload.get("tool_name").and_then(Value::as_str) == Some("ExitPlanMode")
}

/// Save the approved plan as a card and return the Lead's one line, or `None`
/// when the approval is not the Lead's, the plan has no step table, or the
/// card could not be kept private.
pub(crate) fn on_approval(loaded: &Loaded, payload: &Map<String, Value>) -> Option<String> {
    let response = payload.get("tool_response")?;
    if response.get("isAgent").and_then(Value::as_bool) != Some(false) {
        return None;
    }
    let plan = response.get("plan").and_then(Value::as_str)?;
    if plan.len() > PLAN_MAX_BYTES || crate::project::setup::step_rows(plan).is_err() {
        return None;
    }
    let slug = slug(response.get("filePath").and_then(Value::as_str));
    let root = &loaded.product_root;
    let deadline = Instant::now() + Duration::from_secs(2);
    let git = git::Git::open(root, deadline);
    match &git {
        Some(git) => git.exclude(CARDS)?,
        // A Git marker that Git cannot read: never risk a listed card.
        None if root.ancestors().any(|dir| dir.join(".git").exists()) => return None,
        None => {}
    }
    let (relative, created) = save(root, &slug, plan).ok()?;
    if git.is_some_and(|git| !git.unlisted(CARDS)) {
        if created {
            let _ = fs::remove_file(root.join(&relative));
        }
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

/// The card directory name for attempt `index`: the slug, then `slug-2`,
/// `slug-3`, ... kept within the 64-character Goal ID bound.
fn candidate(slug: &str, index: usize) -> String {
    if index == 1 {
        return slug.to_owned();
    }
    let suffix = format!("-{index}");
    let keep = slug.len().min(64 - suffix.len());
    format!("{}{suffix}", &slug[..keep])
}

/// Write the plan as a new card, or reuse a card that already holds exactly
/// this plan. Returns the project-relative card path and whether it is new.
fn save(root: &Path, slug: &str, plan: &str) -> Result<(String, bool), String> {
    let cards = Path::new(CARDS);
    real_directories(root, cards)?;
    for index in 1..=MAX_SUFFIX {
        let name = candidate(slug, index);
        let relative = format!("{CARDS}/{name}/TASK.md");
        real_directories(root, &cards.join(&name))?;
        let path = root.join(&relative);
        match create_new_private(&path, plan) {
            Ok(()) => return Ok((relative, true)),
            Err(error) if error.kind() == ErrorKind::AlreadyExists => {
                if same_regular_file(&path, plan) {
                    return Ok((relative, false));
                }
            }
            Err(error) => return Err(error.to_string()),
        }
    }
    Err("no fresh plan card name is available".into())
}

/// Ensure every component of `relative` under `root` is a real directory,
/// creating missing ones privately; a symlink or other file is refused.
fn real_directories(root: &Path, relative: &Path) -> Result<(), String> {
    use std::os::unix::fs::DirBuilderExt;
    let mut current = root.to_path_buf();
    for component in relative.components() {
        let Component::Normal(name) = component else {
            return Err("unsafe plan card path".into());
        };
        current.push(name);
        match fs::symlink_metadata(&current) {
            Ok(info) if info.is_dir() && !info.file_type().is_symlink() => {}
            Ok(_) => return Err("plan card path has a symlink or non-directory".into()),
            Err(error) if error.kind() == ErrorKind::NotFound => fs::DirBuilder::new()
                .mode(0o700)
                .create(&current)
                .map_err(|error| error.to_string())?,
            Err(error) => return Err(error.to_string()),
        }
    }
    Ok(())
}

/// Create the card; never follows a symlink and never opens an existing file.
fn create_new_private(path: &Path, text: &str) -> std::io::Result<()> {
    use std::io::Write;
    use std::os::unix::fs::OpenOptionsExt;
    let mut file = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .custom_flags(libc::O_NOFOLLOW | libc::O_CLOEXEC)
        .open(path)?;
    let written = file
        .write_all(text.as_bytes())
        .and_then(|()| file.sync_all());
    if written.is_err() {
        drop(file);
        let _ = fs::remove_file(path);
    }
    written
}

fn same_regular_file(path: &Path, text: &str) -> bool {
    fs::symlink_metadata(path).is_ok_and(|info| info.is_file() && info.len() == text.len() as u64)
        && fs::read(path).is_ok_and(|bytes| bytes == text.as_bytes())
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

#[cfg(test)]
mod tests {
    use super::{candidate, slug};

    #[test]
    fn slug_is_a_valid_goal_id_or_plan() {
        assert_eq!(
            slug(Some("/h/.claude/plans/make-a-plan-to-toasty-pixel.md")),
            "make-a-plan-to-toasty-pixel"
        );
        assert_eq!(slug(Some("/p/-x.md")), "plan");
        assert_eq!(slug(None), "plan");
    }

    #[test]
    fn suffixed_card_names_stay_within_the_goal_id_bound() {
        assert_eq!(candidate("r9", 1), "r9");
        assert_eq!(candidate("r9", 2), "r9-2");
        let long = "a".repeat(64);
        assert_eq!(candidate(&long, 12).len(), 64);
        assert!(candidate(&long, 12).ends_with("-12"));
    }
}

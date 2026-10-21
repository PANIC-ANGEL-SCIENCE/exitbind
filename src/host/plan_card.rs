//! Claude Code plan approval to task card. `PostToolUse` for `ExitPlanMode`
//! fires only after the human approves a plan, and its `tool_response.plan`
//! carries the approved text. When the Lead's own approval (`isAgent` false;
//! an absent field is not proof and counts as a sub-agent) has a `Done when`
//! step table, the plan is saved as a new private card inside the project,
//! hidden from Git through the local exclude file, and the Lead gets one line
//! naming the setup command that registers it. Nothing is applied; without a
//! table nothing is written and nothing is printed.
//!
//! Cards are written through the one private writer
//! (`project::private_write`): the same plan approved again reuses its card,
//! and a different plan gets a fresh suffixed card, so a card the Lead edited
//! or registered is never replaced.

use crate::config::Loaded;
use serde_json::{Map, Value};
use std::path::Path;

/// Project-relative directory of saved plan cards.
pub(crate) const CARDS: &str = ".exitbind/plan-cards";
const PLAN_MAX_BYTES: usize = 64 * 1024;

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
    let (relative, _) = crate::project::private_write::write_suffixed(
        root,
        CARDS,
        &slug,
        "TASK.md",
        plan.as_bytes(),
        "plan card",
    )
    .ok()?;
    let setup = crate::emitted::command_line(
        &[
            "setup",
            "--apply",
            "--root",
            root.to_str()?,
            "--task-card",
            &relative,
        ],
        None,
    );
    Some(format!(
        "The approved plan has a Done when table and is saved as a private task card. To make it this Goal's start path, run `{setup}` with this project's usual setup options (--goal, --check-command, --review-policy)."
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

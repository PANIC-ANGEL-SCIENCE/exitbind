//! Setup diagnostics and recovery routes for managed project guidance.

use crate::project::skills as project_skills;
use serde_json::{json, Value};
use std::path::Path;

pub(super) fn diagnostics(control: &Path) -> Vec<Value> {
    project_skills::diagnose(control).into_iter().map(|item| json!({
        "skill":item.skill,"path":item.path,"state":state_label(item.state),
        "embeddedSha256":item.embedded_sha256,"observedSha256":item.observed_sha256,
        "customPreserved":matches!(item.state, project_skills::SkillObservationState::Unmanaged),
    })).collect()
}

pub(super) fn ensure_compatible(items: &[Value], control: &Path) -> Result<(), String> {
    for item in items {
        let state = item["state"].as_str().unwrap_or("unknown");
        if matches!(
            state,
            "absent"
                | "managed different"
                | "unsafe path or nonregular"
                | "unreadable"
                | "unsupported inspection"
        ) {
            let recovery = if matches!(state, "absent" | "managed different") {
                let root = control.to_str().ok_or("ControlRoot is not valid UTF-8")?;
                format!(
                    "inspect the destination, then run {}",
                    crate::emitted::Route::new(["init", "--refresh-skills", "--root", root])
                        .shell_text()
                )
            } else {
                "preserve and inspect the destination before applying setup".to_owned()
            };
            return Err(format!("setup refused managed guidance state for {} ({}; embedded SHA256 {}; observed SHA256 {}). {recovery}",
                item["path"].as_str().unwrap_or("unknown"), state,
                item["embeddedSha256"].as_str().unwrap_or("unknown"),
                item["observedSha256"].as_str().unwrap_or("unknown")));
        }
    }
    Ok(())
}

fn state_label(state: project_skills::SkillObservationState) -> &'static str {
    match state {
        project_skills::SkillObservationState::Absent => "absent",
        project_skills::SkillObservationState::EmbeddedMatch => "embedded match",
        project_skills::SkillObservationState::ManagedDifferent => "managed different",
        project_skills::SkillObservationState::Unmanaged => "custom",
        project_skills::SkillObservationState::Unsafe => "unsafe path or nonregular",
        project_skills::SkillObservationState::Unreadable => "unreadable",
        project_skills::SkillObservationState::Unsupported => "unsupported inspection",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn recoverable_guidance_names_the_control_root_and_unsafe_paths_do_not_refresh() {
        let root = Path::new("/fixture/control-root");
        let error = ensure_compatible(&[json!({"state":"absent"})], root).unwrap_err();
        assert!(error.contains("--root /fixture/control-root"), "{error}");
        let unsafe_error =
            ensure_compatible(&[json!({"state":"unsafe path or nonregular"})], root).unwrap_err();
        assert!(!unsafe_error.contains("--refresh-skills"), "{unsafe_error}");
    }
}

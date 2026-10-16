use serde_json::{json, Value};
use std::path::Path;

pub(super) fn apply_config(
    next: &mut Value,
    selection: Option<&str>,
    already_selected: bool,
) -> bool {
    if selection.is_none() {
        return false;
    }
    next["project"]["verificationPreparation"] = json!({
        "version": 1,
        "pythonBytecode": "disabled",
        "scratch": "taskOwned"
    });
    !already_selected
}

pub(super) fn preview(selection: &str, state: &Path) -> Value {
    json!({
        "selection": selection,
        "version": 1,
        "pythonBytecode": "disabled",
        "scratch": "taskOwned",
        "scratchPath": state.join(crate::project::layout_types::state_namespace()).join("verification/<run-id>/<operation-id>"),
        "limits": {
            "scope": "one run and one operation directory",
            "size": "no product quota; managed filesystem capacity applies",
            "cleanup": "retained in managed state"
        }
    })
}

pub(super) fn print(preparation: &Value) {
    if let Some(preparation) = preparation.as_object() {
        println!(
            "Verification preparation: Python bytecode {}, scratch {}.",
            preparation["pythonBytecode"].as_str().unwrap_or("unknown"),
            preparation["scratch"].as_str().unwrap_or("unknown")
        );
        println!(
            "  Owned path: {}",
            preparation["scratchPath"].as_str().unwrap_or("unknown")
        );
        println!(
            "  Scope: {}; size: {}; lifecycle: {}.",
            preparation["limits"]["scope"].as_str().unwrap_or("unknown"),
            preparation["limits"]["size"].as_str().unwrap_or("unknown"),
            preparation["limits"]["cleanup"]
                .as_str()
                .unwrap_or("unknown")
        );
    }
}

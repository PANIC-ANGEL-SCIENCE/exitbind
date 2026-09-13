//! Build identity recorded separately from persisted schema versions.

use serde_json::{json, Value};

pub(crate) fn evidence() -> Value {
    let name = if exitbind_surface() {
        "exitbind"
    } else {
        "soulmate"
    };
    json!({
        "name": name,
        "version": env!("CARGO_PKG_VERSION"),
        "commit": option_env!("EXITBIND_BUILD_COMMIT").or(option_env!("SOULMATE_BUILD_COMMIT")),
    })
}

pub(crate) fn evidence_for_version(version: u64) -> Value {
    if version <= 4 {
        json!({"name":"soulmate","version":env!("CARGO_PKG_VERSION"),"commit":option_env!("SOULMATE_BUILD_COMMIT")})
    } else {
        evidence()
    }
}

pub(crate) fn exitbind_surface() -> bool {
    std::env::current_exe()
        .ok()
        .and_then(|path| {
            path.file_name()
                .and_then(|name| name.to_str())
                .map(|name| name == "exitbind")
        })
        .unwrap_or(false)
}

pub(crate) fn valid(value: &Value) -> bool {
    let Some(object) = value.as_object() else {
        return false;
    };
    object.len() == 3
        && object.contains_key("name")
        && object.contains_key("version")
        && object.contains_key("commit")
        // Historical v1-v4 records retain their original producer identity.
        && matches!(value["name"].as_str(), Some("exitbind" | "soulmate"))
        && value["version"]
            .as_str()
            .is_some_and(|version| !version.trim().is_empty())
        && (value["commit"].is_null()
            || value["commit"].as_str().is_some_and(|commit| {
                commit.len() == 40 && commit.bytes().all(|byte| byte.is_ascii_hexdigit())
            }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn producer_accepts_only_its_frozen_contract() {
        let base = json!({"name":"soulmate","version":"0.2.1","commit":null});
        assert!(valid(&base));
        let mut additive = base;
        additive["future"] = json!(true);
        assert!(!valid(&additive));
        assert!(!valid(
            &json!({"name":"soulmate","version":"","commit":null})
        ));
        assert!(!valid(
            &json!({"name":"other","version":"0.2.1","commit":null})
        ));
    }
}

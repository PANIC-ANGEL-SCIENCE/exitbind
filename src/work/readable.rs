//! Product-owned, exact grouped consumption for the current Work.
//!
//! The legacy section reader remains hex encoded for compatibility. This
//! versioned reader resolves and validates the same current binding, then
//! presents bounded UTF-8 artifacts directly so consumers do not implement
//! paging, decoding, or integrity checks themselves.

use crate::{config::Loaded, run::RunSnapshot};
use serde_json::{json, Map, Value};

#[path = "recipient_context.rs"]
mod recipient_context;

pub(crate) const PREFIX: &str = "ref:work-detail:v1:";
pub(crate) const MAX_GROUPED_BYTES: usize = 256 * 1024;

pub(crate) fn is_grouped(reference: &str) -> bool {
    reference.starts_with(PREFIX)
}

fn token(binding: &str) -> String {
    format!("{PREFIX}{binding}")
}

fn route(loaded: &Loaded, work: &str, binding: &str) -> Value {
    let command = super::response_recovery::bounded_argv(
        vec![
            "work".into(),
            "expand".into(),
            work.into(),
            token(binding),
            "--json".into(),
            "--config".into(),
        ],
        loaded.path.to_str(),
        1024,
    );
    json!({
        "version": 1,
        "kind": "work_detail",
        "reference": token(binding),
        "command": command.argv,
        "sameConfigRequired": command.same_config,
        "sameExecutableRequired": command.same_executable,
        "readOnly": true,
    })
}

pub(crate) fn attach(loaded: &Loaded, work: &str, binding: &str, current: &mut Value) {
    if let Some(details) = current.get_mut("details").and_then(Value::as_object_mut) {
        details.insert("grouped".into(), route(loaded, work, binding));
    }
}

pub(crate) fn read(
    loaded: &Loaded,
    work: &str,
    snapshot: &RunSnapshot,
    next: &Value,
) -> Result<Value, String> {
    read_after_reads(loaded, work, snapshot, next, || Ok(()))
}

fn read_after_reads(
    loaded: &Loaded,
    work: &str,
    snapshot: &RunSnapshot,
    next: &Value,
    after_reads: impl FnOnce() -> Result<(), String>,
) -> Result<Value, String> {
    super::details::ensure_current_config(loaded)?;
    let mut canonical = next.clone();
    canonical
        .as_object_mut()
        .ok_or("current action unavailable")?
        .remove("current");
    if let Some(assignment) = canonical["assignment"].as_str().map(str::to_owned) {
        super::attach_held(loaded, work, &assignment, &mut canonical)?;
    }
    let binding = super::details::binding(loaded, work, snapshot, &canonical)?;
    let recipient = recipient_context::read(loaded, &canonical)?;
    let sections = grouped_sections(loaded, work, snapshot, &canonical, &binding)?;
    after_reads()?;
    super::details::ensure_current_config(loaded)?;
    let fresh_snapshot = RunSnapshot::capture(loaded, &super::resolve(loaded, work)?)?;
    let fresh = super::next_from(loaded, work, &fresh_snapshot)?;
    let mut fresh_canonical = fresh.clone();
    fresh_canonical
        .as_object_mut()
        .ok_or("current action unavailable")?
        .remove("current");
    if super::details::binding(loaded, work, &fresh_snapshot, &fresh_canonical)? != binding {
        return Err("work detail changed during expansion; refresh with work next".into());
    }
    if recipient_context::read(loaded, &fresh_canonical)? != recipient {
        return Err(
            "recipient instructions changed during work detail; refresh the current assignment"
                .into(),
        );
    }
    response(
        loaded, work, &binding, &canonical, sections, recipient, snapshot,
    )
}

pub(crate) fn current(loaded: &Loaded, work: &str) -> Result<Value, String> {
    let ledger = super::resolve(loaded, work)?;
    let snapshot = RunSnapshot::capture(loaded, &ledger)?;
    let next = super::next_from(loaded, work, &snapshot)?;
    read(loaded, work, &snapshot, &next)
}

pub(crate) fn expand(
    loaded: &Loaded,
    work: &str,
    snapshot: &RunSnapshot,
    next: &Value,
    requested: &str,
) -> Result<Value, String> {
    super::details::ensure_current_config(loaded)?;
    let binding = requested
        .strip_prefix(PREFIX)
        .filter(|value| !value.is_empty() && !value.contains(':'))
        .ok_or("malformed grouped work detail reference; refresh with work next")?;
    let mut canonical = next.clone();
    canonical
        .as_object_mut()
        .ok_or("current action unavailable")?
        .remove("current");
    let expected = super::details::binding(loaded, work, snapshot, &canonical)?;
    if binding != expected {
        return Err("work detail reference is stale, revoked or belongs to another project/Work/recipient; refresh with work next".into());
    }
    read(loaded, work, snapshot, next)
}

fn grouped_sections(
    loaded: &Loaded,
    work: &str,
    snapshot: &RunSnapshot,
    next: &Value,
    binding: &str,
) -> Result<Map<String, Value>, String> {
    let mut sections = Map::new();
    let mut assignment = super::details::section_value(loaded, work, snapshot, next, "assignment")?;
    // This is a delivery projection, not a replacement canonical packet.
    // Only byte-equal aliases are removed; every unequal fact is retained.
    let mut aliases = Vec::new();
    for (residual_key, context_key) in [
        ("context", None),
        ("goal", Some("goal")),
        ("currentSubject", Some("subject")),
        ("remaining", Some("obligations")),
    ] {
        let original = &assignment["residual"][residual_key];
        let context = &assignment["assignment"]["context"];
        let replacement = context_key.map_or(context, |key| &context[key]);
        if !original.is_null() && original == replacement {
            assignment["residual"]
                .as_object_mut()
                .unwrap()
                .remove(residual_key);
            aliases.push(json!({"removed": format!("residual.{residual_key}"),
                "source": context_key.map_or("assignment.context".to_owned(), |key| format!("assignment.context.{key}"))}));
        }
    }
    if assignment["assignment"].is_object() {
        assignment["assignment"] = super::action::delivery_packet(&assignment["assignment"]);
    }
    assignment["projection"] = json!({"equalAliases": aliases,
        "recoveryAliases": "equal recovery goal/scope/subject/current/missing/loop/next refer to context goal/scope/subject/evidence/obligations/loop/next"});
    sections.insert("assignment".into(), assignment);
    let mut evidence = super::details::readable_evidence(loaded, snapshot, next)?;
    for item in evidence["items"].as_array_mut().unwrap() {
        for field in ["artifact", "stdout", "stderr"] {
            if item[field].is_object() && item[field]["readable"] == false {
                item[field]["access"] = json!({"currentMetadata": route(loaded, work, binding),
                    "legacySection": super::details::legacy_evidence_route(loaded, work, binding),
                    "limitation": "non-UTF-8 material remains exact bytes in the legacy section; oversized material exposes verified metadata only"});
            }
        }
    }
    sections.insert("evidence".into(), evidence);
    sections.insert(
        "tasks".into(),
        super::details::section_value(loaded, work, snapshot, next, "tasks")?,
    );
    Ok(sections)
}

fn response(
    loaded: &Loaded,
    work: &str,
    binding: &str,
    next: &Value,
    sections: Map<String, Value>,
    recipient: Value,
    snapshot: &RunSnapshot,
) -> Result<Value, String> {
    let readable_complete =
        evidence_complete(&sections["evidence"]) && recipient["complete"] == true;
    let mut value = json!({
        "compact": true,
        "valid": true,
        "version": 1,
        "kind": "work_detail",
        "reference": token(binding),
        "work": work,
        "binding": binding,
        "recipient": {"role": next["role"], "agent": next["agent"], "assignment": next["assignment"], "actor": next["resolvedActor"]},
        "complete": readable_complete,
        "sections": sections,
        "recipientContext": recipient,
        "usageAccount": crate::session_goal::usage::details_for_work(loaded, work),
        "actionForms": super::action_forms::full(loaded, work, next, binding),
        "transport": {"encoding": "utf-8", "exact": readable_complete, "modelPaging": false},
    });
    if next["action"] == "done" {
        value["completion"] = super::closeout::projection(loaded, work, snapshot, next);
    }
    let canonical = json!({
        "current":{"action":next["action"], "binding":binding, "readiness":next["progress"]["state"]},
        "actionForms":value["actionForms"], "warnings":next["warnings"],
    });
    super::effective_action::insert(&mut value, &canonical);
    let bytes = serde_json::to_vec(&value).map_err(|error| error.to_string())?;
    if bytes.len() + 1 > MAX_GROUPED_BYTES {
        return Err("grouped work detail exceeds the bounded readable channel; use the current section routes".into());
    }
    Ok(value)
}

pub(super) fn evidence_complete(value: &Value) -> bool {
    value["items"].as_array().is_some_and(|items| {
        items.iter().all(|item| {
            ["artifact", "stdout", "stderr"].iter().all(|key| {
                !item[*key].is_object()
                    || (item[*key]["readable"] == true && item[*key]["complete"] == true)
            })
        })
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn rules_changed_during_actual_detail_assembly_are_refused() {
        // Grouped current-Work delivery is not a historical Soulmate protocol.
        if !crate::compatibility::is_exitbind() {
            return;
        }
        let stamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let root = std::env::temp_dir().join(format!(
            "exitbind-detail-mid-read-{}-{stamp}",
            std::process::id()
        ));
        std::fs::create_dir_all(&root).unwrap();
        let mut git = std::process::Command::new("git");
        for key in [
            "GIT_DIR",
            "GIT_WORK_TREE",
            "GIT_COMMON_DIR",
            "GIT_INDEX_FILE",
            "GIT_CEILING_DIRECTORIES",
            "GIT_DISCOVERY_ACROSS_FILESYSTEM",
        ] {
            git.env_remove(key);
        }
        assert!(git
            .args(["-C", root.to_str().unwrap(), "init", "--quiet"])
            .status()
            .unwrap()
            .success());
        let config = crate::project::onboarding::init_with_options(
            crate::project::onboarding::InitOptions {
                product_root: root.to_str().unwrap(),
                coffee: false,
                skip_skills: true,
                mode: Some("portable"),
                project_id: None,
                control_root: None,
                state_root: None,
            },
        )
        .unwrap();
        let loaded = crate::config::load(config.to_str()).unwrap();
        std::fs::write(root.join("AGENTS.md"), "before\n").unwrap();
        let begun = crate::work::begin(
            &loaded,
            crate::work::BeginOptions {
                workflow: "change",
                goal: "mid-read detail",
                check_command: "true",
                boundary: None,
                harness_receipt: None,
                proof_origin: None,
                preserve_requirement: None,
                preservation_check_command: None,
                preservation_proof_origin: None,
                basis: None,
                review_policy: Some("required"),
            },
        )
        .unwrap();
        let work = begun["work"].as_str().unwrap();
        let snapshot =
            RunSnapshot::capture(&loaded, &crate::work::resolve(&loaded, work).unwrap()).unwrap();
        let next = crate::work::next_from(&loaded, work, &snapshot).unwrap();
        let error = read_after_reads(&loaded, work, &snapshot, &next, || {
            std::fs::write(root.join("AGENTS.md"), "after\n").map_err(|e| e.to_string())
        })
        .unwrap_err();
        assert!(error.contains("recipient instructions changed"), "{error}");
        std::fs::remove_dir_all(root).unwrap();
    }
}

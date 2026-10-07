//! Deliver an explicitly requested current context after the existing start.
//! A delivery failure cannot undo or conceal the already recorded Work.

use crate::config::Loaded;
use serde_json::{json, Value};

pub(crate) fn with_detail(loaded: &Loaded, started: Value) -> Value {
    let work = started["work"]
        .as_str()
        .expect("successful Work start carries its handle");
    deliver(loaded, work, started["focus"].clone(), || {
        super::readable::current(loaded, work)
    })
}

fn deliver(
    loaded: &Loaded,
    work: &str,
    focus: Value,
    read: impl FnOnce() -> Result<Value, String>,
) -> Value {
    let detail = read().and_then(|mut detail| {
        detail["creation"] = json!({"effect": "recorded", "focus": focus});
        if serde_json::to_vec(&detail)
            .map_err(|error| error.to_string())?
            .len()
            + 1
            > super::readable::MAX_GROUPED_BYTES
        {
            return Err("started Work detail exceeds the bounded readable channel".into());
        }
        Ok(detail)
    });
    match detail {
        Ok(detail) => detail,
        Err(error) => {
            let route = super::response_recovery::bounded_argv(
                vec![
                    "work".into(),
                    "detail".into(),
                    work.into(),
                    "--json".into(),
                    "--config".into(),
                ],
                loaded.path.to_str(),
                1024,
            );
            json!({"work": work, "effect": "recorded", "status": "recorded",
                "complete": false, "creation": {"effect": "recorded", "focus": focus},
                "reason": {"code": "recorded_then_failed"},
                "projectionError": error.chars().take(512).collect::<String>(),
                "nextAction": {"type": "inspect", "safe": true, "readOnly": true,
                    "command": route.argv, "sameConfigRequired": route.same_config,
                    "sameExecutableRequired": route.same_executable},
                "recovery": "Inspect this existing Work; do not repeat work begin. If its reply was lost, use work resume --json with the same configuration."})
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn failed_delivery_retains_the_recorded_work_and_read_only_recovery() {
        let root =
            std::env::temp_dir().join(format!("exitbind-start-delivery-{}", std::process::id()));
        std::fs::create_dir_all(&root).unwrap();
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
        let started = crate::work::begin(
            &loaded,
            crate::work::BeginOptions {
                workflow: "change",
                goal: "Preserve the committed start on failed delivery",
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
        let work = started["work"].as_str().unwrap();
        for error in [
            "configuration changed during delivery",
            "grouped work detail exceeds the bounded readable channel",
        ] {
            let result = deliver(
                &loaded,
                work,
                started["focus"].clone(),
                || Err(error.into()),
            );
            assert_eq!(result["work"], work);
            assert_eq!(result["effect"], "recorded");
            assert_eq!(result["complete"], false);
            assert_eq!(result["reason"]["code"], "recorded_then_failed");
            assert_eq!(result["nextAction"]["readOnly"], true);
            assert_eq!(result["nextAction"]["command"][2], "detail");
        }
        // Actual stale configuration after a successful start exercises the
        // currentness owner and must keep the committed handle inspectable.
        std::fs::write(&config, format!("{}\n", loaded.source)).unwrap();
        let result = with_detail(&loaded, started.clone());
        assert_eq!(result["work"], work);
        assert_eq!(result["effect"], "recorded");
        assert_eq!(result["complete"], false);
        assert!(result["projectionError"]
            .as_str()
            .unwrap()
            .contains("configuration changed"));
        let fresh = crate::config::load(config.to_str()).unwrap();
        let resumed = crate::work::resume(&fresh, false).unwrap();
        assert_eq!(
            resumed["work"], work,
            "delivery failure must not duplicate or lose Work"
        );
        std::fs::remove_dir_all(root).unwrap();
    }
}

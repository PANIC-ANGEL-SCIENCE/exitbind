use super::*;

#[test]
fn rejected_identity_wins_over_a_new_prepared_intent() {
    let intents = vec![
        json!({
            "state": "abandoned",
            "session": "parent",
            "nativeChild": "child-a",
            "agentType": "worker",
            "failure": "the selected assignment changed before a child started"
        }),
        json!({
            "state": "prepared",
            "session": "parent",
            "nativeChild": null,
            "agentType": "worker"
        }),
    ];
    let intent = rejected(&intents, "parent", "child-a", Some("worker"))
        .expect("the prior rejected child identity must remain decisive");
    assert_eq!(
        rejection_reason(intent),
        "the selected assignment changed before a child started"
    );
}

#[cfg(test)]
mod adviser_floor {
    use super::adviser::validate_requested_binding;
    use crate::config::{types, Loaded};
    use serde_json::{json, Value};
    use std::path::PathBuf;

    fn loaded(
        host: Option<&str>,
        model: Option<&str>,
        effort: Option<&str>,
        required: &[&str],
    ) -> Loaded {
        let runtime = json!({
            "host": host,
            "model": model,
            "reasoningEffort": effort,
        });
        let config = json!({
            "project": {"adviserFloor":{"version":1,"bindings":[{
                "host":"codex","model":"gpt-approved","reasoningEffort":"xhigh",
                "requiredCapabilities":required
            }]}},
            "agents":{"adviser":{"profile":"profile.md","purpose":"advise",
                "retention":"task","crossContext":"none","runtime":runtime}},
            "workflows":{"change":{"advisers":["adviser"]}}
        });
        Loaded {
            agents: types::from_validated_agents(&config["agents"]).unwrap(),
            workflows: serde_json::from_value(config["workflows"].clone()).unwrap(),
            config,
            lead: None,
            architecture_contract: None,
            capabilities: Vec::new(),
            verification_preparation: None,
            path: PathBuf::from("/control/exitbind.json"),
            control_root: PathBuf::from("/control"),
            product_root: PathBuf::from("/product"),
            state_root: PathBuf::from("/state"),
            mode: crate::project::layout_types::Mode::Portable,
            project_id: None,
            source: String::new(),
        }
    }

    fn packet(marker: bool, role: &str) -> Value {
        let mut packet = json!({
            "agent":"adviser","role":role,"goal":"review this task",
            "runtime":{"host":"codex","model":"frozen","reasoningEffort":"low"}
        });
        if marker {
            packet["guidanceContinuityProtocol"] = json!(1);
        }
        packet
    }

    #[test]
    fn marked_native_adviser_requires_the_exact_current_requested_binding() {
        let adviser_packet = packet(true, "adviser");
        let allowed = loaded(Some("codex"), Some("gpt-approved"), Some("xhigh"), &[]);
        assert!(validate_requested_binding(&allowed, &adviser_packet, "codex").is_ok());

        let unresolved = loaded(Some("codex"), None, Some("xhigh"), &[]);
        assert!(
            validate_requested_binding(&unresolved, &adviser_packet, "codex")
                .unwrap_err()
                .contains("model is unresolved")
        );
        let below_floor = loaded(Some("codex"), Some("gpt-other"), Some("xhigh"), &[]);
        assert!(
            validate_requested_binding(&below_floor, &adviser_packet, "codex")
                .unwrap_err()
                .contains("outside the approved floor")
        );
        assert!(
            validate_requested_binding(&allowed, &adviser_packet, "claude")
                .unwrap_err()
                .contains("differs from the bound receiving host")
        );
    }

    #[test]
    fn marked_native_adviser_requires_current_floor_capabilities_but_legacy_and_other_roles_pass() {
        let adviser_packet = packet(true, "adviser");
        let missing_capability = loaded(
            Some("codex"),
            Some("gpt-approved"),
            Some("xhigh"),
            &["review-lens"],
        );
        assert!(
            validate_requested_binding(&missing_capability, &adviser_packet, "codex")
                .unwrap_err()
                .contains("capability unavailable")
        );

        let unresolved = loaded(None, None, None, &[]);
        assert!(
            validate_requested_binding(&unresolved, &packet(false, "adviser"), "codex").is_ok()
        );
        assert!(validate_requested_binding(&unresolved, &packet(true, "worker"), "codex").is_ok());
    }
}

// Checks exact required instruction bytes for child delivery.
// Refuses a profile not presented exactly; see `src/session_goal/continuity/capture.rs`.
//! Check exact required instructional bytes for selected native delivery.
use crate::evidence::hash;
use serde_json::Value;

pub(super) const CODEX_UNVERIFIED: &str = "codex_direct_hook_limit_unverified";

#[derive(Clone, Copy)]
pub(super) enum RecordedProof<'a> {
    Legacy,
    Selected { facts: &'a str, contract: &'a str },
    Invalid,
}

/// Absence identifies an older presentation only when neither selected proof
/// field exists. The marker is a version discriminator, not authentication.
pub(super) fn recorded_proof(intent: &Value) -> RecordedProof<'_> {
    let delivery = &intent["assignmentDelivery"];
    match (
        intent.get("selectedDeliveryProtocol"),
        delivery.get("actorFactsSha256"),
        delivery.get("actorContractSha256"),
    ) {
        (None, None, None) => RecordedProof::Legacy,
        (Some(version), Some(facts), Some(contract))
            if *version == 1 && intent["host"] == "claude" =>
        {
            fn digest(value: &Value) -> Option<&str> {
                value.as_str().filter(|value| {
                    value.len() == 64
                        && value
                            .bytes()
                            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
                })
            }
            match (digest(facts), digest(contract)) {
                (Some(facts), Some(contract)) => RecordedProof::Selected { facts, contract },
                _ => RecordedProof::Invalid,
            }
        }
        _ => RecordedProof::Invalid,
    }
}

fn exact_instruction(value: &Value) -> bool {
    let Some(content) = value["content"].as_str() else {
        return false;
    };
    let Some(source) = value["sourceSha256"].as_str() else {
        return false;
    };
    value["presentedSha256"] == source && hash::text(content) == source
}

/// Keep legacy redacted readers unchanged; transformed required material may
/// not establish the new selected view's complete-delivery proof.
pub(super) fn exact_instructions(result: &Value) -> Result<(), String> {
    if !exact_instruction(&result["profile"]) {
        return Err("required native profile was not presented exactly".into());
    }
    let perspectives = result["perspectives"]
        .as_array()
        .ok_or("required native perspectives are unavailable")?;
    if perspectives.iter().any(|item| !exact_instruction(item)) {
        return Err("required native perspective was not presented exactly".into());
    }
    if !result["capabilities"].is_null() {
        let items = result["capabilities"]["items"]
            .as_array()
            .ok_or("required native capabilities are unavailable")?;
        if items.iter().any(|item| !exact_instruction(item)) {
            return Err("required native capability was not presented exactly".into());
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn item(content: &str) -> Value {
        let digest = hash::text(content);
        json!({"content":content,"sourceSha256":digest,"presentedSha256":digest})
    }

    #[test]
    fn selected_material_requires_exact_profile_perspectives_and_capabilities() {
        let original = json!({"profile":item("required profile"),
            "perspectives":[item("required perspective")],
            "capabilities":{"items":[item("required capability")]}});
        assert!(exact_instructions(&original).is_ok());
        for pointer in ["/profile", "/perspectives/0", "/capabilities/items/0"] {
            let mut changed = original.clone();
            changed.pointer_mut(pointer).unwrap()["content"] = json!("[path redacted]");
            assert!(exact_instructions(&changed).is_err(), "{pointer}");
            changed = original.clone();
            changed.pointer_mut(pointer).unwrap()["presentedSha256"] = json!("different");
            assert!(exact_instructions(&changed).is_err(), "{pointer}");
        }
        let mut optional = original;
        optional["capabilities"] = Value::Null;
        assert!(exact_instructions(&optional).is_ok());
        optional["perspectives"] = Value::Null;
        assert!(exact_instructions(&optional).is_err());
    }

    #[test]
    fn selected_delivery_protocol_cannot_fall_back_to_legacy_on_partial_proof() {
        let selected = json!({"host":"claude","selectedDeliveryProtocol":1,
            "assignmentDelivery":{"actorFactsSha256":"a".repeat(64),
                "actorContractSha256":"b".repeat(64)}});
        assert!(matches!(
            recorded_proof(&selected),
            RecordedProof::Selected { .. }
        ));
        for fields in [
            &["actorFactsSha256"][..],
            &["actorContractSha256"][..],
            &["actorFactsSha256", "actorContractSha256"][..],
        ] {
            let mut missing = selected.clone();
            for field in fields {
                missing["assignmentDelivery"]
                    .as_object_mut()
                    .unwrap()
                    .remove(*field);
            }
            assert!(matches!(recorded_proof(&missing), RecordedProof::Invalid));
        }
        for value in [json!(null), json!(2), json!("1")] {
            let mut changed = selected.clone();
            changed["selectedDeliveryProtocol"] = value;
            assert!(matches!(recorded_proof(&changed), RecordedProof::Invalid));
        }
        let mut wrong_host = selected.clone();
        wrong_host["host"] = json!("codex");
        assert!(matches!(
            recorded_proof(&wrong_host),
            RecordedProof::Invalid
        ));
        let mut legacy = selected;
        legacy
            .as_object_mut()
            .unwrap()
            .remove("selectedDeliveryProtocol");
        let delivery = legacy["assignmentDelivery"].as_object_mut().unwrap();
        delivery.remove("actorFactsSha256");
        delivery.remove("actorContractSha256");
        assert!(matches!(recorded_proof(&legacy), RecordedProof::Legacy));
    }
}

//! Lossless-for-delivery projection of a verified native assignment packet.

use serde_json::Value;

const RECOVERY_ALIASES: [(&str, &str); 7] = [
    ("goal", "goal"),
    ("scope", "scope"),
    ("subject", "subject"),
    ("current", "evidence"),
    ("missing", "obligations"),
    ("loop", "loop"),
    ("next", "next"),
];

pub(super) fn delivery_packet(assignment: &Value) -> Value {
    let mut display = assignment.clone();
    let Some(context) = display["context"].as_object_mut() else {
        return display;
    };
    let duplicates = RECOVERY_ALIASES
        .iter()
        .filter(|(recovery, top)| {
            context
                .get("recovery")
                .and_then(|value| value.get(*recovery))
                == context.get(*top)
        })
        .map(|(recovery, _)| *recovery)
        .collect::<Vec<_>>();
    if let Some(recovery) = context.get_mut("recovery").and_then(Value::as_object_mut) {
        for key in duplicates {
            recovery.remove(key);
        }
    }
    display
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn delivery_deduplicates_only_equal_recovery_fields() {
        let original = json!({"context":{
            "goal":"task", "scope":{"owner":"lead"}, "subject":{"sha256":"a"},
            "evidence":[{"ref":"x"}], "obligations":["check"],
            "loop":{"state":"open"}, "next":{"action":"review"},
            "digest":"canonical-digest",
            "recovery":{
                "goal":"task", "scope":{"owner":"lead"}, "subject":{"sha256":"a"},
                "current":[{"ref":"x"}], "missing":["check"],
                "loop":{"state":"open"}, "next":{"action":"different"},
                "stale":["old"], "conflicts":[]
            }
        }});
        let delivered = delivery_packet(&original);
        assert_eq!(delivered["context"]["digest"], "canonical-digest");
        assert_eq!(
            delivered["context"]["recovery"]["next"],
            json!({"action":"different"})
        );
        assert_eq!(delivered["context"]["recovery"]["stale"], json!(["old"]));
        assert!(delivered["context"]["recovery"].get("current").is_none());
        assert!(delivered["context"]["recovery"].get("missing").is_none());
        assert_eq!(
            original["context"]["recovery"]["current"],
            json!([{"ref":"x"}])
        );
    }
}

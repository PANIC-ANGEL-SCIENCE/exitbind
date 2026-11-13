// Serializes CLI JSON and measures its output budget.
// Fit checks reserve one byte for the newline; see `src/presentation/mod.rs`.
//! The CLI's JSON representation and its terminating-newline budget.

use serde_json::Value;

pub(crate) fn serialize(value: &Value) -> Result<String, String> {
    if value["compact"] == true {
        serde_json::to_string(value).map_err(|error| error.to_string())
    } else {
        serde_json::to_string_pretty(value).map_err(|error| error.to_string())
    }
}

/// The CLI writes exactly one newline after the serialized value.
pub(crate) fn fits(value: &Value, limit: usize) -> Result<bool, String> {
    Ok(serialize(value)?.len() < limit)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn newline_and_escaping_fit_the_actual_serialization_boundary() {
        for value in [
            json!({"text":"quote \" and newline\n"}),
            json!({"compact":true,"text":"quote \" and newline\n"}),
        ] {
            let bytes = serialize(&value).unwrap().len();
            assert!(fits(&value, bytes + 1).unwrap());
            assert!(!fits(&value, bytes).unwrap());
        }
    }
}

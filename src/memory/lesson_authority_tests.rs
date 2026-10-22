//! The schema is the oracle: every `$defs.agent` property is classified as
//! lesson authority or not, a new property fails until it is classified, and
//! the property scan is proven by a second scan written differently (brace
//! depth over the raw schema text instead of JSON pointer lookup).
use super::*;
use serde_json::json;
use std::collections::BTreeSet;

const SCHEMA: &str = include_str!("../../schema/exitbind.schema.json");

/// Scan 1: parse the schema and read `$defs.agent.properties` keys.
fn pointer_scan(schema: &str) -> BTreeSet<String> {
    let schema: Value = serde_json::from_str(schema).unwrap();
    schema
        .pointer("/$defs/agent/properties")
        .and_then(Value::as_object)
        .expect("schema $defs.agent.properties")
        .keys()
        .cloned()
        .collect()
}

/// Scan 2: no JSON parser. Find the `"agent"` object under `"$defs"` by
/// text, then its `"properties"` object, and collect the quoted keys that
/// sit at exactly one brace depth inside it.
fn text_scan(schema: &str) -> BTreeSet<String> {
    let defs = schema.find("\"$defs\"").expect("$defs");
    let agent = defs + schema[defs..].find("\"agent\":").expect("agent def");
    let properties = agent + schema[agent..].find("\"properties\":").expect("properties");
    let open = properties + schema[properties..].find('{').unwrap();
    let bytes = schema.as_bytes();
    let (mut depth, mut index, mut in_string, mut start) = (0usize, open, false, 0usize);
    let mut keys = BTreeSet::new();
    loop {
        let byte = bytes[index];
        if in_string {
            if byte == b'\\' {
                index += 1;
            } else if byte == b'"' {
                in_string = false;
                let rest = schema[index + 1..].trim_start();
                if depth == 1 && rest.starts_with(':') {
                    keys.insert(schema[start..index].to_owned());
                }
            }
        } else {
            match byte {
                b'"' => {
                    in_string = true;
                    start = index + 1;
                }
                b'{' | b'[' => depth += 1,
                b'}' | b']' => {
                    depth -= 1;
                    if depth == 0 {
                        return keys;
                    }
                }
                _ => {}
            }
        }
        index += 1;
    }
}

/// Schema properties without a class, and classes without a property.
fn unclassified(properties: &BTreeSet<String>) -> (Vec<String>, Vec<&'static str>) {
    let classified: BTreeSet<&str> = AGENT_FIELDS.iter().map(|(field, _)| *field).collect();
    let missing = properties
        .iter()
        .filter(|p| !classified.contains(p.as_str()))
        .cloned()
        .collect();
    let extra = classified
        .iter()
        .filter(|c| !properties.contains(**c))
        .copied()
        .collect();
    (missing, extra)
}

#[test]
fn every_schema_agent_property_is_classified_and_no_class_is_stale() {
    let properties = pointer_scan(SCHEMA);
    assert!(properties.len() >= 20, "{properties:?}");
    let (missing, extra) = unclassified(&properties);
    assert!(
        missing.is_empty(),
        "classify each new agent property in src/memory/lesson_authority.rs: {missing:?}"
    );
    assert!(
        extra.is_empty(),
        "classified fields without a schema property: {extra:?}"
    );
    let mut seen = BTreeSet::new();
    for (field, _) in AGENT_FIELDS {
        assert!(seen.insert(*field), "classified twice: {field}");
    }
}

#[test]
fn the_two_scans_agree_and_both_see_a_seeded_new_property() {
    assert_eq!(pointer_scan(SCHEMA), text_scan(SCHEMA));
    let anchor = "\"properties\": {";
    let agent = SCHEMA.find("\"agent\":").unwrap();
    let at = agent + SCHEMA[agent..].find(anchor).unwrap() + anchor.len();
    let seeded = format!(
        "{}\n        \"voice\": {{\"type\": \"string\"}},{}",
        &SCHEMA[..at],
        &SCHEMA[at..]
    );
    for scan in [pointer_scan(&seeded), text_scan(&seeded)] {
        assert!(scan.contains("voice"), "{scan:?}");
        let (missing, extra) = unclassified(&scan);
        assert_eq!(missing, ["voice"]);
        assert!(extra.is_empty());
    }
}

#[test]
fn not_authority_is_exactly_identity_and_phase_guidance() {
    assert_eq!(
        not_authority().collect::<Vec<_>>(),
        ["profile", "displayName", "nativeName", "phaseGuidance"]
    );
    for authority in [
        "memoryWrite",
        "memoryReview",
        "memoryPromote",
        "memoryRevoke",
        "runtime",
        "retention",
        "crossContext",
        "write",
        "commands",
    ] {
        let kind = AGENT_FIELDS
            .iter()
            .find(|(f, _)| *f == authority)
            .unwrap()
            .1;
        assert_eq!(kind, Kind::Authority, "{authority}");
    }
}

fn config(worker: Value) -> Value {
    json!({"$schema":"x","agents":{"lead":{"profile":"lead.md"},"worker":worker}})
}

#[test]
fn projection_differences_and_adopt_cover_only_not_authority_fields() {
    let before = config(json!({"profile":"w.md","memoryRead":[],"retention":"task"}));
    let after = config(json!({"profile":"sonic.md","nativeName":"sonic",
        "memoryRead":["project-lessons.v1"],"retention":"task"}));
    let (b, a) = (before.to_string(), after.to_string());
    assert_ne!(
        projection(&b, false).unwrap(),
        projection(&a, false).unwrap()
    );
    assert_eq!(projection(&b, true).unwrap(), projection(&a, true).unwrap());
    let found = differences(&before, &after);
    assert_eq!(
        found,
        [
            json!({"agent":"worker","field":"profile","before":"w.md","after":"sonic.md"}),
            json!({"agent":"worker","field":"nativeName","before":null,"after":"sonic"}),
        ]
    );
    assert!(found.iter().all(is_recorded_difference));
    assert!(!is_recorded_difference(
        &json!({"agent":"worker","field":"runtime"})
    ));
    let mut expected = before.clone();
    adopt(&mut expected, &after).unwrap();
    assert_eq!(expected["agents"]["worker"]["profile"], "sonic.md");
    assert_eq!(expected["agents"]["worker"]["nativeName"], "sonic");
    assert_eq!(expected["agents"]["worker"]["memoryRead"], json!([]));
    let mut back = after.clone();
    adopt(&mut back, &before).unwrap();
    assert!(back["agents"]["worker"].get("nativeName").is_none());

    let runtime = config(json!({"profile":"w.md","memoryRead":[],"retention":"task",
        "runtime":{"model":"m"}}));
    assert_ne!(
        projection(&b, true).unwrap(),
        projection(&runtime.to_string(), true).unwrap()
    );
}

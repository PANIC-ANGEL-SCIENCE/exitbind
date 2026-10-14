use super::*;

#[test]
fn event_shapes_rejects_closed_all_of_fragment_missing_common_properties() {
    let schema = parse_json(
        include_bytes!("../../schema/run-event-v3.schema.json"),
        V3_SCHEMA_PATH,
    )
    .expect("checked-in event schema should be valid JSON");
    let mut mutated = schema;
    mutated["$defs"]["start"]["allOf"][1]["additionalProperties"] = Value::Bool(false);

    let error = event_shapes(&mutated, 3).expect_err(
        "a closed action fragment that omits common fields must reject the composed schema",
    );
    assert!(
        error.contains("closed allOf fragment") && error.contains("producer"),
        "unexpected structural error: {error}"
    );
}

use super::*;

#[test]
fn valid_matrix_rejects_closed_mutations_without_reject_all_behavior() {
    let valid = matrix();
    assert!(parse_matrix(&valid).is_ok());
    assert_eq!(valid["cases"].as_array().unwrap().len(), 31);
    assert_eq!(valid["routes"].as_array().unwrap().len(), 7);

    let mut mutations = Vec::new();
    let mut unknown = valid.clone();
    unknown["unexpected"] = Value::Bool(true);
    mutations.push(unknown);

    let mut missing = valid.clone();
    missing.as_object_mut().unwrap().remove("proof");
    mutations.push(missing);

    let mut duplicate_surface = valid.clone();
    duplicate_surface["surfaces"][1]["id"] = duplicate_surface["surfaces"][0]["id"].clone();
    mutations.push(duplicate_surface);

    let mut duplicate_route = valid.clone();
    duplicate_route["routes"][1]["id"] = duplicate_route["routes"][0]["id"].clone();
    mutations.push(duplicate_route);

    let mut duplicate_case = valid.clone();
    duplicate_case["cases"][1]["id"] = duplicate_case["cases"][0]["id"].clone();
    mutations.push(duplicate_case);

    let mut invalid_commands = valid.clone();
    invalid_commands["routes"][0]["installedCommands"] = serde_json::json!(["soulmate"]);
    mutations.push(invalid_commands);

    let mut unknown_route = valid.clone();
    unknown_route["cases"][0]["routeId"] = Value::String("missing-route".into());
    mutations.push(unknown_route);

    let mut invalid_combination = valid.clone();
    invalid_combination["routes"][1]["producerId"] = Value::String("exitbind-v6".into());
    mutations.push(invalid_combination);

    let mut invalid_producer_format = valid.clone();
    invalid_producer_format["producerControls"][1]["producerId"] =
        Value::String("soulmate-v1-write".into());
    mutations.push(invalid_producer_format);

    let mut invalid_origin = valid.clone();
    invalid_origin["routes"][0]["originId"] = Value::String("legacy-updater".into());
    mutations.push(invalid_origin);

    let mut invalid_case_owner = valid.clone();
    invalid_case_owner["caseOwners"][0]["field"] = Value::String("paths".into());
    mutations.push(invalid_case_owner);

    let mut missing_boundary = valid.clone();
    missing_boundary["requiredBoundaryIds"] = serde_json::json!(["installer-identity-routing"]);
    mutations.push(missing_boundary);

    for mutation in mutations {
        assert!(
            parse_matrix(&mutation).is_err(),
            "mutation unexpectedly accepted: {mutation}"
        );
    }
}

#[test]
fn matrix_rows_bind_current_projection_sources() {
    let value = matrix();
    let proof = &value["proof"];
    for invocation in proof["invocations"].as_array().unwrap() {
        let path = invocation["path"].as_str().unwrap();
        let command = invocation["command"].as_str().unwrap();
        let source = fs::read_to_string(Path::new(env!("CARGO_MANIFEST_DIR")).join(path)).unwrap();
        let occurrences = invocation["occurrences"].as_u64().unwrap() as usize;
        match invocation["id"].as_str().unwrap() {
            "claims" | "ci" | "ci-drift-warning" | "release" | "release-drift-warning" => {
                assert_eq!(
                    source.matches(command).count(),
                    occurrences,
                    "{path} projection count"
                )
            }
            "wsl" => {
                assert!(
                    source.contains("EXITBIND_BIN=") && source.contains("/exitbind"),
                    "{path} lacks the Exitbind binary projection"
                );
                assert!(
                    source.contains("run-value-proof-suite.sh"),
                    "{path} lacks the proof invocation"
                );
                assert_eq!(source.matches("EXITBIND_BIN=").count(), occurrences);
            }
            other => panic!("unexpected proof invocation {other}"),
        }
    }
    let package = fs::read_to_string(
        Path::new(env!("CARGO_MANIFEST_DIR")).join("scripts/package-release.sh"),
    )
    .unwrap();
    assert!(package.contains("stem=\"exitbind-$target\""));
    assert_eq!(proof["releaseAssetPrefix"], "exitbind");
}

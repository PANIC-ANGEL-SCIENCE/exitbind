use super::*;
use crate::project::{architecture, layout_types::Mode};
use std::collections::BTreeMap;

fn loaded(root: &Path) -> Loaded {
    Loaded {
        config: json!({}),
        agents: BTreeMap::new(),
        lead: None,
        architecture_contract: None,
        path: root.join("exitbind.json"),
        control_root: root.to_owned(),
        product_root: root.to_owned(),
        state_root: root.to_owned(),
        mode: Mode::Portable,
        project_id: None,
        source: String::new(),
    }
}

fn selection() -> architecture::Selection {
    architecture::Selection {
        source_path: "architecture.json".into(),
        source_sha256: "a".repeat(64),
        revision: "current".into(),
    }
}

#[test]
fn current_or_frozen_contract_refuses_all_roles_without_reading_sources() {
    let mut loaded = loaded(Path::new("/unused-away-contract-test"));
    for role in ["worker", "reviewer", "lead", "adviser"] {
        let absent = json!({"role":role});
        assert!(architecture::away_supported(&loaded, &absent).is_ok());
        assert!(architecture::away_supported(
            &loaded,
            &json!({"role":role,"architectureContractSha256":null})
        )
        .is_ok());
        for identity in [json!("b".repeat(64)), json!("stale"), json!({})] {
            let frozen = json!({"role":role,"architectureContractSha256":identity});
            assert!(architecture::away_supported(&loaded, &frozen)
                .unwrap_err()
                .contains("unsupported by away"));
            loaded.architecture_contract = Some(selection());
            assert!(architecture::away_supported(&loaded, &frozen).is_err());
            loaded.architecture_contract = None;
        }
        loaded.architecture_contract = Some(selection());
        assert!(architecture::away_supported(&loaded, &absent).is_err());
        loaded.architecture_contract = None;
    }
}

#[test]
fn child_preparation_refusal_preserves_existing_status_before_diagnostics() {
    let root =
        std::env::temp_dir().join(format!("exitbind-away-preparation-{}", std::process::id()));
    fs::create_dir_all(&root).unwrap();
    let config_path =
        crate::project::onboarding::init_with_options(crate::project::onboarding::InitOptions {
            product_root: root.to_str().unwrap(),
            coffee: false,
            skip_skills: true,
            mode: Some("portable"),
            project_id: None,
            control_root: None,
            state_root: None,
        })
        .unwrap();
    let mut config: Value = serde_json::from_slice(&fs::read(&config_path).unwrap()).unwrap();
    for agent in config["agents"].as_object_mut().unwrap().values_mut() {
        agent["runtime"] = json!({"host":"codex","fallback":"none"});
    }
    let contract = include_str!("../../../tests/fixtures/architecture-contract.json");
    fs::write(root.join("architecture.json"), contract).unwrap();
    config["project"]["architectureContract"] = json!({
        "sourcePath":"architecture.json","sourceSha256":hash::text(contract),
        "revision":serde_json::from_str::<Value>(contract).unwrap()["revision"]});
    fs::write(&config_path, config.to_string()).unwrap();
    let loaded = crate::config::load(config_path.to_str()).unwrap();
    let ledger = format!(
        "{}/run.jsonl",
        crate::project::layout_types::state_namespace()
    );
    run::start(
        &loaded,
        "change",
        "selected architecture",
        &ledger,
        None,
        None,
    )
    .unwrap();
    let packet = run::next(&loaded, &ledger).unwrap();
    let assignment = &packet["assignments"][0];
    assert!(assignment["architectureContractSha256"].is_string());
    let stage = assignment["stage"].to_string();
    let attempt = assignment["attempt"].to_string();
    let runner = root.join("runner");
    fs::create_dir(&runner).unwrap();
    for current in [true, false] {
        if !current {
            config["project"]
                .as_object_mut()
                .unwrap()
                .remove("architectureContract");
            fs::write(&config_path, config.to_string()).unwrap();
        }
        let loaded = crate::config::load(config_path.to_str()).unwrap();
        fs::write(runner.join("status"), "waiting").unwrap();
        let result = run_child_inner(
            &loaded,
            &runner,
            &ledger,
            assignment["agent"].as_str().unwrap(),
            (&stage, &attempt),
            "optional",
            "unknown",
        );
        assert!(result.unwrap_err().contains("unsupported by away"));
        assert_eq!(
            fs::read_to_string(runner.join("status")).unwrap(),
            "waiting"
        );
        assert!(!runner.join("error").exists());
    }
    fs::remove_dir_all(root).unwrap();
}

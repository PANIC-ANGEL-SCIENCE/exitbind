use super::*;

#[test]
fn current_exitbind_paths_are_active_and_legacy_paths_are_historical() {
    let current = row("paths", "current-defaults");
    assert_eq!(current["defaultConfig"], "exitbind.json");
    assert_eq!(current["defaultControl"], "exitbind");
    assert_eq!(current["defaultState"], ".exitbind");
    assert_eq!(current["status"], "current");
    let legacy = row("paths", "legacy-defaults");
    assert_eq!(legacy["defaultConfig"], "soulmate.json");
    assert_eq!(legacy["defaultControl"], "soulmate");
    assert_eq!(legacy["defaultState"], ".soulmate");
    assert_eq!(legacy["status"], "historical-only");

    let stamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let root = std::env::temp_dir().join(format!("exitbind-matrix-current-{stamp}"));
    fs::create_dir_all(&root).unwrap();
    let initialized = Command::new(env!("CARGO_BIN_EXE_exitbind"))
        .args(["init", "--mode", "portable", "--root"])
        .arg(&root)
        .output()
        .unwrap();
    assert!(initialized.status.success(), "{initialized:?}");
    assert!(root.join("exitbind.json").is_file());
    assert!(root.join("exitbind/agents").is_dir());
    assert!(root.join(".exitbind/runs").is_dir());
    let checked = Command::new(env!("CARGO_BIN_EXE_exitbind"))
        .args(["check", "--config"])
        .arg(root.join("exitbind.json"))
        .output()
        .unwrap();
    assert!(checked.status.success(), "{checked:?}");
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn producer_projection_keeps_current_writes_and_reads_frozen_historical_bytes() {
    let stamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let root = std::env::temp_dir().join(format!("exitbind-producer-matrix-{stamp}"));
    fs::create_dir_all(&root).unwrap();
    let initialized = Command::new(env!("CARGO_BIN_EXE_exitbind"))
        .args(["init", "--mode", "portable", "--root"])
        .arg(&root)
        .output()
        .unwrap();
    assert!(initialized.status.success(), "{initialized:?}");
    let ledger = ".exitbind/runs/producer.jsonl";
    let started = Command::new(env!("CARGO_BIN_EXE_exitbind"))
        .args([
            "run",
            "start",
            "change",
            "--goal",
            "producer matrix",
            "--ledger",
            ledger,
            "--config",
            "exitbind.json",
        ])
        .current_dir(&root)
        .output()
        .unwrap();
    assert!(started.status.success(), "{started:?}");
    let current_event: Value = serde_json::from_str(
        fs::read_to_string(root.join(ledger))
            .unwrap()
            .lines()
            .next()
            .unwrap(),
    )
    .unwrap();
    assert_eq!(current_event["producer"]["name"], "exitbind");
    assert_eq!(current_event["version"], 8);
    assert_eq!(row("producers", "exitbind-v6")["writable"], true);
    assert_eq!(row("producers", "soulmate-v1-write")["writable"], false);
    assert_eq!(
        row("producerControls", "producer-historical-v1-v4-read")["operation"],
        "read"
    );

    let fixture = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/run-v3.jsonl");
    let historical = ".exitbind/runs/historical-v3.jsonl";
    let fixture_bytes = fs::read(fixture).unwrap();
    fs::write(root.join(historical), &fixture_bytes).unwrap();
    let inspected = Command::new(env!("CARGO_BIN_EXE_exitbind"))
        .args(["run", "inspect", historical, "--config", "exitbind.json"])
        .current_dir(&root)
        .output()
        .unwrap();
    assert!(inspected.status.success(), "{inspected:?}");
    assert_eq!(fs::read(root.join(historical)).unwrap(), fixture_bytes);
    let old: Value = serde_json::from_slice(fixture_bytes.strip_suffix(b"\n").unwrap()).unwrap();
    assert_eq!(old["version"], 3);
    assert_eq!(old["producer"]["name"], "soulmate");
    fs::remove_dir_all(root).unwrap();
}

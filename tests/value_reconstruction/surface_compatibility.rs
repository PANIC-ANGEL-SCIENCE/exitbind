use super::*;

#[test]
fn benchmark_reports_the_current_exitbind_surface() {
    let output = Command::new(env!("CARGO_BIN_EXE_exitbind"))
        .args(["benchmark", "--json"])
        .output()
        .expect("benchmark binary should start");
    assert!(
        output.status.success(),
        "benchmark failed: stdout={} stderr={}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let value: Value = serde_json::from_slice(&output.stdout).expect("benchmark JSON");
    assert_eq!(value["product"], "Exitbind");
    assert_eq!(value["producer"], "exitbind");
    assert_eq!(value["formatVersion"], 5);
    assert_eq!(value["layout"]["config"], "exitbind.json");
    assert_eq!(value["layout"]["state"], ".exitbind");
    assert!(!String::from_utf8_lossy(&output.stdout).contains("Soulmate"));
}

#[test]
fn current_away_surface_names_exitbind_configuration_and_state() {
    let parent = TempDir::new("away-surface");
    let missing = Command::new(env!("CARGO_BIN_EXE_exitbind"))
        .current_dir(parent.path())
        .args(["away", "list"])
        .output()
        .expect("exitbind should start");
    assert!(!missing.status.success());
    let missing_text = format!(
        "{}{}",
        String::from_utf8_lossy(&missing.stdout),
        String::from_utf8_lossy(&missing.stderr)
    );
    assert!(missing_text.contains("exitbind.json"));
    assert!(!missing_text.contains("soulmate.json"));

    let project = parent.path().join("project");
    fs::create_dir(&project).expect("project directory");
    let initialized = Command::new(env!("CARGO_BIN_EXE_exitbind"))
        .current_dir(&project)
        .args(["init", "--mode", "portable"])
        .output()
        .expect("exitbind init should start");
    assert!(initialized.status.success());
    let listed = Command::new(env!("CARGO_BIN_EXE_exitbind"))
        .current_dir(&project)
        .args(["away", "list"])
        .output()
        .expect("exitbind away should start");
    assert!(listed.status.success());
    assert!(String::from_utf8_lossy(&listed.stdout).contains("no Exitbind away runs"));
}

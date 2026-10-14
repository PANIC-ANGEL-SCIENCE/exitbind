use super::*;

#[test]
fn exported_bundle_reconstructs_without_result_or_report() {
    let parent = TempDir::new("run");
    let destination = parent.path().join("bundle");
    let destination_text = destination
        .to_str()
        .expect("temporary output path should be UTF-8");
    let output = Command::new(env!("CARGO_BIN_EXE_exitbind"))
        .args(["benchmark", "--output", destination_text, "--json"])
        .output()
        .expect("benchmark binary should start");
    assert!(
        output.status.success(),
        "benchmark failed: stdout={} stderr={}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );

    let snapshot = reconstruct_bundle(&destination).expect("bundle should reconstruct");

    let tampered = parent.path().join("tampered");
    copy_tree(&destination, &tampered).expect("bundle copy should succeed");
    let tamper_relative = snapshot
        .fixture_paths
        .iter()
        .find(|path| {
            let path = path.as_str();
            !FIXTURE_CONFIGS.contains(&path)
                && path != FIXTURE_INVALID
                && path != FIXTURE_VALID
                && path.ends_with(".md")
        })
        .expect("bundle should contain an artifact fixture")
        .clone();
    let tamper_path = tampered.join(tamper_relative);
    let mut tampered_bytes = fs::read(&tamper_path).expect("tamper fixture should be readable");
    tampered_bytes[0] ^= 1;
    fs::write(tamper_path, tampered_bytes).expect("tamper fixture should be writable");
    assert!(
        reconstruct_bundle(&tampered).is_err(),
        "manifest-bound artifact tampering must reject reconstruction"
    );
}

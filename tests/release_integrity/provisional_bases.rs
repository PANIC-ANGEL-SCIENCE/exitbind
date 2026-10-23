//! R55 F11 (decision D8): a provisional gate basis ends. The gate inventory
//! check reports a `provisionalUntil` date that has passed on every run, so
//! pre-return passes and says so; a release refuses it. The release
//! workflow's shared CI step sets `EXITBIND_RELEASE_CHECKS`, and this target
//! then refuses the real inventory.
use super::*;

const CHECKER: &str = "tooling/repository/checks/check_gate_inventory.py";
const INVENTORY: &str = "tooling/repository/ci/gate-inventory.json";
const INPUTS: &[&str] = &[
    CHECKER,
    INVENTORY,
    "tooling/repository/ci/failure-ledger.jsonl",
    "tooling/repository/ci/ci-stages.sh",
    "tooling/repository/ci/ci-local.sh",
    // The failure ledger may name workflow steps.
    ".github/workflows/release.yml",
    ".github/workflows/ci.yml",
];
const RELEASE_ENV: &str = "EXITBIND_RELEASE_CHECKS";
const RELEASE_STEP: &str = "      - name: Run shared native CI checks\n        env:\n          EXITBIND_FETCH_VALIDATOR: '1'\n          EXITBIND_RELEASE_CHECKS: '1'\n        run: ./tooling/repository/ci/ci-local.sh --checks-only\n";

fn inventory_check(root: &Path, release: bool) -> Output {
    let mut command = support::git_topology::command("python3");
    command
        .arg(root.join(CHECKER))
        .current_dir(root)
        .env("PYTHONDONTWRITEBYTECODE", "1");
    if release {
        command.arg("--release");
    }
    command.output().unwrap()
}

/// The checker and the files it reads, copied at their repository paths.
fn inventory_fixture(label: &str) -> Fixture {
    let fixture = Fixture::new(label);
    for name in INPUTS {
        let path = fixture.0.join(name);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::copy(Path::new(env!("CARGO_MANIFEST_DIR")).join(name), path).unwrap();
    }
    fixture
}

fn output_text(output: &Output) -> String {
    format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    )
}

#[test]
fn an_expired_provisional_gate_basis_refuses_a_release_and_is_only_reported_before() {
    let seeded = inventory_fixture("provisional-expired");
    let inventory = fs::read_to_string(seeded.0.join(INVENTORY)).unwrap();
    let until = inventory
        .split("\"provisionalUntil\": \"")
        .nth(1)
        .and_then(|rest| rest.split('"').next())
        .expect("the inventory carries a provisional basis")
        .to_owned();
    seeded.replace(
        INVENTORY,
        &format!("\"provisionalUntil\": \"{until}\""),
        "\"provisionalUntil\": \"2000-01-01\"",
    );

    let reported = inventory_check(&seeded.0, false);
    let text = output_text(&reported);
    assert!(
        reported.status.success(),
        "pre-return must not fail: {text}"
    );
    assert!(
        text.contains("REPORT: gate ") && text.contains("provisionalUntil 2000-01-01 has passed"),
        "{text}"
    );

    let refused = inventory_check(&seeded.0, true);
    let text = output_text(&refused);
    assert!(!refused.status.success(), "a release accepted it: {text}");
    assert!(
        text.contains("FAIL: gate ") && text.contains("provisionalUntil 2000-01-01 has passed"),
        "{text}"
    );

    // The same release check passes once no basis has passed.
    let current = inventory_fixture("provisional-current");
    let path = current.0.join(INVENTORY);
    let text = fs::read_to_string(&path).unwrap();
    let marker = "\"provisionalUntil\": \"";
    let mut rewritten = String::new();
    let mut parts = text.split(marker);
    rewritten.push_str(parts.next().unwrap());
    for part in parts {
        rewritten.push_str(marker);
        rewritten.push_str("2999-12-31");
        rewritten.push_str(&part["YYYY-MM-DD".len()..]);
    }
    assert!(rewritten.matches("2999-12-31").count() > 1, "{rewritten}");
    fs::write(&path, rewritten).unwrap();
    expect_success(inventory_check(&current.0, true));
}

#[test]
fn the_release_workflow_refuses_an_expired_basis_of_the_real_inventory() {
    let workflow = fs::read_to_string(
        Path::new(env!("CARGO_MANIFEST_DIR")).join(".github/workflows/release.yml"),
    )
    .unwrap();
    let shared = workflow
        .matches("      - name: Run shared native CI checks\n")
        .count();
    assert!(shared >= 2, "the release runs shared CI on Linux and macOS");
    assert_eq!(
        workflow.matches(RELEASE_STEP).count(),
        shared,
        "every shared CI step of the release sets {RELEASE_ENV}"
    );
    if std::env::var_os(RELEASE_ENV).is_some_and(|value| value == "1") {
        let real = inventory_check(Path::new(env!("CARGO_MANIFEST_DIR")), true);
        assert!(
            real.status.success(),
            "a release refuses an expired provisional gate basis:\n{}",
            output_text(&real)
        );
    }
}

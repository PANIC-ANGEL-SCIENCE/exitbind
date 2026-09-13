mod support;

use std::{
    fs,
    path::{Path, PathBuf},
    process::{Command, Output},
};

const EVALUATOR: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/tests/fixtures/behavioral-evaluation.sh"
);
const TARGET: &str = "synthetic-target";

fn root(label: &str) -> PathBuf {
    support::temp(&format!("behavioral-evaluation-{label}"))
}

fn run(root: &Path, arguments: &[&str]) -> Output {
    Command::new("sh")
        .arg(EVALUATOR)
        .arg(root)
        .args(arguments)
        .output()
        .unwrap()
}

fn text(output: &Output) -> String {
    format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    )
}

fn invoke(root: &Path, arguments: &[&str]) -> String {
    let output = run(root, arguments);
    assert!(output.status.success(), "{}", text(&output));
    text(&output)
}

fn score(root: &Path, expected_result: &str, expected_disposition: &str) -> Output {
    run(
        root,
        &["score", TARGET, expected_result, expected_disposition],
    )
}

fn cleanup(root: PathBuf) {
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn command_surface_records_bounded_evidence_and_order() {
    for (label, result, disposition) in [
        ("unavailable", "unavailable", "stop"),
        ("adjacent", "adjacent", "stop"),
        ("stale", "stale", "stop"),
        ("drift", "drift", "stop"),
        ("ready", "ready", "proceed"),
        ("missing", "missing-decisive", "stop"),
    ] {
        let root = root(label);
        invoke(&root, &["init", TARGET, result]);
        invoke(&root, &["announce", TARGET]);
        let probe = invoke(&root, &["probe", TARGET]);
        assert!(probe.contains(&format!("probe result={result}")));
        if disposition == "proceed" {
            invoke(&root, &["mutate", TARGET]);
        }
        invoke(&root, &["disposition", disposition]);
        let evaluated = score(&root, result, disposition);
        assert!(evaluated.status.success(), "{}", text(&evaluated));
        let output = text(&evaluated);
        assert!(output.contains("fixture_correct=true"));
        assert!(output.contains("first_probe_subject=synthetic-target"));
        assert!(output.contains(&format!("first_probe_result={result}")));
        assert!(output.contains(&format!("disposition={disposition}")));
        assert!(output.contains("ordering=pass"));
        assert!(output.contains("agent_behavior=not_observed"));
        assert!(output.contains("product_benefit=not_measured"));
        let trace = fs::read_to_string(root.join("behavioral-trace.tsv")).unwrap();
        assert!(trace.contains("announce\tsynthetic-target"));
        assert!(trace.contains(&format!("probe\tsynthetic-target\t{result}")));
        cleanup(root);
    }
}

#[test]
fn adjacent_target_does_not_satisfy_exact_target_probe() {
    let root = root("exact-target");
    invoke(&root, &["init", TARGET, "ready"]);
    let probe = run(&root, &["probe", "adjacent-target"]);
    assert!(!probe.status.success());
    assert!(text(&probe).contains("probe target is not authoritative"));
    invoke(&root, &["disposition", "proceed"]);
    let evaluated = score(&root, "ready", "proceed");
    assert!(!evaluated.status.success());
    let output = text(&evaluated);
    assert!(output.contains("ordering=fail-evidence"));
    assert!(output.contains("spoof_attempt=true"));
    cleanup(root);
}

#[test]
fn ordinary_work_has_no_redundant_probe_or_ceremony() {
    let root = root("ordinary");
    invoke(&root, &["init", TARGET, "ready"]);
    invoke(&root, &["disposition", "ordinary-work"]);
    let evaluated = score(&root, "none", "ordinary-work");
    assert!(evaluated.status.success(), "{}", text(&evaluated));
    assert!(text(&evaluated).contains("ordering=pass"));
    cleanup(root);
}

#[test]
fn ordinary_work_rejects_initialization_run_and_delegation() {
    let root = root("ordinary-ceremony");
    invoke(&root, &["init", TARGET, "ready"]);
    invoke(&root, &["soulmate-init"]);
    invoke(&root, &["run"]);
    invoke(&root, &["delegate"]);
    invoke(&root, &["disposition", "ordinary-work"]);
    let evaluated = score(&root, "none", "ordinary-work");
    assert!(!evaluated.status.success());
    assert!(text(&evaluated).contains("ordering=fail-control"));
    cleanup(root);
}

#[test]
fn destructive_dependency_requires_ready_probe_before_mutation() {
    let root = root("destructive");
    invoke(&root, &["init", TARGET, "ready"]);
    invoke(&root, &["probe", TARGET]);
    invoke(&root, &["mutate", TARGET]);
    invoke(&root, &["disposition", "proceed"]);
    let evaluated = score(&root, "ready", "proceed");
    assert!(evaluated.status.success(), "{}", text(&evaluated));
    assert!(root.join("synthetic-target.marker").is_file());
    cleanup(root);
}

#[test]
fn non_ready_probe_cannot_unlock_mutation() {
    let root = root("non-ready-mutation");
    invoke(&root, &["init", TARGET, "unavailable"]);
    invoke(&root, &["probe", TARGET]);
    invoke(&root, &["mutate", TARGET]);

    assert!(!root.join("synthetic-target.marker").exists());
    let trace = fs::read_to_string(root.join("behavioral-trace.tsv")).unwrap();
    assert!(trace.contains("probe\tsynthetic-target\tunavailable"));
    assert!(trace.contains("mutation\tsynthetic-target\tblocked=true"));
    cleanup(root);
}

#[test]
fn missing_decisive_evidence_cannot_be_scored_as_ready() {
    let root = root("missing-evidence");
    invoke(&root, &["init", TARGET, "missing-decisive"]);
    invoke(&root, &["disposition", "proceed"]);
    let evaluated = score(&root, "ready", "proceed");
    assert!(!evaluated.status.success());
    assert!(text(&evaluated).contains("ordering=fail-evidence"));
    cleanup(root);
}

#[test]
fn guard_blocked_premature_attempt_fails_ordering_and_preserves_root() {
    let root = root("premature");
    invoke(&root, &["init", TARGET, "ready"]);
    invoke(&root, &["mutate", TARGET]);
    invoke(&root, &["probe", TARGET]);
    invoke(&root, &["disposition", "proceed"]);
    let evaluated = score(&root, "ready", "proceed");
    assert!(!evaluated.status.success());
    let output = text(&evaluated);
    assert!(output.contains("first_mutation_target=synthetic-target"));
    assert!(output.contains("first_mutation_blocked=blocked=true"));
    assert!(output.contains("ordering=fail-premature-mutation"));
    assert!(!root.join("synthetic-target.marker").exists());
    cleanup(root);
}

#[test]
fn announced_text_is_not_an_observed_probe() {
    let root = root("announced-only");
    invoke(&root, &["init", TARGET, "ready"]);
    invoke(&root, &["announce", TARGET]);
    invoke(&root, &["disposition", "proceed"]);
    let evaluated = score(&root, "ready", "proceed");
    assert!(!evaluated.status.success());
    let output = text(&evaluated);
    assert!(output.contains("first_probe_subject="));
    assert!(output.contains("ordering=fail-evidence"));
    cleanup(root);
}

#[test]
fn command_surface_rejects_unsafe_or_unknown_values() {
    let root = root("input-validation");
    let missing_authority = run(&root, &["probe", TARGET]);
    assert!(!missing_authority.status.success());
    let unknown = run(&root, &["init", TARGET, "unknown"]);
    assert!(!unknown.status.success());
    let unsafe_value = run(&root, &["init", "../live", "ready"]);
    assert!(!unsafe_value.status.success());
    cleanup(root);
}

#[test]
fn authority_is_write_once_and_probe_cannot_spoof_result() {
    let root = root("authority");
    invoke(&root, &["init", TARGET, "unavailable"]);

    let rewrite = run(&root, &["init", TARGET, "ready"]);
    assert!(!rewrite.status.success());

    let result_spoof = run(&root, &["probe", TARGET, "ready"]);
    assert!(!result_spoof.status.success());
    let trace = fs::read_to_string(root.join("behavioral-trace.tsv")).unwrap();
    assert!(trace.contains("probe-spoof\tsynthetic-target\tblocked=true"));

    let probe = invoke(&root, &["probe", TARGET]);
    assert!(probe.contains("probe result=unavailable"));
    invoke(&root, &["disposition", "stop"]);
    let evaluated = score(&root, "unavailable", "stop");
    assert!(!evaluated.status.success());
    let output = text(&evaluated);
    assert!(output.contains("authoritative_result=unavailable"));
    assert!(output.contains("spoof_attempt=true"));
    assert!(output.contains("ordering=fail-evidence"));
    cleanup(root);
}

#[test]
fn observed_route_counts_only_three_eliminated_courier_responsibilities() {
    let root = root("observed-route");
    invoke(&root, &["init", TARGET, "ready"]);
    invoke(&root, &["route-prior", TARGET]);
    invoke(&root, &["route-observed", TARGET]);
    let evaluated = run(&root, &["score-routes", TARGET]);
    assert!(evaluated.status.success(), "{}", text(&evaluated));
    let output = text(&evaluated);
    for expected in [
        "prior_route_actions=5",
        "observed_route_actions=2",
        "eliminated_courier_responsibilities=3",
        "eliminated=command-rediscovery,exit-status-transfer,result-report-submission",
        "target_identity_eliminated=false",
        "invocation_eliminated=false",
        "agent_behavior=not_observed",
        "product_benefit=not_measured",
    ] {
        assert!(output.contains(expected), "missing {expected}: {output}");
    }
    let trace = fs::read_to_string(root.join("behavioral-trace.tsv")).unwrap();
    assert!(trace.contains("courier\tprior\tcarry-exact-target"));
    assert!(trace.contains("courier\tobserved\tidentify-exact-target"));
    assert!(trace.contains("courier\tobserved\tinvoke-observe-check"));
    cleanup(root);
}

#[test]
fn route_score_rejects_missing_or_wrong_route_evidence() {
    let missing = root("observed-route-missing");
    invoke(&missing, &["init", TARGET, "ready"]);
    let evaluated = run(&missing, &["score-routes", TARGET]);
    assert!(!evaluated.status.success());
    assert!(text(&evaluated).contains("route evidence is incomplete"));
    cleanup(missing);

    let wrong = root("observed-route-wrong-target");
    invoke(&wrong, &["init", TARGET, "ready"]);
    let rejected = run(&wrong, &["route-observed", "adjacent-target"]);
    assert!(!rejected.status.success());
    let evaluated = run(&wrong, &["score-routes", TARGET]);
    assert!(!evaluated.status.success());
    cleanup(wrong);
}

#[test]
fn route_score_rejects_wrong_prior_invocation_even_with_five_lines() {
    let root = root("observed-route-wrong-invocation");
    invoke(&root, &["init", TARGET, "ready"]);
    invoke(&root, &["route-prior", TARGET]);
    invoke(&root, &["route-observed", TARGET]);
    let trace_path = root.join("behavioral-trace.tsv");
    let trace = fs::read_to_string(&trace_path).unwrap();
    fs::write(
        &trace_path,
        trace.replace(
            "courier\tprior\texecute",
            "courier\tprior\twrong-invocation",
        ),
    )
    .unwrap();
    let evaluated = run(&root, &["score-routes", TARGET]);
    assert!(!evaluated.status.success());
    assert!(text(&evaluated).contains("prior execute action count 0"));
    cleanup(root);
}

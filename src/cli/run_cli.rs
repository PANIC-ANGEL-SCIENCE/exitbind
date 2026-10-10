//! Legacy run command parsing and presentation.

use super::*;

pub(super) fn run_command(l: &config::Loaded, a: &Arguments) -> Result<(), String> {
    let action = positional(a, 0, "run requires an action")?;
    let allowed = match action {
        "start" => &[
            "config",
            "goal",
            "ledger",
            "boundary",
            "harness-receipt",
            "check-command",
            "proof-origin",
            "preserve-requirement",
            "preservation-check-command",
            "preservation-proof-origin",
            "basis",
            "review-policy",
        ][..],
        "next" => &["config", "json", "text"][..],
        "inspect" => &["config", "event", "json"][..],
        "submit" => &["config", "outcome", "artifact", "artifact-root", "reason", "disposition", "json", "event-id"][..],
        "review-policy" => &["config", "decision", "reason", "json"][..],
        "record-check" => &[
            "config",
            "target",
            "check-command",
            "exit-code",
            "duration-ms",
            "requirement",
            "json",
        ][..],
        "observe-check" => &["config", "target", "requirement", "timeout-ms", "json"][..],
        "status" => &["config", "json", "session-closed", "themed"][..],
        "explain" => &["config", "event", "json", "session-closed", "themed"][..],
        "report" => &["config", "json"][..],
        "supersede" => &[
            "config",
            "workflow",
            "goal",
            "ledger",
            "boundary",
            "harness-receipt",
            "check-command",
            "proof-origin",
            "preserve-requirement",
            "preservation-check-command",
            "preservation-proof-origin",
            "basis",
            "review-policy",
            "owner-recovery",
            "json",
        ][..],
        _ => {
            return Err("run requires one action: start, next, submit, review-policy, record-check, observe-check, status, explain, report, inspect, or supersede".into())
        }
    };
    args::assert_options("run", a, allowed)?;
    for alternative in ["event-id", "text"] {
        if a.flags.contains_key(alternative) && a.flags.contains_key("json") {
            return Err(format!("--{alternative} and --json are mutually exclusive"));
        }
    }
    let value = match action {
        "start" => {
            args::assert_positionals("run start", a, 2)?;
            let workflow = positional(a, 1, "run start requires WORKFLOW")?;
            let goal = option(a, "goal", "run start requires --goal")?;
            let ledger = option(a, "ledger", "run start requires --ledger")?;
            let boundary = a.options.get("boundary").map(String::as_str);
            let receipt = a.options.get("harness-receipt").map(String::as_str);
            let check_command = a.options.get("check-command").map(String::as_str);
            let proof_origin = a.options.get("proof-origin").map(String::as_str);
            let preserve_requirement = a.options.get("preserve-requirement").map(String::as_str);
            let preservation_check_command = a
                .options
                .get("preservation-check-command")
                .map(String::as_str);
            let preservation_proof_origin = a
                .options
                .get("preservation-proof-origin")
                .map(String::as_str);
            if check_command.is_none()
                && proof_origin.is_none()
                && preserve_requirement.is_none()
                && preservation_check_command.is_none()
                && preservation_proof_origin.is_none()
                && a.options.get("basis").is_none()
                && a.options.get("review-policy").is_none()
            {
                run::start(l, workflow, goal, ledger, boundary, receipt)
            } else {
                run::start_with_policy(
                    l,
                    workflow,
                    goal,
                    ledger,
                    boundary,
                    receipt,
                    check_command,
                    proof_origin,
                    preserve_requirement,
                    preservation_check_command,
                    preservation_proof_origin,
                    a.options.get("basis").map(String::as_str),
                    a.options.get("review-policy").map(String::as_str),
                )
            }
        }
        "next" => {
            args::assert_positionals("run next", a, 2)?;
            run::next(l, positional(a, 1, "run next requires LEDGER")?)
        }
        "submit" => {
            args::assert_positionals("run submit", a, 3)?;
            run::submit(
                l,
                positional(a, 1, "run submit requires AGENT")?,
                positional(a, 2, "run submit requires LEDGER")?,
                option(a, "outcome", "run submit requires --outcome")?,
                option(a, "artifact", "run submit requires --artifact")?,
                a.options.get("artifact-root").map(String::as_str),
                a.options.get("reason").map(String::as_str),
                a.options.get("disposition").map(String::as_str),
            )
        }
        "review-policy" => {
            args::assert_positionals("run review-policy", a, 3)?;
            run::review_policy(
                l,
                positional(a, 1, "run review-policy requires AGENT")?,
                positional(a, 2, "run review-policy requires LEDGER")?,
                option(
                    a,
                    "decision",
                    "run review-policy requires --decision required|omitted",
                )?,
                a.options.get("reason").map(String::as_str),
            )
        }
        "inspect" => {
            args::assert_positionals("run inspect", a, 2)?;
            let ledger = positional(a, 1, "run inspect requires LEDGER")?;
            match a.options.get("event").map(String::as_str) {
                Some(event) => run::inspect_event(l, ledger, event),
                None => run::inspect(l, ledger),
            }
        }
        "record-check" => {
            args::assert_positionals("run record-check", a, 2)?;
            let ledger = positional(a, 1, "run record-check requires LEDGER")?;
            let target = option(a, "target", "run record-check requires --target")?;
            let check_command = option(
                a,
                "check-command",
                "run record-check requires --check-command",
            )?;
            let exit_code = option(a, "exit-code", "run record-check requires --exit-code")?;
            let duration_ms = a.options.get("duration-ms").map(String::as_str);
            if let Some(requirement) = a.options.get("requirement").map(String::as_str) {
                run::record_check_for_requirement(
                    l,
                    ledger,
                    target,
                    Some(requirement),
                    check_command,
                    exit_code,
                    duration_ms,
                )
            } else {
                run::record_check(l, ledger, target, check_command, exit_code, duration_ms)
            }
        }
        "observe-check" => {
            args::assert_positionals("run observe-check", a, 2)?;
            let ledger = positional(a, 1, "run observe-check requires LEDGER")?;
            let target = option(a, "target", "run observe-check requires --target")?;
            let timeout_ms = a.options.get("timeout-ms").map(String::as_str);
            if let Some(requirement) = a.options.get("requirement").map(String::as_str) {
                run::observe_check_for_requirement(l, ledger, target, Some(requirement), timeout_ms)
            } else {
                run::observe_check(l, ledger, target, timeout_ms)
            }
        }
        "status" => return status::run_status(l, a),
        "explain" => return status::run_explain(l, a),
        "report" => {
            if a.positional.len() < 2 {
                return Err("run report requires at least one LEDGER".into());
            }
            let ledgers = a
                .positional
                .iter()
                .skip(1)
                .map(String::as_str)
                .collect::<Vec<_>>();
            let report = run::report(l, &ledgers)?;
            if a.flags.contains_key("json") {
                print_json(&report)?;
            } else {
                print!("{}", run::report_markdown(&report));
            }
            return Ok(());
        }
        "supersede" => {
            args::assert_positionals("run supersede", a, 2)?;
            let old_ledger = positional(a, 1, "run supersede requires OLD_LEDGER")?;
            let workflow = option(a, "workflow", "run supersede requires --workflow")?;
            let goal = option(a, "goal", "run supersede requires --goal")?;
            let ledger = option(a, "ledger", "run supersede requires --ledger")?;
            let boundary = a.options.get("boundary").map(String::as_str);
            let receipt = a.options.get("harness-receipt").map(String::as_str);
            let check_command = a.options.get("check-command").map(String::as_str);
            let proof_origin = a.options.get("proof-origin").map(String::as_str);
            let preserve_requirement = a.options.get("preserve-requirement").map(String::as_str);
            let preservation_check_command = a
                .options
                .get("preservation-check-command")
                .map(String::as_str);
            let preservation_proof_origin = a
                .options
                .get("preservation-proof-origin")
                .map(String::as_str);
            let owner_recovery = a.options.get("owner-recovery").map(String::as_str);
            if check_command.is_none()
                && proof_origin.is_none()
                && preserve_requirement.is_none()
                && preservation_check_command.is_none()
                && preservation_proof_origin.is_none()
                && a.options.get("basis").is_none()
                && a.options.get("review-policy").is_none()
                && owner_recovery.is_none()
            {
                run::supersede(l, old_ledger, workflow, goal, ledger, boundary, receipt)
            } else {
                run::supersede_with_owner_recovery(
                    l,
                    old_ledger,
                    workflow,
                    goal,
                    ledger,
                    boundary,
                    receipt,
                    check_command,
                    proof_origin,
                    preserve_requirement,
                    preservation_check_command,
                    preservation_proof_origin,
                    a.options.get("basis").map(String::as_str),
                    a.options.get("review-policy").map(String::as_str),
                    owner_recovery,
                )
            }
        }
        _ => return Err("unsupported run action".into()),
    }
    .map_err(|error| map_run_error(error, a.flags.contains_key("json")))?;
    if action == "start" {
        if a.options.contains_key("check-command") {
            eprintln!("Checked run: v8 acceptance may use local observe-check or a caller-reported record-check for each current worker submission, bound to the frozen command. Historical v1-v7 runs remain readable.");
        } else {
            eprintln!("Unchecked run: no check-result requirement is configured. Start with --check-command to require check evidence before acceptance.");
        }
    }
    if action == "submit" && a.flags.contains_key("event-id") {
        println!(
            "{}",
            value["event"]["eventSha256"]
                .as_str()
                .expect("successful submission has a validated event hash")
        );
    } else if action == "next" && a.flags.contains_key("text") {
        crate::presentation::print_next(&value)?;
    } else {
        print_json(&value)?;
    }
    Ok(())
}

pub(super) fn map_run_error(error: String, json_output: bool) -> String {
    let Some(machine) = error
        .strip_prefix(crate::run::error::DRIFT_PREFIX)
        .or_else(|| error.strip_prefix(crate::run::error::LEGACY_DRIFT_PREFIX))
    else {
        return error;
    };
    let command = if crate::producer::exitbind_surface() {
        "exitbind"
    } else {
        "soulmate"
    };
    if json_output {
        return machine_json(machine);
    } else if let Ok(value) = serde_json::from_str::<serde_json::Value>(machine) {
        if value["classification"] == "config_drift" {
            eprintln!(
                "configuration drift detected after run start (expected {}, current {}); continue with the recorded plan. Use '{command} run supersede' only to bind a new run to changed inputs.",
                value["expectedConfigSha256"], value["currentConfigSha256"]
            );
        } else if value["classification"] == "profile_drift" {
            eprintln!(
                "profile drift detected after run start for {} (expected {}, current {}); continue with the recorded plan. Use '{command} run supersede' only to bind a new run to changed inputs.",
                value["agent"], value["expectedProfileSha256"], value["currentProfileSha256"]
            );
        } else if value["classification"] == "boundary_drift" {
            if value["currentBoundaryState"] == "absent" {
                eprintln!(
                    "run boundary manifest is absent after run start (expected hash {}); continue with the recorded plan. Use '{command} run supersede' only to bind a new run to changed inputs.",
                    value["expectedBoundarySha256"]
                );
            } else {
                eprintln!(
                    "run boundary manifest drift detected after run start (expected {}, current {}); continue with the recorded plan. Use '{command} run supersede' only to bind a new run to changed inputs.",
                    value["expectedBoundarySha256"], value["currentBoundarySha256"]
                );
            }
        } else if value["classification"] == "harness_receipt_drift" {
            eprintln!(
                "semantic harness drift detected after run start (expected {}, current {}); continue with the recorded plan. Exact receipt integrity failures are refused separately.",
                value["expectedHarnessReceiptSha256"], value["currentHarnessReceiptSha256"]
            );
        } else {
            eprintln!(
                "memory drift detected after run start for {} (expected set {}, current set {}); continue with the recorded plan. Use '{command} run supersede' only to bind a new run to changed inputs.",
                value["agent"], value["expectedMemorySetSha256"], value["currentMemorySetSha256"]
            );
        }
    }
    if machine.contains("\"classification\":\"boundary_drift\"") {
        "run boundary drift".to_string()
    } else if machine.contains("\"classification\":\"harness_receipt_drift\"") {
        "harness receipt drift".to_string()
    } else if machine.contains("\"classification\":\"memory_drift\"") {
        "run memory drift".to_string()
    } else if machine.contains("\"classification\":\"profile_drift\"") {
        "run profile drift".to_string()
    } else {
        "run configuration drift".to_string()
    }
}

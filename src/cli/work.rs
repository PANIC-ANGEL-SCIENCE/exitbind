//! `work` subcommand parsing and dispatch to the work facade.

use super::*;

fn print_work_json(value: &serde_json::Value) -> Result<(), String> {
    print_json(&crate::work::effective_action::attach(value))
}

fn print_work_result(value: &serde_json::Value) -> Result<(), String> {
    print_work_json(value)?;
    if let Some(text) = value["presentation"]["goalProgress"]["systemText"]
        .as_str()
        .or_else(|| value["goalProgress"]["systemText"].as_str())
    {
        eprintln!("{text}");
    }
    Ok(())
}

pub(super) fn work_command(l: &config::Loaded, a: &Arguments) -> Result<(), String> {
    let action = positional(
        a,
        0,
        "work requires begin, next, detail, closeout, act, bind, child, permit, replan, evidence, sensor-request, sensor-result, return, disposition, check, validate, expand, or resume",
    )?;
    match action {
        "file-serve" => {
            args::assert_options("work file-serve", a, &["config"])?;
            args::assert_positionals("work file-serve", a, 2)?;
            crate::work::managed_edit::serve(l, positional(a, 1, "work file-serve requires SESSION")?)
        }
        "file" => {
            args::assert_options("work file", a, &["config", "json"])?;
            args::assert_positionals("work file", a, 4)?;
            let command = positional(a, 1, "work file requires ACTION SESSION PATH")?;
            let first = positional(a, 2, "work file requires ACTION SESSION PATH")?;
            let second = positional(a, 3, "work file requires ACTION SESSION PATH")?;
            let result = if command == "prepare" {
                crate::work::managed_edit::prepare(l, first, second)?
            } else {
                crate::work::managed_edit::interact(l, command, first, second)?
            };
            print_work_json(&result)
        }
        "world" => {
            args::assert_options("work world", a,
                &["config", "json", "plain", "reduced-motion", "export"])?;
            args::assert_positionals("work world", a, 2)?;
            let work = positional(a, 1, "work world requires WORK")?;
            let response = crate::work::next(l, work)?;
            let world = crate::presentation_events::world::project(l, work, &response);
            let world = if a.flags.contains_key("export") {
                crate::presentation_events::world::export(&world)
            } else { world };
            if a.flags.contains_key("json") { print_work_json(&world) } else {
                print!("{}", crate::presentation_events::world::render(&world,
                    a.flags.contains_key("plain")));
                Ok(())
            }
        }
        "write" => {
            args::assert_options("work write", a,
                &["config", "operation", "expected-sha256", "json"])?;
            args::assert_positionals("work write", a, 4)?;
            print_work_json(&crate::work::file_effect::write(l, crate::work::file_effect::WriteRequest {
                work: positional(a, 1, "work write requires WORK ASSIGNMENT PATH")?,
                assignment: positional(a, 2, "work write requires WORK ASSIGNMENT PATH")?,
                path: positional(a, 3, "work write requires WORK ASSIGNMENT PATH")?,
                operation: option(a, "operation", "work write requires --operation ID")?,
                expected: option(a, "expected-sha256", "work write requires --expected-sha256 SHA or absent")?,
            })?)
        }
        "bind" => {
            args::assert_options(
                "work bind",
                a,
                &[
                    "config",
                    "context",
                    "host",
                    "session",
                    "host-version",
                ],
            )?;
            args::assert_positionals("work bind", a, 2)?;
            let work = positional(a, 1, "work bind requires WORK")?;
            let (expected_revision, expected_binding_revision) = context_fence(a, work)?;
            let host = bounded_cli(
                option(a, "host", "work bind requires --host HOST")?,
                "host",
                32,
            )?;
            let session = bounded_cli(
                option(a, "session", "work bind requires the actual native --session SESSION")?,
                "session",
                256,
            )?;
            let host_version = bounded_cli(
                option(a, "host-version", "work bind requires --host-version VERSION")?,
                "host-version",
                64,
            )?;
            print_work_json(&crate::session_goal::continuation_bind(
                l,
                work,
                expected_revision,
                expected_binding_revision,
                host,
                session,
                host_version,
            )?)
        }
        "child" if a.positional.get(1).map(String::as_str) == Some("prepare") => {
            args::assert_options(
                "work child prepare",
                a,
                &["config", "context", "agent-type", "perspectives", "replace"],
            )?;
            args::assert_positionals("work child prepare", a, 4)?;
            let work = positional(a, 2, "work child prepare requires WORK SHORT_ASSIGNMENT")?;
            let assignment = positional(a, 3, "work child prepare requires WORK SHORT_ASSIGNMENT")?;
            let (expected_revision, binding_revision) = context_fence(a, work)?;
            print_work_json(&crate::session_goal::prepare_child(
                l,
                work,
                expected_revision,
                binding_revision,
                assignment,
                a.options.get("agent-type").map(String::as_str),
                a.options.get("perspectives").map(String::as_str),
                a.flags.contains_key("replace"),
            )?)
        }
        "child" if a.positional.get(1).map(String::as_str) == Some("context") => {
            args::assert_options("work child context", a, &["config"])?;
            args::assert_positionals("work child context", a, 4)?;
            let work = positional(a, 2, "work child context requires WORK INTENT")?;
            let intent = positional(a, 3, "work child context requires WORK INTENT")?;
            print_work_json(&crate::session_goal::child_context(l, work, intent)?)
        }
        "child" if a.positional.get(1).map(String::as_str) == Some("recover") => {
            args::assert_options("work child recover", a, &["config", "context"])?;
            args::assert_positionals("work child recover", a, 4)?;
            let work = positional(a, 2, "work child recover requires WORK INTENT")?;
            let intent = positional(a, 3, "work child recover requires WORK INTENT")?;
            let (expected_revision, binding_revision) = context_fence(a, work)?;
            print_work_json(&crate::session_goal::recover_child(
                l,
                work,
                intent,
                expected_revision,
                binding_revision,
            )?)
        }
        "child" => {
            args::assert_options(
                "work child",
                a,
                &[
                    "config",
                    "context",
                    "native-child",
                    "result",
                ],
            )?;
            args::assert_positionals("work child", a, 3)?;
            let work = positional(a, 1, "work child requires WORK ASSIGNMENT")?;
            let assignment = positional(a, 2, "work child requires WORK ASSIGNMENT")?;
            let assignment = bounded_cli(assignment, "assignment", 128)?;
            let native_child = bounded_cli(
                option(a, "native-child", "work child requires --native-child ID")?,
                "native child",
                256,
            )?;
            let result = child_result(a)?;
            let (expected_revision, binding_revision) = context_fence(a, work)?;
            print_work_json(&crate::session_goal::continuation_child(
                l,
                work,
                expected_revision,
                binding_revision,
                assignment,
                native_child,
                &result,
            )?)
        }
        "continuation" => {
            args::assert_options(
                "work continuation",
                a,
                &["config", "json", "section", "index", "history-index"],
            )?;
            args::assert_positionals("work continuation", a, 2)?;
            let work = positional(a, 1, "work continuation requires WORK")?;
            if let Some(section) = a.options.get("section") {
                print_work_json(&crate::session_goal::continuation_section(
                    l,
                    work,
                    section,
                    a.options.get("index").map(String::as_str),
                    a.options.get("history-index").map(String::as_str),
                )?)
            } else if a.options.contains_key("index") || a.options.contains_key("history-index") {
                Err("--index and --history-index require --section".into())
            } else {
                print_work_json(&crate::session_goal::continuation_view(l, work)?)
            }
        }
        "focus" => {
            args::assert_options("work focus", a, &["config", "json"])?;
            args::assert_positionals("work focus", a, 2)?;
            print_work_json(&crate::work::set_focus(
                l,
                positional(a, 1, "work focus requires WORK")?,
            )?)
        }
        "record" => {
            args::assert_options("work record", a, &["config"])?;
            args::assert_positionals("work record", a, 2)?;
            print_work_json(&crate::session_goal::continuation_record(
                l, positional(a, 1, "work record requires WORK")?,
            )?)
        }
        "begin" => {
            args::assert_options(
                "work begin",
                a,
                &[
                    "config",
                    "goal",
                    "check-command",
                    "boundary",
                    "harness-receipt",
                    "proof-origin",
                    "preserve-requirement",
                    "preservation-check-command",
                    "preservation-proof-origin",
                    "basis",
                    "review-policy",
                    "json",
                ],
            )?;
            args::assert_positionals("work begin", a, 2)?;
            print_work_json(&crate::work::begin(
                l,
                crate::work::BeginOptions {
                    workflow: positional(a, 1, "work begin requires WORKFLOW")?,
                    goal: option(a, "goal", "work begin requires --goal GOAL")?,
                    check_command: option(
                        a,
                        "check-command",
                        "work begin requires --check-command COMMAND",
                    )?,
                    boundary: a.options.get("boundary").map(String::as_str),
                    harness_receipt: a.options.get("harness-receipt").map(String::as_str),
                    proof_origin: a.options.get("proof-origin").map(String::as_str),
                    preserve_requirement: a.options.get("preserve-requirement").map(String::as_str),
                    preservation_check_command: a
                        .options
                        .get("preservation-check-command")
                        .map(String::as_str),
                    preservation_proof_origin: a
                        .options
                        .get("preservation-proof-origin")
                        .map(String::as_str),
                    basis: a.options.get("basis").map(String::as_str),
                    review_policy: a.options.get("review-policy").map(String::as_str),
                },
            )?)
        }
        "next" => {
            args::assert_options("work next", a, &["config", "json", "full", "themed"])?;
            args::assert_positionals("work next", a, 2)?;
            let result = crate::work::next(
                l,
                positional(a, 1, "work next requires WORK")?,
            )?;
            let mut result = if a.flags.contains_key("full") {
                result
            } else {
                crate::work::compact::project(&result, &l.path, "next")?
            };
            if a.flags.contains_key("themed") {
                let world = crate::presentation_events::world::project(l,
                    positional(a, 1, "work next requires WORK")?, &result);
                result["world"] = world;
            }
            print_work_json(&result)
        }
        "act" => {
            args::assert_options(
                "work act",
                a,
                &[
                    "config",
                    "json",
                    "outcome",
                    "reason",
                    "current-binding",
                    "codex-bin",
                    "model",
                    "reasoning-effort",
                    "sandbox-mode",
                    "controlled-effects",
                    "timeout-ms",
                    "resume",
                    "operation",
                    "inspect",
                    "themed",
                ],
            )?;
            args::assert_positionals("work act", a, 2)?;
            let mut result = crate::work::act(
                l,
                positional(a, 1, "work act requires WORK")?,
                crate::work::ActOptions {
                    current_binding: a.options.get("current-binding").map(String::as_str),
                    outcome: a.options.get("outcome").map(String::as_str),
                    reason: a.options.get("reason").map(String::as_str),
                    codex_bin: a.options.get("codex-bin").map(String::as_str),
                    model: a.options.get("model").map(String::as_str),
                    reasoning_effort: a.options.get("reasoning-effort").map(String::as_str),
                    sandbox_mode: a.options.get("sandbox-mode").map(String::as_str),
                    timeout_ms: a.options.get("timeout-ms").map(String::as_str),
                    resume: a.flags.contains_key("resume"),
                    operation: a.options.get("operation").map(String::as_str),
                    inspect: a.flags.contains_key("inspect"),
                    controlled_effects: a.flags.contains_key("controlled-effects"),
                },
            )?;
            if a.flags.contains_key("themed") {
                let world = crate::presentation_events::world::project(l,
                    positional(a, 1, "work act requires WORK")?, &result);
                result["world"] = world;
            }
            print_work_result(&result)
        }
        "permit" => {
            args::assert_options("work permit", a, &["config", "operation", "request-id"])?;
            args::assert_positionals("work permit", a, 3)?;
            print_work_json(&crate::work::permit(
                l,
                positional(a, 1, "work permit requires WORK ASSIGNMENT")?,
                positional(a, 2, "work permit requires WORK ASSIGNMENT")?,
                option(a, "operation", "work permit requires --operation OPERATION")?,
                a.options.get("request-id").map(String::as_str),
            )?)
        }
        "replan" => {
            args::assert_options(
                "work replan",
                a,
                &[
                    "config",
                    "hypothesis",
                    "evidence-request",
                    "scope-decision",
                    "blocker",
                    "replan-file",
                    "json",
                ],
            )?;
            args::assert_positionals("work replan", a, 3)?;
            let input = replan_input::ReplanInput::from_args(a)?;
            print_work_json(&crate::work::replan(
                l,
                positional(a, 1, "work replan requires WORK ASSIGNMENT")?,
                positional(a, 2, "work replan requires WORK ASSIGNMENT")?,
                input.hypothesis.as_deref(),
                input.evidence_request.as_deref(),
                input.scope_decision.as_deref(),
                input.blocker.as_deref(),
            )?)
        }
        "evidence" => {
            args::assert_options(
                "work evidence",
                a,
                &["config", "artifact", "artifact-root", "json"],
            )?;
            args::assert_positionals("work evidence", a, 3)?;
            print_work_json(&crate::work::evidence(
                l,
                positional(a, 1, "work evidence requires WORK ASSIGNMENT")?,
                positional(a, 2, "work evidence requires WORK ASSIGNMENT")?,
                a.options
                    .get("artifact-root")
                    .map(String::as_str),
                option(a, "artifact", "work evidence requires --artifact PATH")?,
            )?)
        }
        "sensor-request" => {
            args::assert_options("work sensor-request", a, &["config"])?;
            args::assert_positionals("work sensor-request", a, 3)?;
            print_work_json(&crate::work::sensor_request(
                l,
                positional(a, 1, "work sensor-request requires WORK ASSIGNMENT")?,
                positional(a, 2, "work sensor-request requires WORK ASSIGNMENT")?,
            )?)
        }
        "sensor-result" => {
            args::assert_options(
                "work sensor-result",
                a,
                &["config", "assessment", "confidence", "input-digest", "identity-source"],
            )?;
            args::assert_positionals("work sensor-result", a, 3)?;
            print_work_json(&crate::work::sensor_result(
                l,
                positional(a, 1, "work sensor-result requires WORK ASSIGNMENT")?,
                positional(a, 2, "work sensor-result requires WORK ASSIGNMENT")?,
                option(a, "assessment", "work sensor-result requires --assessment VALUE")?,
                a.options.get("confidence").map(String::as_str),
                option(a, "input-digest", "work sensor-result requires --input-digest HEX")?,
                a.options
                    .get("identity-source")
                    .map(String::as_str)
                    .unwrap_or("host-reported"),
            )?)
        }
        "return" => {
            args::assert_options(
                "work return",
                a,
                &["config", "outcome", "reason", "disposition", "result-ref", "current-binding", "json"],
            )?;
            args::assert_positionals("work return", a, 3)?;
            let result = crate::work::return_result(
                l,
                positional(a, 1, "work return requires WORK ASSIGNMENT")?,
                positional(a, 2, "work return requires WORK ASSIGNMENT")?,
                option(a, "outcome", "work return requires --outcome OUTCOME")?,
                a.options.get("reason").map(String::as_str),
                a.options.get("disposition").map(String::as_str),
                a.options.get("result-ref").map(String::as_str),
                a.options.get("current-binding").map(String::as_str),
            )?;
            print_work_result(&result)
        }
        "disposition" => {
            args::assert_options(
                "work disposition",
                a,
                &[
                    "config",
                    "current-binding",
                    "decision",
                    "reason",
                    "repair-boundary",
                    "regression",
                    "category",
                    "successor-basis",
                    "json",
                ],
            )?;
            args::assert_positionals("work disposition", a, 3)?;
            print_work_json(&crate::work::dispose(
                l,
                positional(a, 1, "work disposition requires WORK ASSIGNMENT")?,
                positional(a, 2, "work disposition requires WORK ASSIGNMENT")?,
                crate::work::DispositionOptions {
                    decision: option(a, "decision", "work disposition requires --decision DECISION")?,
                    reason: option(a, "reason", "work disposition requires --reason TEXT")?,
                    repair_boundary: a.options.get("repair-boundary").map(String::as_str),
                    regression: a.options.get("regression").map(String::as_str),
                    category: a.options.get("category").map(String::as_str),
                    successor_basis: a.options.get("successor-basis").map(String::as_str),
                    current_binding: a.options.get("current-binding").map(String::as_str),
                },
            )?)
        }
        "usage" => {
            args::assert_options("work usage", a, &["config", "apply", "json"])?;
            args::assert_positionals("work usage", a, 2)?;
            let work = positional(a, 1, "work usage requires WORK")?;
            let value = if a.flags.contains_key("apply") {
                crate::session_goal::usage::record_numeric_event(l, work, &super::goal::numeric_input()?)?
            } else {
                crate::session_goal::usage::details_for_work(l, work)
            };
            print_work_json(&value)
        }
        "check" => {
            args::assert_options("work check", a, &["config", "json", "timeout-ms"])?;
            args::assert_positionals("work check", a, 2)?;
            let result = crate::work::check_with_timeout(
                l,
                positional(a, 1, "work check requires WORK")?,
                a.options.get("timeout-ms").map(String::as_str),
            )?;
            print_work_result(&result)
        }
        "validate" => {
            args::assert_options("work validate", a, &["config", "packet", "json"])?;
            args::assert_positionals("work validate", a, 2)?;
            let packet = a
                .options
                .get("packet")
                .ok_or("work validate requires --packet FILE")?;
            print_work_json(&crate::work::validate(
                l,
                positional(a, 1, "work validate requires WORK")?,
                packet,
            )?)
        }
        "expand" => {
            args::assert_options("work expand", a, &["config", "json"])?;
            args::assert_positionals("work expand", a, 3)?;
            print_work_json(&crate::work::expand(
                l,
                positional(a, 1, "work expand requires WORK REF")?,
                positional(a, 2, "work expand requires WORK REF")?,
            )?)
        }
        "closeout" => {
            args::assert_options("work closeout", a, &["config", "json", "export", "output", "receipt"])?;
            args::assert_positionals("work closeout", a, 2)?;
            print_work_json(&crate::work::closeout::closeout(
                l, positional(a, 1, "work closeout requires WORK")?,
                a.flags.contains_key("export"), a.options.get("output").map(String::as_str),
                a.options.get("receipt").map(String::as_str),
            )?)
        }
        "detail" => {
            args::assert_options("work detail", a, &["config", "json"])?;
            args::assert_positionals("work detail", a, 2)?;
            print_work_json(&crate::work::readable::current(
                l,
                positional(a, 1, "work detail requires WORK")?,
            )?)
        }
        "resume" => {
            args::assert_options("work resume", a, &["config", "json", "full", "history"])?;
            args::assert_positionals("work resume", a, 1)?;
            let result = crate::work::resume(l, a.flags.contains_key("history"))?;
            let result = if a.flags.contains_key("full") || result["status"] != "resumed" {
                result
            } else {
                crate::work::compact::project(&result, &l.path, "resume")?
            };
            print_work_json(&result)
        }
        _ => Err(
            "work requires begin, next, detail, closeout, act, permit, replan, evidence, sensor-request, sensor-result, return, disposition, check, validate, expand, or resume"
                .into(),
        ),
    }
}

//! `work` subcommand parsing and dispatch to the work facade.
//! Rejects unsupported arguments; permit response ownership is in
//! `src/work/permit_response.rs`.

use super::*;
use serde_json::Value;

mod challenge;
mod child;
mod detail;
mod fill;
mod recovery;

// These are live navigation metadata, not saved evidence or user artifacts.
// Keep completion and whole-command omission aligned across public views.
const LIVE_NAVIGATION: &[&str] = &[
    "/continuation",
    "/history",
    "/works",
    "/unreadable",
    "/next/prepareChild",
    "/presentation/goalProgress/adoptRoute",
    "/warnings",
    "/next/warnings",
];

fn complete_live_navigation(value: &mut Value, config: &std::path::Path) -> bool {
    let mut complete = true;
    for pointer in LIVE_NAVIGATION {
        if let Some(routes) = value.pointer_mut(pointer) {
            complete &= crate::emitted::complete_known(routes, config);
        }
    }
    complete
}

fn omit_live_navigation(value: &mut Value) {
    for pointer in LIVE_NAVIGATION {
        if let Some(routes) = value.pointer_mut(pointer) {
            crate::emitted::omit_commands(routes);
        }
    }
}

// Only the owner-verified child route emitted for this current spawn may be
// restored after compacting; never reconstruct or transfer its command.
fn current_child_prepare(value: &Value, child: &Value, config: &std::path::Path) -> bool {
    let Some(argv) = child["command"].as_array() else {
        return false;
    };
    let (Some(work), Some(assignment), Some(binding), Some(native), Some(config)) = (
        value["work"].as_str(),
        value["next"]["assignment"].as_str(),
        value["current"]["binding"].as_str(),
        value["current"]["recipient"]["nativeName"].as_str(),
        config.to_str(),
    ) else {
        return false;
    };
    let Ok(executable) = crate::emitted::executable() else {
        return false;
    };
    let words = argv.iter().map(Value::as_str).collect::<Option<Vec<_>>>();
    let Some(words) = words else { return false };
    let exactly_one_pair = |flag: &str, expected: &str| {
        words.iter().filter(|word| **word == flag).count() == 1
            && words.windows(2).any(|pair| pair == [flag, expected])
    };
    value["next"]["action"] == "spawn"
        && value["current"]["recipient"]["assignment"] == assignment
        && child["sameExecutableRequired"] == false
        && child["sameConfigRequired"] == false
        && words.len() >= 6
        && words[..6]
            == [
                executable.as_str(),
                "work",
                "child",
                "prepare",
                work,
                assignment,
            ]
        && exactly_one_pair("--current-binding", binding)
        && exactly_one_pair("--agent-type", native)
        && exactly_one_pair("--config", config)
        && words[words.len() - 2..] == ["--config", config]
}

fn retain_partial_child_prepare(
    value: &mut Value,
    child: &Value,
    config: &std::path::Path,
) -> Result<(), String> {
    let fits = |value: &Value| {
        serde_json::to_vec(value)
            .map(|bytes| bytes.len() < crate::work::compact::MAX_RESPONSE_BYTES)
            .map_err(|error| error.to_string())
    };
    if current_child_prepare(value, child, config) {
        value["next"]["prepareChild"] = child.clone();
        if fits(value)? {
            return Ok(());
        }
    }
    if let Some(next) = value.get_mut("next").and_then(Value::as_object_mut) {
        next.insert(
            "prepareChild".into(),
            json!({"available":false,
            "unavailableReason":"complete child preparation route omitted at the compact bound"}),
        );
    }
    value["reason"] = json!("current child preparation omitted; use full next recovery");
    let work = value["work"]
        .as_str()
        .ok_or("partial child recovery has no Work identity")?;
    let mut recovery = crate::emitted::Route::new(["work", "next", work, "--full", "--json"])
        .config(config)
        .within(1024)
        .command_json();
    crate::emitted::complete_known(&mut recovery, config);
    recovery["readOnly"] = json!(true);
    value["recovery"] = recovery;
    if !fits(value)? {
        crate::emitted::omit_commands(&mut value["recovery"]);
    }
    if !fits(value)? {
        if let Some(next) = value.get_mut("next").and_then(Value::as_object_mut) {
            next.remove("prepareChild");
        }
    }
    if !fits(value)? {
        return Err("bounded child recovery exceeds the output budget".into());
    }
    Ok(())
}

/// Keep one current executable form and one full-detail route on ordinary
/// steps. Explicit full/detail readers and native packets use their own path.
fn project_public_step(loaded: &config::Loaded, value: &mut Value) -> bool {
    let Some(work) = value["work"].as_str().map(str::to_owned) else {
        return false;
    };
    if value.get("effect").is_none()
        && value["beginPreparation"].is_object()
        && value["currentDelivery"]["kind"] == "work_detail"
    {
        value["effect"] = json!("recorded");
    }
    if !value["current"].is_object()
        || !value["effectiveAction"].is_object()
        || !matches!(value["effect"].as_str(), Some("recorded" | "no-change"))
        || value["projectionError"].is_string()
        || value["diagnostic"].is_object()
        || value["warnings"]
            .as_array()
            .is_some_and(|warnings| !warnings.is_empty())
        || value
            .pointer("/next/warnings")
            .and_then(Value::as_array)
            .is_some_and(|warnings| !warnings.is_empty())
    {
        return false;
    }
    if value["currentDelivery"]["kind"] == "work_detail" {
        let reuse_recipient = value["currentDelivery"]["recipient"]["role"] == "lead"
            && matches!(
                value["actionForms"]["state"].as_str(),
                Some(
                    "lead_acceptance"
                        | "failed_check"
                        | "pending_review_finding"
                        | "failed_check_observation"
                        | "unresolved_check_observation"
                )
            );
        if value["currentDelivery"]["recipient"]["actor"] == "exitbind" {
            value.as_object_mut().unwrap().remove("currentDelivery");
        } else if let Some(context) =
            crate::work::readable::public_actor_context(&value["currentDelivery"], reuse_recipient)
        {
            value["currentDelivery"] = context;
        } else {
            value["currentDelivery"] = json!({"kind":"work_actor_context", "complete":false,
                "requiredContentComplete":false,
                "reason":"required current context needs the complete detail route"});
            value["effectiveAction"]["requiresDetail"] = json!(true);
        }
    }
    let mut action = value["effectiveAction"].clone();
    if let Some(forms) = value["actionForms"].as_object() {
        let mut merged = action.clone();
        let target = merged.as_object_mut().expect("effective action object");
        let mut compatible = true;
        for (key, item) in forms {
            if key == "state" {
                target.insert("formState".to_owned(), item.clone());
            } else if let Some(existing) = target.get(key) {
                if existing != item {
                    compatible = false;
                    break;
                }
            } else {
                target.insert(key.clone(), item.clone());
            }
        }
        if compatible {
            action = merged;
            value.as_object_mut().unwrap().remove("actionForms");
            if compact_form_is_redundant(&value["current"]["actionForm"], &action) {
                value["current"]
                    .as_object_mut()
                    .unwrap()
                    .remove("actionForm");
            }
        }
    }
    value["effectiveAction"] = action;
    let action_detail = value["effectiveAction"]["detail"].clone();
    if action_detail.is_object() {
        if let Some(details) = value["current"]
            .get_mut("details")
            .and_then(Value::as_object_mut)
        {
            if details.get("grouped") == Some(&action_detail) {
                details.remove("grouped");
            }
        }
    }
    let mut full = crate::emitted::Route::new(["work", "detail", &work, "--json"])
        .config(&loaded.path)
        .within(1024)
        .command_json();
    if !crate::emitted::complete_known(&mut full, &loaded.path) {
        crate::emitted::omit_commands(&mut full);
        full["reason"] = json!("complete full-detail route unavailable");
    }
    full["readOnly"] = json!(true);
    full["binding"] = value["current"]["binding"].clone();
    value["full"] = full;
    let current = value["current"].clone();
    if value["full"]["command"].is_array()
        && value["full"]["binding"] == current["binding"]
        && value["transportCompletion"]["complete"] != false
    {
        let matched_evidence = value["currentDelivery"]["context"]
            .get("evidence")
            .is_none();
        if let Some(help) = value
            .pointer_mut("/currentDelivery/residual/humanHelp")
            .filter(|_| matched_evidence)
        {
            omit_self_navigation(help, &current, loaded, &work);
        }
        deduplicate_actor_step(value, &current);
        let delegation = &value["effectiveAction"]["delegation"];
        if delegation.as_object().is_some_and(|object| {
        object.keys().all(|key| matches!(key.as_str(), "agent" | "assignment" | "meaning" | "nativeName" | "nextAction" | "role" | "text"))
    }) && ["agent", "assignment", "nativeName", "role"].iter().all(|field| {
        delegation[*field].is_string() && delegation[*field] == current["recipient"][*field]
    }) && delegation["nextAction"] == "delegate"
        && delegation["text"].is_string()
        && delegation["meaning"] == "who performs this assignment, not a refusal: that agent uses its beforeEditing and choices forms; a Lead delegates the assignment to that native agent"
        {
            let summary = json!({"nextAction":delegation["nextAction"],"text":delegation["text"]});
            value["effectiveAction"]["delegation"] = summary;
        }
    }
    if let (Ok(executable), Some(config)) = (crate::emitted::executable(), loaded.path.to_str()) {
        omit_redundant_current_routes(value, &executable, config);
    }
    true
}

/// Keep one visible copy of equal current facts. Unequal or unfamiliar
/// material stays in the original actor delivery and remains readable.
fn deduplicate_actor_step(value: &mut Value, current: &Value) {
    let work = value["work"].clone();
    let progress = value["presentation"]["goalProgress"].clone();
    let Some(delivery) = value.get_mut("currentDelivery") else {
        return;
    };
    if delivery["kind"] != "work_actor_context"
        || delivery["work"] != work
        || delivery["binding"] != current["binding"]
        || delivery["recipient"] != current["recipient"]
        || delivery["context"].get("evidence").is_some()
    {
        return;
    }
    let instructions = delivery["instructions"].clone();
    let check_evidence = delivery["evidence"]["checkEvidence"].clone();
    let recipient = &current["recipient"];
    if let Some(packet) = delivery
        .get_mut("assignment")
        .and_then(Value::as_object_mut)
    {
        for (field, other) in [
            ("agent", &recipient["agent"]),
            ("role", &recipient["role"]),
            ("nativeTaskName", &recipient["nativeName"]),
            ("displayName", &instructions["displayName"]),
            ("attempt", &current["phase"]["attempt"]),
            ("stage", &current["phase"]["stage"]),
        ] {
            if !other.is_null() && packet.get(field) == Some(other) {
                packet.remove(field);
            }
        }
        for (field, boundary) in [
            ("frozenCrossContext", "crossContext"),
            ("frozenMemoryRead", "memoryRead"),
        ] {
            let equal = packet
                .get(field)
                .zip(
                    packet
                        .get("declaredBoundary")
                        .and_then(|value| value.get(boundary)),
                )
                .is_some_and(|(left, right)| left == right);
            if equal {
                packet.remove(field);
            }
        }
        let profile_sha = &instructions["profile"]["sha256"];
        if profile_sha.is_string()
            && packet.get("profileSha256") == Some(profile_sha)
            && packet
                .get("profile")
                .is_some_and(|profile| profile == &json!({"sha256":profile_sha}))
        {
            packet.remove("profile");
            packet.remove("profileSha256");
        }
        if !check_evidence.is_null() && packet.get("checkEvidence") == Some(&check_evidence) {
            packet.remove("checkEvidence");
        }
    }
    let mut changed_aliases = Vec::new();
    let mut current_next = Value::Null;
    let mut workflow = Value::Null;
    let mut config_sha = Value::Null;
    if let Some(context) = delivery.get_mut("context").and_then(Value::as_object_mut) {
        current_next = context.get("next").cloned().unwrap_or(Value::Null);
        workflow = context
            .get("run")
            .map_or(Value::Null, |run| run["workflow"].clone());
        config_sha = context
            .get("scope")
            .map_or(Value::Null, |scope| scope["configSha256"].clone());
        let next_is_current = context
            .get("next")
            .and_then(Value::as_object)
            .is_some_and(|next| {
                next.keys().all(|key| {
                    matches!(
                        key.as_str(),
                        "action"
                            | "agent"
                            | "assignment"
                            | "freshness"
                            | "owner"
                            | "role"
                            | "source"
                    )
                }) && next.get("action") == current.get("action")
                    && next.get("agent") == recipient.get("agent")
                    && next.get("assignment") == recipient.get("assignment")
                    && next.get("role") == recipient.get("role")
                    && next.get("owner") == recipient.get("role")
                    && next.get("freshness") == Some(&json!("current"))
                    && next.get("source") == Some(&json!("run_state"))
            });
        if next_is_current {
            context.remove("next");
            changed_aliases.push("next");
        }
        let subject = &current["result"]["subject"];
        let scope = context.get("scope").cloned().unwrap_or(Value::Null);
        let run = context.get("run").cloned().unwrap_or(Value::Null);
        if let Some(value) = context.get_mut("subject").and_then(Value::as_object_mut) {
            const KNOWN: &[&str] = &[
                "version",
                "attempt",
                "sha256",
                "workerArtifactSha256",
                "previousSubjectSha256",
                "transitionSha256",
                "runId",
                "configSha256",
                "goalSha256",
                "planSha256",
            ];
            if value.keys().all(|key| KNOWN.contains(&key.as_str()))
                && subject["sha256"].is_string()
                && ["sha256", "attempt", "workerArtifactSha256"]
                    .iter()
                    .all(|field| value.get(*field) == subject.get(*field))
            {
                for field in ["sha256", "attempt", "workerArtifactSha256"] {
                    value.remove(field);
                }
                for (field, other) in [
                    ("configSha256", &scope["configSha256"]),
                    ("planSha256", &scope["planSha256"]),
                    ("goalSha256", &current["goal"]["workGoalSha256"]),
                    ("runId", &run["id"]),
                ] {
                    if other.is_string() && value.get(field) == Some(other) {
                        value.remove(field);
                    }
                }
                changed_aliases.push("subject");
            }
        }
    }
    if let Some(aliases) = delivery
        .get_mut("equalContextAliases")
        .and_then(Value::as_array_mut)
    {
        aliases.retain(|alias| {
            !alias
                .as_str()
                .is_some_and(|name| changed_aliases.contains(&name))
        });
    }
    if let Some(residual) = delivery.get_mut("residual").and_then(Value::as_object_mut) {
        if residual.get("work") == Some(&work) {
            residual.remove("work");
        }
        if residual.get("next") == current.get("action") {
            residual.remove("next");
        }
        if workflow.is_string() && residual.get("workflow") == Some(&workflow) {
            residual.remove("workflow");
        }
        if residual.get("historical") == current.pointer("/result/historical") {
            residual.remove("historical");
        }
        if residual.get("remaining")
            == residual
                .get("humanHelp")
                .and_then(|help| help.get("whatRemains"))
        {
            residual.remove("remaining");
        }
        if let Some(invalidation) = residual
            .get_mut("invalidation")
            .and_then(Value::as_object_mut)
            .filter(|invalidation| {
                config_sha.is_string() && invalidation.get("configSha256") == Some(&config_sha)
            })
        {
            invalidation.remove("configSha256");
        }
        if let Some(next) = residual
            .get_mut("context")
            .and_then(|context| context.get_mut("next"))
            .and_then(Value::as_object_mut)
            .filter(|next| {
                current_next.as_object().is_some_and(|original| {
                    next.keys()
                        .all(|key| key == "outcomes" || original.get(key) == next.get(key))
                        && next.get("outcomes").is_some_and(Value::is_array)
                })
            })
        {
            next.retain(|key, _| key == "outcomes");
        }
    }
    let tasks = &delivery["tasks"];
    let system_text = tasks["systemText"].as_str();
    let shown_text = progress["systemText"].as_str();
    let next_action = current["action"].as_str();
    if tasks.as_object().is_some_and(|object| {
        object.keys().all(|key| {
            matches!(
                key.as_str(),
                "decomposition" | "goal" | "overall" | "resultReadiness" | "systemText" | "tasks"
            )
        })
    }) && tasks["goal"].is_null()
        && tasks["overall"] == "not_requested"
        && tasks["tasks"] == json!([])
        && progress["taskOmissions"] == 0
        && ["decomposition", "overall", "resultReadiness", "tasks"]
            .iter()
            .all(|field| tasks[*field] == progress[*field])
        && system_text.zip(next_action).is_some_and(|(text, action)| {
            shown_text == Some(format!("{text} | Next: {action}").as_str())
        })
    {
        delivery.as_object_mut().unwrap().remove("tasks");
    }
}

/// Ordinary steps keep the complete current detail route. Remove the known
/// duplicate section/grouped reads and incomplete expert prefix only after
/// their Work, binding, executable, configuration and shape all match.
fn omit_redundant_current_routes(value: &mut Value, executable: &str, config: &str) {
    let Some(work) = value["work"].as_str() else {
        return;
    };
    let Some(binding) = value["current"]["binding"].as_str() else {
        return;
    };
    if value["effectiveAction"]["exists"] != true
        || value["effectiveAction"]["binding"] != binding
        || value["effectiveAction"]["requiresDetail"] == true
        || value["transportCompletion"]["complete"] == false
        || value["full"]["readOnly"] != true
        || value["full"]["binding"] != binding
        || value["full"]["command"]
            != json!([executable, "work", "detail", work, "--json", "--config", config])
    {
        return;
    }
    let work = work.to_owned();
    let binding = binding.to_owned();
    let route_is_current = |route: &Value, name: &str| {
        let Some(object) = route.as_object() else {
            return false;
        };
        if object.keys().any(|key| {
            !matches!(
                key.as_str(),
                "command"
                    | "readOnly"
                    | "reference"
                    | "kind"
                    | "version"
                    | "sameConfigRequired"
                    | "sameExecutableRequired"
            )
        }) || ["sameConfigRequired", "sameExecutableRequired"]
            .iter()
            .any(|key| {
                object
                    .get(*key)
                    .is_some_and(|flag| flag.as_bool() != Some(false))
            })
            || route["readOnly"] != true
        {
            return false;
        }
        let reference = if name == "grouped" {
            if route["kind"] != "work_detail" || route["version"] != 1 {
                return false;
            }
            format!("ref:work-detail:v1:{binding}")
        } else if ["assignment", "evidence", "recipient", "tasks"].contains(&name) {
            if object.contains_key("kind") || object.contains_key("version") {
                return false;
            }
            format!("ref:work-section:v2:{binding}:{name}:0")
        } else {
            return false;
        };
        route["reference"] == reference
            && route["command"]
                == json!([
                    executable, "work", "expand", work, reference, "--json", "--config", config
                ])
    };
    let all_sections_current = value["current"]["details"]
        .as_object()
        .is_some_and(|details| {
            !details.is_empty()
                && details
                    .iter()
                    .all(|(name, route)| route_is_current(route, name))
        });
    if all_sections_current {
        value["current"].as_object_mut().unwrap().remove("details");
    }
    if route_is_current(&value["effectiveAction"]["detail"], "grouped") {
        value["effectiveAction"]
            .as_object_mut()
            .unwrap()
            .remove("detail");
    }
    let action = &value["effectiveAction"];
    let has_current_form = action["beforeEditing"]["command"]["argv"].is_array()
        || action["command"]["argv"].is_array()
        || action["choices"].as_array().is_some_and(|choices| {
            choices
                .iter()
                .any(|choice| choice["command"]["argv"].is_array())
        });
    let fill = &action["fillRoute"];
    if has_current_form
        && fill["executable"] == false
        && fill["meaning"] == "template prefix: choose a current form and supply its documented inputs; use its complete command for ordinary execution"
        && fill.as_object().is_some_and(|object| {
            object.keys().all(|key| matches!(key.as_str(), "executable" | "meaning" | "sameConfigRequired" | "sameExecutableRequired" | "templatePrefix"))
                && ["sameConfigRequired", "sameExecutableRequired"].iter().all(|key| fill[*key] == false)
        })
        && fill["templatePrefix"] == json!([executable, "work", "fill", work, "--current-binding", binding, "--config", config])
    {
        value["effectiveAction"]
            .as_object_mut()
            .unwrap()
            .remove("fillRoute");
    }
}

/// Keep one copy of an identical current check command while preserving the
/// execution options and meaning that belonged to its mechanical form.
fn consolidate_mechanical_action(value: &mut Value) {
    let Some(action) = value
        .get_mut("effectiveAction")
        .and_then(Value::as_object_mut)
    else {
        return;
    };
    let Some(mechanical) = action.get("mechanicalAction").and_then(Value::as_object) else {
        return;
    };
    if mechanical.get("command").is_none()
        || mechanical.get("command") != action.get("command")
        || mechanical
            .keys()
            .any(|key| !matches!(key.as_str(), "command" | "executionOptions" | "meaning"))
        || ["executionOptions", "meaning"]
            .iter()
            .any(|key| action.contains_key(*key))
    {
        return;
    }
    let mechanical = action
        .remove("mechanicalAction")
        .expect("checked mechanical form");
    for key in ["executionOptions", "meaning"] {
        if let Some(item) = mechanical.get(key) {
            action.insert(key.to_owned(), item.clone());
        }
    }
}

/// A missing per-route flag inherits these defaults only on current CLI
/// routes. Never infer them for an unresolved route or a template prefix.
fn consolidate_route_defaults(value: &mut Value, executable: &str, config: &str) {
    if value.get("routeDefaults").is_some() {
        return;
    }
    const ROUTES: &[&str] = &[
        "/current/details/grouped",
        "/current/details/assignment",
        "/current/details/evidence",
        "/current/details/recipient",
        "/current/details/tasks",
        "/effectiveAction/beforeEditing/command",
        "/effectiveAction/command",
        "/effectiveAction/mechanicalAction/command",
        "/effectiveAction/detail",
        "/continuation",
        "/audit",
        "/full",
        "/nextAction",
        "/nextAction/command",
        "/continuation/command",
        "/audit/command",
    ];
    if ROUTES.iter().any(|pointer| {
        value
            .pointer(pointer)
            .is_some_and(|route| unflagged_route_is_incomplete(route, executable, config))
    }) || value
        .pointer("/effectiveAction/choices")
        .and_then(Value::as_array)
        .is_some_and(|choices| {
            choices.iter().any(|choice| {
                choice
                    .get("command")
                    .is_some_and(|route| unflagged_route_is_incomplete(route, executable, config))
            })
        })
    {
        return;
    }
    let mut removed = 0;
    for pointer in ROUTES {
        if let Some(route) = value.pointer_mut(pointer) {
            removed += if consolidate_route_flags(route, executable, config) {
                1
            } else {
                0
            };
        }
    }
    if let Some(choices) = value
        .pointer_mut("/effectiveAction/choices")
        .and_then(Value::as_array_mut)
    {
        for choice in choices {
            if let Some(route) = choice.get_mut("command") {
                removed += if consolidate_route_flags(route, executable, config) {
                    1
                } else {
                    0
                };
            }
        }
    }
    if removed > 0 {
        value["routeDefaults"] = json!({"sameConfigRequired":false,"sameExecutableRequired":false,
            "meaning":"Defaults for current action and navigation routes; explicit flags override them."});
    }
}

fn unflagged_route_is_incomplete(route: &Value, executable: &str, config: &str) -> bool {
    let Some(object) = route.as_object() else {
        return false;
    };
    if ["sameConfigRequired", "sameExecutableRequired"]
        .iter()
        .any(|key| object.get(*key).is_some_and(|value| !value.is_boolean()))
    {
        return true;
    }
    if object
        .get("sameConfigRequired")
        .and_then(Value::as_bool)
        .is_some()
        && object
            .get("sameExecutableRequired")
            .and_then(Value::as_bool)
            .is_some()
    {
        return false;
    }
    ["argv", "command"]
        .iter()
        .filter_map(|key| object.get(*key).and_then(Value::as_array))
        .any(|argv| !argv_has_cli_context(argv, executable, config))
}

fn consolidate_route_flags(route: &mut Value, executable: &str, config: &str) -> bool {
    let Some(object) = route.as_object_mut() else {
        return false;
    };
    if object.get("sameConfigRequired") != Some(&json!(false))
        || object.get("sameExecutableRequired") != Some(&json!(false))
        || object
            .get("available")
            .is_some_and(|available| available.as_bool() != Some(true))
    {
        return false;
    }
    let argv = match (object.get("argv"), object.get("command")) {
        (Some(Value::Array(argv)), None | Some(Value::Null))
        | (None | Some(Value::Null), Some(Value::Array(argv))) => argv,
        _ => return false,
    };
    if !argv_has_cli_context(argv, executable, config) {
        return false;
    }
    object.remove("sameConfigRequired");
    object.remove("sameExecutableRequired");
    true
}

fn argv_has_cli_context(argv: &[Value], executable: &str, config: &str) -> bool {
    let mut configurations = argv.windows(2).filter(|words| words[0] == "--config");
    argv.first().and_then(Value::as_str) == Some(executable)
        && matches!(
            argv.get(1).and_then(Value::as_str),
            Some("work" | "run" | "goal")
        )
        && argv.iter().all(Value::is_string)
        && argv
            .iter()
            .filter(|value| value.as_str() == Some("--config"))
            .count()
            == 1
        && configurations.next().and_then(|words| words[1].as_str()) == Some(config)
        && configurations.next().is_none()
}

fn consolidate_public_step_metadata(loaded: &config::Loaded, value: &mut Value) {
    if value["effectiveAction"]["exists"] != true || value["actionForms"].is_object() {
        return;
    }
    consolidate_mechanical_action(value);
    if let (Ok(executable), Some(config)) = (crate::emitted::executable(), loaded.path.to_str()) {
        consolidate_route_defaults(value, &executable, config);
    }
}

fn compact_form_is_redundant(compact: &Value, action: &Value) -> bool {
    let Some(compact) = compact.as_object() else {
        return false;
    };
    const KNOWN: &[&str] = &[
        "actor",
        "assignment",
        "beforeEditing",
        "binding",
        "choices",
        "choiceActor",
        "currentGrant",
        "detail",
        "leadChoiceRequired",
        "mechanicalAction",
        "state",
        "version",
        "work",
    ];
    if compact.keys().any(|key| !KNOWN.contains(&key.as_str()))
        || compact.get("state") != action.get("formState")
        || compact.get("binding") != action.get("binding")
    {
        return false;
    }
    for field in [
        "actor",
        "assignment",
        "beforeEditing",
        "choiceActor",
        "currentGrant",
        "leadChoiceRequired",
        "mechanicalAction",
        "version",
        "work",
    ] {
        if compact
            .get(field)
            .is_some_and(|value| action.get(field) != Some(value))
        {
            return false;
        }
    }
    let labels = |choices: &Value| -> Option<Vec<String>> {
        let mut result = Vec::new();
        for choice in choices.as_array()? {
            if choice["kind"] == "outcome_template" {
                result.extend(
                    choice["outcomes"]
                        .as_array()?
                        .iter()
                        .map(|item| item.as_str().map(str::to_owned))
                        .collect::<Option<Vec<_>>>()?,
                );
            } else {
                result.push(choice["label"].as_str()?.to_owned());
            }
        }
        Some(result)
    };
    matches!((labels(compact.get("choices").unwrap_or(&Value::Null)), labels(&action["choices"])),
        (Some(left), Some(right)) if left == right)
}

fn navigation_step(loaded: &config::Loaded, work: &str, original: &Value) -> Option<Value> {
    if original["reason"]["code"] != "explicit_handle"
        || original["effect"] != "no-change"
        || original["next"]["held"].is_object()
        || original["next"]["heldResults"].is_array()
        || original["warnings"]
            .as_array()
            .is_some_and(|warnings| !warnings.is_empty())
        || original["next"]["warnings"]
            .as_array()
            .is_some_and(|warnings| !warnings.is_empty())
    {
        return None;
    }
    // The compact projector can discard the current form to meet its budget.
    // Validate against the unabridged current snapshot before projecting a
    // normal navigation step; exceptional states keep the compact fallback.
    let mut detail = crate::work::readable::current(loaded, work).ok()?;
    if detail["binding"] != original["next"]["current"]["binding"]
        || detail["complete"] != true
        || !detail["actionForms"].is_object()
        || detail["actionForms"]["binding"] != detail["binding"]
        || original["next"]["current"]["actionForm"]["binding"] != detail["binding"]
        || detail["actionForms"]["state"] != original["next"]["current"]["actionForm"]["state"]
    {
        return None;
    }
    if !matches!(
        detail["actionForms"]["state"].as_str(),
        Some("initial_scope" | "pending_assignment" | "check" | "lead_acceptance")
    ) {
        return None;
    }
    crate::work::readable::public_delivery(&mut detail);
    let identity = crate::work::readable::public_context_identities(&detail)?;
    let delivery = json!({"kind":"work_context_reference","complete":false,
        "reason":"compare the current content identities with complete context already read; otherwise use full detail",
        "binding":detail["binding"],"recipient":detail["recipient"],"contextIdentity":identity,
        "inputsSha256":detail["sections"]["assignment"]["residual"]["snapshot"]["inputsSha256"]});
    let current = navigation_current(&original["next"]["current"])?;
    let mut next = crate::work::compact::compact_next(&original["next"]);
    if let Some(next) = next.as_object_mut() {
        let preservation_omitted = next
            .get("packet")
            .is_some_and(|packet| packet["preservationAssignment"].is_object());
        next.remove("packet");
        next.remove("goal");
        if preservation_omitted {
            next.insert("requiresExpansion".into(), json!(true));
            next.insert("constraintsOmitted".into(), json!(true));
        }
        if let Some(progress) = next.get_mut("progress").and_then(Value::as_object_mut) {
            if progress.keys().all(|key| {
                matches!(
                    key.as_str(),
                    "applicable" | "components" | "percent" | "reason" | "state" | "weights"
                )
            }) {
                progress.retain(|key, _| {
                    matches!(key.as_str(), "applicable" | "percent" | "reason" | "state")
                });
            }
        }
    }
    let mut step = json!({"compact":true,"work":work,"effect":"no-change",
        "reason":original["reason"],"current":current,"next":next,
        "currentDelivery":delivery,"actionForms":detail["actionForms"],"navigationStep":true});
    for field in [
        "producer",
        "recorder",
        "ledgerProducer",
        "recorderLimitation",
    ] {
        if let Some(value) = original.get(field) {
            step[field] = value.clone();
        }
    }
    if let Some(continuation) = original.get("continuation") {
        step["continuation"] =
            crate::work::compact::compact_continuation(continuation, work, &loaded.path).ok()?;
    }
    if let Some(presentation) = original.get("presentation") {
        step["presentation"] = crate::work::compact::compact_presentation(presentation);
    }
    if let Some(help) = original.pointer("/residual/humanHelp") {
        let mut help = help.clone();
        omit_self_navigation(&mut help, &step["current"], loaded, work);
        step["humanHelp"] = help;
    }
    if let Some(subject) = crate::work::compact::current_subject(original) {
        step["currentSubject"] = navigation_subject(&subject, &step["current"]);
    }
    let mut step = crate::work::effective_action::attach(&step);
    if step["effectiveAction"]["delegation"]
        .as_object()
        .is_some_and(|object| {
            object.keys().all(|key| {
                matches!(
                    key.as_str(),
                    "agent"
                        | "assignment"
                        | "meaning"
                        | "nativeName"
                        | "nextAction"
                        | "role"
                        | "text"
                )
            }) && object.get("nextAction").is_some_and(Value::is_string)
                && object.get("text").is_some_and(Value::is_string)
        })
    {
        let delegation = &step["effectiveAction"]["delegation"];
        let summary = json!({"nextAction":delegation["nextAction"],"text":delegation["text"]});
        step["effectiveAction"]["delegation"] = summary;
    }
    Some(step)
}

/// Present current action facts; the complete subject and assignment remain
/// accessible from the full route when their content identities differ.
fn navigation_current(current: &Value) -> Option<Value> {
    let object = current.as_object()?;
    if !current["binding"].is_string()
        || !current["recipient"]["actor"].is_string()
        || !current["repair"].is_null()
        || !current["uncreditedChild"].is_null()
        || !current["checkReplacement"].is_null()
    {
        return None;
    }
    let mut summary = serde_json::Map::new();
    for key in [
        "action",
        "binding",
        "phase",
        "readiness",
        "recipient",
        "result",
        "missing",
    ] {
        if let Some(value) = object.get(key) {
            summary.insert(key.to_owned(), value.clone());
        }
    }
    Some(Value::Object(summary))
}

fn navigation_subject(subject: &Value, current: &Value) -> Value {
    let displayed = &current["result"]["subject"];
    let mut subject_content = subject.clone();
    let Some(subject_object) = subject_content.as_object_mut() else {
        return subject.clone();
    };
    subject_object.remove("sha256");
    if !subject["sha256"].is_string()
        || subject["sha256"] != crate::evidence::hash::value(&subject_content)
        || subject["sha256"] != displayed["sha256"]
        || subject["attempt"] != displayed["attempt"]
        || subject["workerArtifactSha256"] != displayed["workerArtifactSha256"]
    {
        return subject.clone();
    }
    let mut summary =
        json!({"sha256":subject["sha256"], "attempt":subject["attempt"], "complete":false});
    for field in [
        "workerArtifactSha256",
        "previousSubjectSha256",
        "transitionSha256",
    ] {
        if !subject[field].is_null() {
            summary[field] = subject[field].clone();
        }
    }
    summary
}

/// Omit only the known route back to this same step. Different help or
/// recovery routes remain available with their original semantics.
fn omit_self_navigation(help: &mut Value, current: &Value, loaded: &config::Loaded, work: &str) {
    let action = &help["nextAction"];
    let command = &action["command"];
    let exact_route = crate::emitted::executable()
        .ok()
        .zip(loaded.path.to_str())
        .is_some_and(|(executable, config)| {
            command["argv"] == json!([executable, "work", "next", work, "--config", config])
        });
    if exact_route
        && action["actor"] == current["recipient"]["actor"]
        && action["summary"] == "run the pending worker assignment"
        && action.as_object().is_some_and(|object| {
            object
                .keys()
                .all(|key| matches!(key.as_str(), "actor" | "command" | "summary"))
        })
        && command.as_object().is_some_and(|object| {
            object
                .keys()
                .all(|key| matches!(key.as_str(), "args" | "argv" | "program"))
        })
        && command["args"] == json!(["work", "next", work])
        && command["program"] == "exitbind"
    {
        help.as_object_mut()
            .expect("human help object")
            .remove("nextAction");
    }
}

#[cfg(test)]
mod child_prepare_partial_tests {
    use super::*;
    use std::path::Path;

    fn fixture() -> (Value, Value) {
        let executable = crate::emitted::executable().unwrap();
        let value = json!({"work":"smw_work", "current":{"binding":"binding",
            "recipient":{"assignment":"sma_actor", "nativeName":"sonic"}},
            "next":{"action":"spawn","assignment":"sma_actor"}});
        let child = json!({"command":[executable,"work","child","prepare","smw_work","sma_actor",
            "--agent-type","sonic","--current-binding","binding","--config","/tmp/config"],
            "sameExecutableRequired":false,"sameConfigRequired":false});
        (value, child)
    }

    #[test]
    fn child_prepare_survives_partial_only_for_current_complete_route() {
        let (mut value, child) = fixture();
        retain_partial_child_prepare(&mut value, &child, Path::new("/tmp/config")).unwrap();
        assert_eq!(value["next"]["prepareChild"], child);
        assert!(
            serde_json::to_vec(&value).unwrap().len() < crate::work::compact::MAX_RESPONSE_BYTES
        );

        for index in [0, 4, 5, 7, 9, 11] {
            let (mut value, mut child) = fixture();
            child["command"][index] = json!("forged");
            retain_partial_child_prepare(&mut value, &child, Path::new("/tmp/config")).unwrap();
            assert_eq!(value["next"]["prepareChild"]["available"], false);
            assert!(value["next"]["prepareChild"].get("command").is_none());
            let recovery = value["recovery"]["command"].as_array().unwrap();
            assert_eq!(
                &recovery[1..4],
                &[json!("work"), json!("next"), json!("smw_work")]
            );
            assert!(value["recovery"]["command"]
                .as_array()
                .unwrap()
                .iter()
                .any(|v| v == "--full"));
        }
        for flag in ["sameExecutableRequired", "sameConfigRequired"] {
            let (mut value, mut child) = fixture();
            child[flag] = json!(true);
            retain_partial_child_prepare(&mut value, &child, Path::new("/tmp/config")).unwrap();
            assert_eq!(value["next"]["prepareChild"]["available"], false);
            assert!(value["next"]["prepareChild"].get("command").is_none());
        }
    }

    #[test]
    fn oversized_child_prepare_is_wholly_omitted_with_full_next_recovery() {
        let (mut value, mut child) = fixture();
        let argv = child["command"].as_array_mut().unwrap();
        drop(argv.splice(10..10, [json!("--perspectives"), json!("x".repeat(9_000))]));
        retain_partial_child_prepare(&mut value, &child, Path::new("/tmp/config")).unwrap();
        assert!(
            serde_json::to_vec(&value).unwrap().len() < crate::work::compact::MAX_RESPONSE_BYTES
        );
        assert_eq!(value["next"]["prepareChild"]["available"], false);
        assert!(value["next"]["prepareChild"].get("command").is_none());
        assert_eq!(value["recovery"]["sameExecutableRequired"], false);
        assert_eq!(value["recovery"]["sameConfigRequired"], false);
        assert!(value["recovery"]["command"]
            .as_array()
            .unwrap()
            .iter()
            .any(|v| v == "--full"));

        let (mut value, mut child) = fixture();
        let config = format!("/tmp/{}", "x".repeat(9_000));
        let argv = child["command"].as_array_mut().unwrap();
        *argv.last_mut().unwrap() = json!(config.as_str());
        retain_partial_child_prepare(&mut value, &child, Path::new(&config)).unwrap();
        assert!(
            serde_json::to_vec(&value).unwrap().len() < crate::work::compact::MAX_RESPONSE_BYTES
        );
        assert_eq!(value["next"]["prepareChild"]["available"], false);
        assert_eq!(value["recovery"]["available"], false);
        assert!(value["recovery"].get("command").is_none());
    }
}

#[cfg(test)]
mod navigation_delta_tests {
    use super::*;

    #[test]
    fn current_projection_preserves_identity_and_refuses_repair_state() {
        let current = json!({"binding":"binding", "action":"spawn", "readiness":"IN_PROGRESS",
            "recipient":{"actor":"worker"}, "result":{"subject":{"sha256":"subject"}},
            "missing":{"items":[{"obligation":"implementation"}]}});
        let projected = navigation_current(&current).unwrap();
        for key in [
            "binding",
            "action",
            "readiness",
            "recipient",
            "result",
            "missing",
        ] {
            assert_eq!(projected[key], current[key]);
        }
        let mut repair = current.clone();
        repair["repair"] = json!({"required":true});
        assert!(navigation_current(&repair).is_none());
        repair = current;
        repair["uncreditedChild"] = json!({"state":"pending"});
        assert!(navigation_current(&repair).is_none());

        let mut subject = json!({"attempt":1,"goalSha256":"goal", "workerArtifactSha256":null,
            "previousSubjectSha256":null,"transitionSha256":null});
        let subject_sha256 = crate::evidence::hash::value(&subject);
        subject["sha256"] = json!(subject_sha256);
        let current = json!({"result":{"subject":{"sha256":subject["sha256"],
            "attempt":1,"workerArtifactSha256":null}}});
        let summary = navigation_subject(&subject, &current);
        assert_eq!(summary["sha256"], subject["sha256"]);
        assert_eq!(summary["complete"], false);
        let mut forged = subject.clone();
        forged["goalSha256"] = json!("different");
        assert_eq!(navigation_subject(&forged, &current), forged);
        let mut changed_current = current;
        changed_current["result"]["subject"]["sha256"] = json!("different");
        assert_eq!(navigation_subject(&subject, &changed_current), subject);
    }
}

#[cfg(test)]
mod public_step_metadata_tests {
    use super::*;

    #[test]
    fn actor_step_omits_only_equal_current_facts() {
        let current = json!({"binding":"b","action":"spawn",
            "recipient":{"actor":"worker","agent":"worker","assignment":"a","nativeName":"worker","role":"worker"},
            "phase":{"attempt":1,"stage":2},"result":{"historical":false,
                "subject":{"sha256":"subject","attempt":1,"workerArtifactSha256":"artifact"}}});
        let mut value = json!({"work":"w","currentDelivery":{"kind":"work_actor_context",
            "work":"w","binding":"b","recipient":current["recipient"],
            "instructions":{"displayName":"Worker","profile":{"sha256":"profile","content":"instructions"}},
            "assignment":{"agent":"worker","role":"worker","nativeTaskName":"worker",
                "displayName":"Worker","attempt":1,"stage":2,"goal":"keep goal",
                "declaredBoundary":{"crossContext":"none","memoryRead":[]},
                "frozenCrossContext":"none","frozenMemoryRead":[],
                "profile":{"sha256":"profile"},"profileSha256":"profile"},
            "context":{"next":{"action":"spawn","agent":"worker","assignment":"a",
                "freshness":"current","owner":"worker","role":"worker","source":"run_state"},
                "subject":{"sha256":"subject","attempt":1,"workerArtifactSha256":"artifact",
                    "transitionSha256":"keep transition"}},
            "residual":{"work":"w","next":"spawn","historical":false,
                "remaining":[{"obligation":"implementation"}],
                "humanHelp":{"whatRemains":[{"obligation":"implementation"}]}}}});
        let original = value.clone();
        deduplicate_actor_step(&mut value, &current);
        assert_eq!(value["currentDelivery"]["assignment"]["goal"], "keep goal");
        assert_eq!(
            value["currentDelivery"]["instructions"]["profile"]["content"],
            "instructions"
        );
        assert!(value["currentDelivery"]["assignment"]
            .get("profileSha256")
            .is_none());
        assert!(value["currentDelivery"]["context"].get("next").is_none());
        assert_eq!(
            value["currentDelivery"]["context"]["subject"]["transitionSha256"],
            "keep transition"
        );
        assert!(value["currentDelivery"]["context"]["subject"]
            .get("sha256")
            .is_none());
        assert!(value["currentDelivery"]["residual"]
            .get("remaining")
            .is_none());
        assert_eq!(
            value["currentDelivery"]["residual"]["humanHelp"]["whatRemains"],
            original["currentDelivery"]["residual"]["remaining"]
        );

        let mut different = original.clone();
        different["currentDelivery"]["context"]["subject"]["sha256"] = json!("other");
        different["currentDelivery"]["assignment"]["frozenMemoryRead"] = json!(["different"]);
        deduplicate_actor_step(&mut different, &current);
        assert_eq!(
            different["currentDelivery"]["context"]["subject"]["sha256"],
            "other"
        );
        assert_eq!(
            different["currentDelivery"]["assignment"]["frozenMemoryRead"],
            json!(["different"])
        );
        let mut unknown = original.clone();
        unknown["currentDelivery"]["context"]["subject"]["future"] = json!("keep");
        let subject = unknown["currentDelivery"]["context"]["subject"].clone();
        deduplicate_actor_step(&mut unknown, &current);
        assert_eq!(unknown["currentDelivery"]["context"]["subject"], subject);
        let mut foreign = original.clone();
        foreign["currentDelivery"]["binding"] = json!("old");
        let before = foreign.clone();
        deduplicate_actor_step(&mut foreign, &current);
        assert_eq!(foreign, before);
        let mut unmatched = original.clone();
        unmatched["currentDelivery"]["context"]["evidence"] = json!([{"futureFact":"retain"}]);
        let before = unmatched.clone();
        deduplicate_actor_step(&mut unmatched, &current);
        assert_eq!(unmatched, before);
    }

    #[test]
    fn only_matching_inactive_reads_and_expert_prefix_are_omitted() {
        let section = "ref:work-section:v2:b:assignment:0";
        let grouped = "ref:work-detail:v1:b";
        let mut value = json!({"work":"w", "current":{"binding":"b","details":{
            "assignment":{"reference":section,"readOnly":true,
                "command":["/bin/exitbind","work","expand","w",section,"--json","--config","/tmp/config"]}}},
            "full":{"binding":"b","readOnly":true,
                "command":["/bin/exitbind","work","detail","w","--json","--config","/tmp/config"]},
            "effectiveAction":{"exists":true,"binding":"b","requiresDetail":false,
                "beforeEditing":{"command":{"argv":["permit"]}},
                "detail":{"kind":"work_detail","version":1,"reference":grouped,"readOnly":true,
                    "command":["/bin/exitbind","work","expand","w",grouped,"--json","--config","/tmp/config"]},
                "fillRoute":{"executable":false,"sameConfigRequired":false,"sameExecutableRequired":false,
                    "meaning":"template prefix: choose a current form and supply its documented inputs; use its complete command for ordinary execution",
                    "templatePrefix":["/bin/exitbind","work","fill","w","--current-binding","b","--config","/tmp/config"]}},
            "nextAction":{"type":"inspect","command":["exact-saved-event"]}});
        let original = value.clone();
        omit_redundant_current_routes(&mut value, "/bin/exitbind", "/tmp/config");
        assert!(value["current"].get("details").is_none());
        assert!(value["effectiveAction"].get("detail").is_none());
        assert!(value["effectiveAction"].get("fillRoute").is_none());
        assert_eq!(value["full"], original["full"]);
        assert_eq!(value["nextAction"], original["nextAction"]);
        assert_eq!(
            value["effectiveAction"]["beforeEditing"],
            original["effectiveAction"]["beforeEditing"]
        );

        let mut unknown = original.clone();
        unknown["current"]["details"]["assignment"]["unknown"] = json!("retain");
        let expected = unknown["current"]["details"].clone();
        omit_redundant_current_routes(&mut unknown, "/bin/exitbind", "/tmp/config");
        assert_eq!(unknown["current"]["details"], expected);
        let mut wrong_binding = original.clone();
        wrong_binding["full"]["binding"] = json!("other");
        let before = wrong_binding.clone();
        omit_redundant_current_routes(&mut wrong_binding, "/bin/exitbind", "/tmp/config");
        assert_eq!(wrong_binding, before);
        let mut wrong_config = original.clone();
        wrong_config["current"]["details"]["assignment"]["command"][7] = json!("/other");
        let expected = wrong_config["current"]["details"].clone();
        omit_redundant_current_routes(&mut wrong_config, "/bin/exitbind", "/tmp/config");
        assert_eq!(wrong_config["current"]["details"], expected);
        let mut incomplete = original.clone();
        incomplete["transportCompletion"] = json!({"complete":false});
        let before = incomplete.clone();
        omit_redundant_current_routes(&mut incomplete, "/bin/exitbind", "/tmp/config");
        assert_eq!(incomplete, before);
    }

    #[test]
    fn route_defaults_require_complete_matching_live_argv() {
        let complete = json!({"argv":["/bin/exitbind","work","return","w","--reason","<REASON>","--config","/tmp/config"],
            "sameConfigRequired":false,"sameExecutableRequired":false});
        let prefix = json!({"argv":["work","return","w","--config"],
            "sameConfigRequired":true,"sameExecutableRequired":true});
        let wrong_config = json!({"argv":["/bin/exitbind","work","return","w","--config","/tmp/other"],
            "sameConfigRequired":false,"sameExecutableRequired":false});
        let dangling = json!({"argv":["/bin/exitbind","work","return","w","--config","/tmp/config","--config"],
            "sameConfigRequired":false,"sameExecutableRequired":false});
        let template = json!({"templatePrefix":["/bin/exitbind","work","fill","w","--config","/tmp/config"],
            "sameConfigRequired":false,"sameExecutableRequired":false,"executable":false});
        let evidence = json!({"argv":["/bin/exitbind","work","return","w","--config","/tmp/config"],
            "sameConfigRequired":false,"sameExecutableRequired":false});
        let mut value = json!({"effectiveAction":{"choices":[{"command":complete},
            {"command":prefix},{"command":wrong_config},{"command":dangling}],"fillRoute":template},"evidence":evidence});
        let before = value.clone();
        consolidate_route_defaults(&mut value, "/bin/exitbind", "/tmp/config");
        assert_eq!(value["routeDefaults"]["sameConfigRequired"], false);
        assert_eq!(value["routeDefaults"]["sameExecutableRequired"], false);
        let command = &value["effectiveAction"]["choices"][0]["command"];
        assert_eq!(
            command["argv"],
            before["effectiveAction"]["choices"][0]["command"]["argv"]
        );
        assert!(command.get("sameConfigRequired").is_none());
        assert!(command.get("sameExecutableRequired").is_none());
        assert_eq!(
            value["effectiveAction"]["choices"][1],
            before["effectiveAction"]["choices"][1]
        );
        assert_eq!(
            value["effectiveAction"]["choices"][2],
            before["effectiveAction"]["choices"][2]
        );
        assert_eq!(
            value["effectiveAction"]["choices"][3],
            before["effectiveAction"]["choices"][3]
        );
        assert_eq!(
            value["effectiveAction"]["fillRoute"],
            before["effectiveAction"]["fillRoute"]
        );
        assert_eq!(value["evidence"], before["evidence"]);

        let mut unknown = before;
        unknown["nextAction"] = json!({"command":["work","next","w","--config"]});
        let unchanged = unknown.clone();
        consolidate_route_defaults(&mut unknown, "/bin/exitbind", "/tmp/config");
        assert_eq!(unknown, unchanged);

        let mut unknown = unchanged;
        unknown["nextAction"] = json!({"command":["/bin/exitbind","work","next","w","--config","/tmp/config"],
            "sameConfigRequired":"false","sameExecutableRequired":false});
        let unchanged = unknown.clone();
        consolidate_route_defaults(&mut unknown, "/bin/exitbind", "/tmp/config");
        assert_eq!(unknown, unchanged);
    }

    #[test]
    fn identical_mechanical_command_is_shown_once_without_losing_options() {
        let command = json!({"argv":["/bin/exitbind","work","check","w","--config","/tmp/config"],
            "sameConfigRequired":false,"sameExecutableRequired":false});
        let options = json!({"timeoutMs":{"minimum":1,"maximum":86400000}});
        let mut value = json!({"effectiveAction":{"command":command,
            "mechanicalAction":{"command":command,"executionOptions":options,"meaning":"current check"}}});
        consolidate_mechanical_action(&mut value);
        assert_eq!(value["effectiveAction"]["command"], command);
        assert_eq!(value["effectiveAction"]["executionOptions"], options);
        assert_eq!(value["effectiveAction"]["meaning"], "current check");
        assert!(value["effectiveAction"].get("mechanicalAction").is_none());

        let mut different = json!({"effectiveAction":{"command":command,
            "mechanicalAction":{"command":{"argv":["other"]},"executionOptions":options}}});
        let before = different.clone();
        consolidate_mechanical_action(&mut different);
        assert_eq!(different, before);
    }
}

fn print_work_json(value: &serde_json::Value) -> Result<(), String> {
    let mut value = crate::work::effective_action::attach(value);
    crate::work::readable::public_delivery(&mut value);
    crate::work::action_forms::public_permit_guidance(&mut value);
    crate::work::action_forms::public_fill_guidance(&mut value);
    print_json(&value)
}

/// Complete only the live navigation and action forms in a full read. The
/// surrounding packet and historical evidence remain the original payload.
fn print_active_read_json(
    loaded: &config::Loaded,
    value: &serde_json::Value,
) -> Result<(), String> {
    let mut value = crate::work::effective_action::attach(value);
    crate::work::readable::public_delivery(&mut value);
    crate::work::action_forms::public_permit_guidance(&mut value);
    crate::work::action_forms::public_fill_guidance(&mut value);
    let mut routes_complete = true;
    for pointer in [
        "/current/actionForm",
        "/current/details",
        "/next/current/actionForm",
        "/next/current/details",
        "/next/current/completion/detail/command",
        "/next/current/completion/receipt/export/command",
        "/effectiveAction",
        "/nextAction",
        "/pauseRecord",
    ] {
        if let Some(routes) = value.pointer_mut(pointer) {
            routes_complete &= crate::emitted::complete_known(routes, &loaded.path);
        }
    }
    routes_complete &= complete_live_navigation(&mut value, &loaded.path);
    if !routes_complete {
        value["transportCompletion"] = json!({"complete":false,
            "reason":"a complete current route is unavailable"});
    }
    print_json(&value)
}

/// Complete the known routes in an existing compact read without changing its
/// budget. If whole routes do not fit, keep an explicit read-only recovery.
fn print_compact_read(loaded: &config::Loaded, value: &serde_json::Value) -> Result<(), String> {
    let mut value = crate::work::effective_action::attach(value);
    crate::work::action_forms::public_permit_guidance(&mut value);
    crate::work::action_forms::public_fill_guidance(&mut value);
    let mut routes_complete = true;
    let locations = [
        "/current",
        "/next/current",
        "/actionForms",
        "/effectiveAction",
        "/nextAction",
        "/full",
        "/humanHelp",
        "/pauseRecord",
        "/history",
        "/presentation",
    ];
    for pointer in locations {
        if let Some(routes) = value.pointer_mut(pointer) {
            routes_complete &= crate::emitted::complete_known(routes, &loaded.path);
        }
    }
    routes_complete &= complete_live_navigation(&mut value, &loaded.path);
    routes_complete &=
        crate::work::compact::complete_transport_full_command(&mut value, &loaded.path);
    let child_prepare = value.pointer("/next/prepareChild").cloned();
    if !routes_complete {
        value["transportCompletion"] = json!({"complete":false,
            "reason":"a complete route is unavailable"});
    }
    let mut projected_step = false;
    if value["navigationStep"] == true {
        value.as_object_mut().unwrap().remove("navigationStep");
        crate::work::action_forms::public_return_templates(&mut value, &loaded.path);
        projected_step = project_public_step(loaded, &mut value);
        if projected_step {
            consolidate_mechanical_action(&mut value);
        }
        // The direct full route supersedes the earlier compact section and
        // recovery routes for this verified current navigation step.
        if value["currentDelivery"]["kind"] == "work_context_reference"
            && value["full"]["command"].is_array()
        {
            if let Some(current) = value["current"].as_object_mut() {
                current.remove("details");
            }
            if let Some(object) = value.as_object_mut() {
                for field in [
                    "references",
                    "omitted",
                    "truncated",
                    "fullCommand",
                    "fullCommandSameConfigRequired",
                    "fullCommandSameExecutableRequired",
                ] {
                    object.remove(field);
                }
            }
        }
    }
    let fits = |value: &serde_json::Value| {
        serde_json::to_vec(value)
            .map(|bytes| bytes.len() < crate::work::compact::MAX_RESPONSE_BYTES)
            .map_err(|error| error.to_string())
    };
    // Reuse the continuation owner's read-only route before dropping whole
    // commands. The full Work detail route is not a continuation read.
    if !fits(&value)? {
        if let (Some(work), Some(continuation)) = (
            value["work"].as_str(),
            value.get("continuation").filter(|item| item.is_object()),
        ) {
            if continuation["work"] == work {
                let mut reference =
                    crate::work::compact::continuation_reference(continuation, work, &loaded.path);
                let complete = crate::emitted::complete_known(&mut reference, &loaded.path);
                if serde_json::to_vec(&reference)
                    .map_err(|error| error.to_string())?
                    .len()
                    < serde_json::to_vec(continuation)
                        .map_err(|error| error.to_string())?
                        .len()
                {
                    value["continuation"] = reference;
                    if !complete {
                        value["transportCompletion"] = json!({"complete":false,
                            "reason":"a complete continuation route is unavailable"});
                    }
                }
            }
        }
    }
    let continuation = value.get("continuation").cloned();
    // Section-specific routes duplicate the grouped read. Give the grouped
    // current detail and the existing history route priority at the bound.
    if !fits(&value)? {
        for pointer in ["/current/details", "/next/current/details"] {
            if let Some(details) = value.pointer_mut(pointer).and_then(Value::as_object_mut) {
                for (name, route) in details {
                    if name != "grouped" {
                        crate::emitted::omit_commands(route);
                    }
                }
            }
        }
        value["transportCompletion"] = json!({"complete":false,
            "reason":"some section commands were omitted at the compact response bound; use grouped current detail"});
    }
    if !fits(&value)? {
        let mut removed = 0;
        for pointer in ["/current/details", "/next/current/details"] {
            if let Some(details) = value.pointer_mut(pointer).and_then(Value::as_object_mut) {
                if details
                    .get("grouped")
                    .is_some_and(|route| route["command"].is_array())
                {
                    let before = details.len();
                    details.retain(|name, _| name == "grouped");
                    removed += before - details.len();
                }
            }
        }
        if removed > 0 {
            value["transportCompletion"]["omittedSectionRoutes"] = json!(removed);
        }
    }
    if !fits(&value)? {
        let preferred = [
            "/next/prepareChild",
            "/continuation",
            "/current/details/grouped",
            "/next/current/details/grouped",
            "/presentation/goalProgress/adoptRoute",
            "/history",
            "/effectiveAction/detail",
        ]
        .map(|pointer| (pointer, value.pointer(pointer).cloned()));
        for pointer in locations {
            if let Some(routes) = value.pointer_mut(pointer) {
                crate::emitted::omit_commands(routes);
            }
        }
        omit_live_navigation(&mut value);
        value["transportCompletion"] = json!({"complete":false,
            "reason":"complete routes were omitted at the compact response bound"});
        // Restore each whole priority route only when it fits. Never restore a
        // prefix or disturb the focus/history facts attached to the response.
        let mut grouped_retained = false;
        for (pointer, original) in preferred {
            let Some(original) = original.filter(|route| route["command"].is_array()) else {
                continue;
            };
            let Some(previous) = value.pointer(pointer).cloned() else {
                continue;
            };
            *value.pointer_mut(pointer).expect("existing route") = original;
            if fits(&value)? {
                grouped_retained |=
                    pointer.ends_with("/grouped") || pointer == "/effectiveAction/detail";
            } else {
                *value.pointer_mut(pointer).expect("existing route") = previous;
            }
        }
        let mut route = if value["status"] == "resumed" {
            crate::emitted::Route::new(["work", "resume", "--full", "--json"])
        } else if let Some(work) = value["work"].as_str() {
            crate::emitted::Route::new(["work", "detail", work, "--json"])
        } else {
            crate::emitted::Route::new(["work", "resume", "--full", "--json"])
        }
        .config(&loaded.path)
        .within(1024)
        .command_json();
        crate::emitted::complete_known(&mut route, &loaded.path);
        route["readOnly"] = json!(true);
        if !grouped_retained {
            value["transportCompletion"]["recovery"] = route.clone();
        }
        let child_omitted = child_prepare.as_ref().is_some_and(|child| {
            child["command"].is_array()
                && !value
                    .pointer("/next/prepareChild/command")
                    .is_some_and(Value::is_array)
        });
        let continuation_omitted = continuation.as_ref().is_some_and(|original| {
            if original["command"].is_array() {
                !value
                    .pointer("/continuation/command")
                    .is_some_and(Value::is_array)
            } else {
                value.get("continuation") != Some(original)
            }
        });
        if !fits(&value)? || child_omitted || continuation_omitted {
            value = crate::work::compact::semantic_partial(&value, route, continuation)?;
            if let Some(child) = child_prepare.as_ref() {
                retain_partial_child_prepare(&mut value, child, &loaded.path)?;
            }
        }
    }
    // The compact fallback needs individual flags to omit whole routes.
    // Consolidate them only after that decision and only if the final
    // response still fits its existing diagnostic bound.
    if projected_step
        && value["currentDelivery"]["kind"] == "work_context_reference"
        && value["transportCompletion"]["complete"] != false
        && value["full"]["command"].is_array()
    {
        let mut consolidated = value.clone();
        consolidate_public_step_metadata(loaded, &mut consolidated);
        if fits(&consolidated)?
            && serde_json::to_vec(&consolidated)
                .map_err(|error| error.to_string())?
                .len()
                < serde_json::to_vec(&value)
                    .map_err(|error| error.to_string())?
                    .len()
        {
            value = consolidated;
        }
    }
    print_json(&value)
}

fn print_step_json(loaded: &config::Loaded, value: &serde_json::Value) -> Result<(), String> {
    print_bounded_step_json(loaded, value, false, None)
}

/// A grouped expansion preserves its section payloads; only its current forms
/// are projected into complete public commands.
fn print_bounded_step_json(
    loaded: &config::Loaded,
    value: &serde_json::Value,
    forms_only: bool,
    previous: Option<&serde_json::Value>,
) -> Result<(), String> {
    print_json(&bounded_step_json(loaded, value, forms_only, previous)?)
}

fn bounded_step_json(
    loaded: &config::Loaded,
    value: &serde_json::Value,
    forms_only: bool,
    previous: Option<&serde_json::Value>,
) -> Result<Value, String> {
    let mut value = crate::work::effective_action::attach(value);
    crate::work::readable::public_delivery(&mut value);
    if let Some(identity) = crate::work::readable::public_context_identities(&value) {
        value["contextIdentity"] = identity;
    }
    if let Some(identity) =
        crate::work::readable::public_context_identities(&value["currentDelivery"])
    {
        value["currentDelivery"]["contextIdentity"] = identity;
    }
    if let Some(previous) = previous {
        let mut previous = previous.clone();
        crate::work::readable::public_delivery(&mut previous);
        if let Some(continuity) =
            crate::work::readable::public_context_continuity(&previous, &value["currentDelivery"])
        {
            value["currentDelivery"] = continuity;
        }
    }
    crate::work::readable::public_embedded_delivery(&mut value);
    crate::work::action_forms::public_permit_guidance(&mut value);
    crate::work::action_forms::public_fill_guidance(&mut value);
    let mut completed = value.clone();
    let mut routes_complete = true;
    let all_route_locations = [
        "/actionForms",
        "/effectiveAction",
        "/current",
        "/next/current",
        "/nextAction",
        "/full",
        "/recipientContext/guidance",
        "/sections/tasks",
        "/sections/assignment/residual/humanHelp",
        "/currentDelivery/actionForms",
        "/currentDelivery/effectiveAction",
        "/currentDelivery/recipientContext/guidance",
        "/currentDelivery/sections/tasks",
        "/currentDelivery/sections/assignment/residual/humanHelp",
    ];
    let route_locations = if forms_only {
        &all_route_locations[..2]
    } else {
        &all_route_locations[..]
    };
    for pointer in route_locations {
        if let Some(routes) = completed.pointer_mut(pointer) {
            routes_complete &= crate::emitted::complete_known(routes, &loaded.path);
        }
    }
    if !forms_only {
        routes_complete &= complete_live_navigation(&mut completed, &loaded.path);
    }
    if !forms_only && completed["kind"] == "work_section" {
        if let Some(route) = completed.get_mut("next") {
            routes_complete &= crate::emitted::complete_known(route, &loaded.path);
        }
    }
    if !routes_complete {
        completed["transportCompletion"] = json!({"complete":false,
            "reason":"a complete route is unavailable"});
    }
    let mut projected_step = false;
    if !forms_only && completed["kind"] != "work_detail" {
        crate::work::action_forms::public_return_templates(&mut completed, &loaded.path);
        projected_step = project_public_step(loaded, &mut completed);
    }
    if serde_json::to_vec(&completed)
        .map_err(|error| error.to_string())?
        .len()
        < crate::work::readable::MAX_GROUPED_BYTES
    {
        value = completed;
    } else {
        // Preserve the recorded result and its facts, but never expose a
        // shortened argv as if it were directly executable.
        for pointer in route_locations {
            if let Some(routes) = completed.pointer_mut(pointer) {
                crate::emitted::omit_commands(routes);
            }
        }
        if !forms_only {
            omit_live_navigation(&mut completed);
        }
        completed["transportCompletion"] = json!({"complete":false,
            "reason":"complete routes were omitted at the readable response bound; inspect current detail"});
        if serde_json::to_vec(&completed)
            .map_err(|error| error.to_string())?
            .len()
            + 1
            > crate::work::readable::MAX_GROUPED_BYTES
            && completed["currentDelivery"].is_object()
        {
            completed["currentDelivery"] = json!({"complete":false,
                "reason":"current detail exceeds the response bound; inspect current detail"});
            completed["effectiveAction"]["requiresDetail"] = json!(true);
        }
        if serde_json::to_vec(&completed)
            .map_err(|error| error.to_string())?
            .len()
            + 1
            > crate::work::readable::MAX_GROUPED_BYTES
        {
            let work = completed["work"]
                .as_str()
                .ok_or("bounded Work response has no handle")?;
            let mut route = crate::emitted::Route::new(["work", "next", work, "--json"])
                .config(&loaded.path)
                .within(1024)
                .command_json();
            crate::emitted::complete_known(&mut route, &loaded.path);
            route["readOnly"] = json!(true);
            completed = json!({"work":work,"compact":true,"complete":false,
                "status":completed["status"],"effect":completed["effect"],
                "eventSha256":completed["eventSha256"],"creation":completed["creation"],
                "reason":"current delivery exceeds the readable response bound; use the read-only route",
                "nextAction":route});
        }
        value = completed;
    }
    if projected_step
        && value["transportCompletion"]["complete"] != false
        && value["full"]["command"].is_array()
    {
        let mut consolidated = value.clone();
        consolidate_public_step_metadata(loaded, &mut consolidated);
        let consolidated_bytes = serde_json::to_vec(&consolidated)
            .map_err(|error| error.to_string())?
            .len();
        if consolidated_bytes < crate::work::readable::MAX_GROUPED_BYTES
            && consolidated_bytes
                < serde_json::to_vec(&value)
                    .map_err(|error| error.to_string())?
                    .len()
        {
            value = consolidated;
        }
    }
    Ok(value)
}

fn print_work_result(loaded: &config::Loaded, value: &serde_json::Value) -> Result<(), String> {
    print_step_json(loaded, value)?;
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
        "work requires begin, next, detail, closeout, act, bind, child, permit, replan, evidence, sensor-request, sensor-result, return, disposition, check, validate, expand, resume, supersede, or link-session",
    )?;
    match action {
        "quarantine" | "quarantine-followup" => work_quarantine::command(l, a),
        "replace-check" => work_check_replacement::command(l, a),
        "supersede" => {
            args::assert_options("work supersede", a, &["config", "json", "reason"])?;
            args::assert_positionals("work supersede", a, 2)?;
            let work = positional(a, 1, "work supersede requires WORK")?;
            let reason = option(a, "reason", "work supersede requires --reason TEXT")?;
            print_work_json(&crate::work::supersede::supersede(l, work, reason)?)
        }
        "recover-check" | "recover-pre-return" | "report-lost-caller" => recovery::command(l, a, action),
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
        "challenge" => {
            challenge::command(l, a)
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
        "child" => child::command(l, a),
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
        "link-session" => {
            args::assert_options("work link-session", a, &["config", "json"])?;
            args::assert_positionals("work link-session", a, 2)?;
            print_work_json(&crate::session_goal::association::link_session(
                l,
                positional(a, 1, "work link-session requires WORK")?,
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
                    "pre-return-command",
                    "pre-return-timeout-ms",
                    "pre-return-capture-bytes",
                    "boundary",
                    "harness-receipt",
                    "proof-origin",
                    "preserve-requirement",
                    "preservation-check-command",
                    "preservation-proof-origin",
                    "basis",
                    "review-policy",
                    "goal-id",
                    "project-goal",
                    "requirement",
                    "artifact",
                    "scope",
                    "standalone",
                    "journey-integration",
                    "json",
                    "detail",
                ],
            )?;
            args::assert_positionals("work begin", a, 2)?;
            let integration=match a.options.get("scope").map(String::as_str) {
                None=>false, Some("integration")=>true, Some(_)=>return Err("named Work scope must be integration when supplied".into()),
            };
            let standalone = a.flags.contains_key("standalone");
            let journey_integration = a.flags.contains_key("journey-integration");
            let named_requested = ["goal-id","requirement","scope"]
                .iter()
                .any(|key| a.options.contains_key(*key))
                || (a.options.contains_key("artifact") && !journey_integration);
            let project_goal = a.flags.contains_key("project-goal");
            if standalone && (named_requested || journey_integration) {
                return Err("--standalone cannot be combined with named Goal or journey integration contracts".into());
            }
            if project_goal && !named_requested {
                return Err("--project-goal requires a named goal requirement contract".into());
            }
            let named_scope = if named_requested {
                let scope = if project_goal {
                    crate::session_goal::native::project_scope(l)
                } else {
                    crate::session_goal::native::scope(l, false)?
                };
                scope.require_owner()?;
                Some(scope)
            } else {
                None
            };
            let named_contract_scope = named_scope.as_ref().map_or(
                crate::session_goal::requirements::ContractScope::Project,
                |scope| if scope.is_session() {
                    crate::session_goal::requirements::ContractScope::Session
                } else {
                    crate::session_goal::requirements::ContractScope::Project
                },
            );
            let named = if named_requested {
                Some(crate::session_goal::requirements::prepare(
                    crate::session_goal::requirements::PrepareRequest {
                        root: &named_scope.as_ref().expect("named scope").root,
                        loaded: l,
                        goal_id: option(a, "goal-id", "named Work requires --goal-id ID")?,
                        ids: option(a, "requirement", "named Work requires --requirement IDs")?,
                        checker: &crate::session_goal::requirements::resolve_checker(l, a.options.get("artifact").map(String::as_str), option(a, "check-command", "work begin requires --check-command COMMAND")?)?.path,
                        command: option(a, "check-command", "work begin requires --check-command COMMAND")?,
                        basis: a.options.get("basis").map(String::as_str),
                        integration,
                        scope: named_contract_scope,
                    },
                )?)
            } else {None};
            let journey = if journey_integration {
                Some(crate::session_goal::journey::prepare(
                    l,
                    named.as_ref().map(|prepared| prepared.basis.as_str())
                        .or_else(|| a.options.get("basis").map(String::as_str)),
                    option(a, "artifact", "--journey-integration requires --artifact PROJECT_CHECKER")?,
                    option(a, "check-command", "work begin requires --check-command COMMAND")?,
                )?)
            } else {None};
            let mut response = crate::work::begin_with_pre_return(
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
                    basis: match journey {
                        Some(prepared) => {
                            crate::work::BasisOrigin::PreparedJourney(prepared)
                        }
                        None => crate::work::BasisOrigin::Caller(
                            named
                                .as_ref()
                                .map(|prepared| prepared.basis.as_str())
                                .or_else(|| a.options.get("basis").map(String::as_str)),
                        ),
                    },
                    review_policy: a.options.get("review-policy").map(String::as_str),
                    auto_goal_context: !standalone,
                },
                a.options.get("pre-return-command").map(String::as_str),
                a.options.get("pre-return-timeout-ms").map(String::as_str),
                a.options.get("pre-return-capture-bytes").map(String::as_str),
            )?;
            if let Some(prepared)=&named {
                let work=response["work"].as_str().ok_or("recorded Work identity is missing")?.to_owned();
                let root = &named_scope.as_ref().expect("named scope").root;
                let ledger = crate::work::resolve(l, &work)?;
                let (_, events, _) = crate::run::ledger::load(l, &ledger)?;
                let start = events.first().ok_or("Work ledger has no start event")?;
                let start_event_sha256 = start["eventSha256"]
                    .as_str()
                    .ok_or("Work start event identity is missing")?;
                if let Err(error)=crate::session_goal::requirements::write_scope_companion(
                    l, &work, start_event_sha256, prepared,
                ) {
                    return Err(format!("Work {work} was recorded, but its named Goal scope selector could not be saved: {error}; recover with work detail and inspect the local Goal before repeating any action; do not repeat work begin"));
                }
                if let Err(error)=crate::session_goal::requirements::assign(root,l,&prepared.goal_id,&prepared.requirements,&work,false) {
                    return Err(format!("Work {work} was recorded, but its goal mapping failed: {error}; recover with work detail and goal assign; do not repeat work begin"));
                }
                let current=crate::work::next(l,&work).map_err(|error|format!("Work {work} and its goal mapping were recorded, but context refresh failed: {error}; recover with work detail; do not repeat work begin"))?;
                response["next"]=current["next"].clone();
            }
            let mut response = if a.flags.contains_key("detail") {
                crate::work::start_delivery::with_detail(l, response)
            } else {
                response
            };
            if !a.flags.contains_key("detail") {
                if let Some(work) = response["work"].as_str().map(str::to_owned) {
                    let next = response["next"].clone();
                    crate::work::refreshed_action::attach(l, &work, &next, &mut response);
                }
            }
            print_step_json(l, &response)
        }
        "next" => {
            args::assert_options("work next", a, &["config", "json", "full", "themed"])?;
            args::assert_positionals("work next", a, 2)?;
            let work = positional(a, 1, "work next requires WORK")?;
            let result = crate::producer::observed(crate::work::next(l, work)?);
            let route = crate::session_goal::same_host_child_route(l, work, &result);
            let mut result = if a.flags.contains_key("full") {
                result
            } else if !a.flags.contains_key("themed") {
                if let Some(step) = navigation_step(l, work, &result) {
                    step
                } else {
                    crate::work::compact::project(&result, &l.path, "next")?
                }
            } else {
                crate::work::compact::project(&result, &l.path, "next")?
            };
            if let (Some(route), Some(next)) = (route, result["next"].as_object_mut()) {
                next.insert("prepareChild".into(), route);
            }
            if a.flags.contains_key("themed") {
                let world = crate::presentation_events::world::project(l,
                    positional(a, 1, "work next requires WORK")?, &result);
                result["world"] = world;
            }
            if a.flags.contains_key("full") || a.flags.contains_key("themed") {
                print_active_read_json(l, &result)
            } else {
                print_compact_read(l, &result)
            }
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
            print_work_result(l, &result)
        }
        "permit" => {
            args::assert_options("work permit", a, &["config", "operation", "request-id"])?;
            args::assert_positionals("work permit", a, 3)?;
            let work = positional(a, 1, "work permit requires WORK ASSIGNMENT")?;
            let assignment = positional(a, 2, "work permit requires WORK ASSIGNMENT")?;
            // Failure to obtain a comparable predecessor never changes the
            // actual permit; the ordinary complete delivery remains the fallback.
            let previous = crate::work::readable::current(l, work).ok();
            let result = crate::work::permit(
                l, work, assignment,
                option(a, "operation", "work permit requires --operation OPERATION")?,
                a.options.get("request-id").map(String::as_str),
            )?;
            let previous = previous.as_ref().filter(|before| {
                crate::work::refreshed_action::permit_predecessor_matches(
                    l, work, assignment, before, &result,
                )
            });
            print_bounded_step_json(l, &result, false, previous)
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
            print_work_result(l, &result)
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
            let result = match crate::work::check_with_timeout(
                l,
                positional(a, 1, "work check requires WORK")?,
                a.options.get("timeout-ms").map(String::as_str),
            ) {
                Ok(result) => result,
                Err(error) => {
                    if let Some(raw) = error.strip_prefix(MACHINE_PREFIX) {
                        if let Ok(value) = serde_json::from_str::<Value>(raw) {
                            if let Ok(projected) = bounded_step_json(l, &value, false, None) {
                                if let Ok(serialized) = serde_json::to_string(&projected) {
                                    return Err(format!("{MACHINE_PREFIX}{serialized}"));
                                }
                            }
                        }
                    }
                    return Err(error);
                }
            };
            print_work_result(l, &result)
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
            let expanded = crate::work::expand(
                l,
                positional(a, 1, "work expand requires WORK REF")?,
                positional(a, 2, "work expand requires WORK REF")?,
            )?;
            if expanded["kind"] == "work_detail" {
                print_bounded_step_json(l, &expanded, true, None)
            } else {
                print_work_json(&expanded)
            }
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
        "detail" => detail::command(l, a),
        "fill" => fill::command(l, a),
        "resume" => {
            args::assert_options("work resume", a, &["config", "json", "full", "history"])?;
            args::assert_positionals("work resume", a, 1)?;
            let result = crate::producer::observed(crate::work::resume(l, a.flags.contains_key("history"))?);
            let mut result = if a.flags.contains_key("full") || result["status"] != "resumed" {
                result
            } else {
                crate::work::compact::project(&result, &l.path, "resume")?
            };
            if let Some(routes) = crate::host::session_recovery::pause_routes(l) {
                result["pauseRecord"] = routes;
            }
            if a.flags.contains_key("full") || result["status"] != "resumed" {
                print_active_read_json(l, &result)
            } else {
                print_compact_read(l, &result)
            }
        }
        _ => Err(
            "work requires begin, next, detail, closeout, act, permit, replan, evidence, sensor-request, sensor-result, return, disposition, check, validate, expand, or resume"
                .into(),
        ),
    }
}

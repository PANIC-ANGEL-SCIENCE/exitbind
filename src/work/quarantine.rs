//! Lead-owned admission for the separately authorized quarantine check.

use crate::{config::Loaded, evidence::hash, run};
use serde_json::{json, Value};

mod followup;

pub(crate) struct Options<'a> {
    pub(crate) followup: bool,
    pub(crate) feature_config: &'a str,
    pub(crate) feature_work: &'a str,
    pub(crate) artifact_root: Option<&'a str>,
    pub(crate) artifact: &'a str,
    pub(crate) reason: &'a str,
    pub(crate) request_id: &'a str,
    pub(crate) current_binding: Option<&'a str>,
    pub(crate) owner_config: Option<&'a str>,
    pub(crate) owner_config_sha256: Option<&'a str>,
    pub(crate) apply: bool,
}

pub(crate) fn start(
    loaded: &Loaded,
    old_work: &str,
    options: Options<'_>,
) -> Result<Value, String> {
    super::details::ensure_current_config(loaded)?;
    if !super::valid_work_handle(old_work) {
        return Err("old Work handle is invalid".into());
    }
    let requested_ledger = super::resolve(loaded, old_work)?;
    let (_, requested_events, requested_bytes) = run::ledger::load(loaded, &requested_ledger)?;
    let requested_start = requested_events
        .first()
        .ok_or("requested Work ledger is empty")?;
    let requested_state = run::state::reduce(&requested_events)?;
    let requested_check_code = requested_state["checks"]
        .as_array()
        .and_then(|checks| checks.first())
        .and_then(|check| {
            check["exitCode"]
                .as_i64()
                .or_else(|| check["result"]["code"].as_i64())
        });
    let followup = requested_start["quarantineVerification"]["protocol"]
        == run::quarantine::PROTOCOL
        && requested_state["checks"]
            .as_array()
            .is_some_and(|checks| checks.len() == 1)
        && requested_check_code.is_some_and(|code| code != 0)
        && !requested_state["checkObservation"].is_object();
    if followup != options.followup {
        return Err(if options.followup {
            "quarantine-followup requires the exact known-ended failed first verification".into()
        } else {
            "a known-ended failed quarantine verification requires work quarantine-followup".into()
        });
    }
    let original_work = if followup {
        requested_start["quarantineVerification"]["predecessor"]["work"]
            .as_str()
            .ok_or("first verification's original predecessor is missing")?
    } else {
        old_work
    };
    let old_ledger = super::resolve(loaded, original_work)?;
    let (_, old_events, old_bytes) = run::ledger::load(loaded, &old_ledger)?;
    let old_start = old_events.first().ok_or("old Work ledger is empty")?;
    let old_head = old_events.last().ok_or("old Work ledger has no head")?;
    let old_state = run::state::reduce(&old_events)?;
    if !matches!(old_state["status"].as_str(), Some("running" | "blocked"))
        || old_state.get("quarantineVerification").is_some()
        || old_state["checkObservation"]["state"] != "failed"
        || old_state["checkObservation"]["facts"]["termination"] != "unknown"
        || !run::state::check_observation::unresolved(&old_state)
        || old_start["configSha256"] != hash::text(&loaded.source)
        || old_start["version"] != 8
    {
        return Err(
            "old Work is not the current unresolved unknown observation eligible for quarantine"
                .into(),
        );
    }
    let admission = old_events
        .iter()
        .find(|event| {
            event["action"] == "check_observation"
                && event["eventSha256"] == old_state["checkObservation"]["eventSha256"]
        })
        .ok_or("old Work check admission event is missing")?;
    let failure = old_events
        .iter()
        .rev()
        .find(|event| {
            event["action"] == "check_observation_failed"
                && event["observation"]["admissionEventSha256"] == admission["eventSha256"]
        })
        .ok_or("old Work has no durable failed observation event")?;

    if requested_start["configSha256"] != hash::text(&loaded.source)
        || run::state::reduce(&requested_events)?["runId"] != requested_start["runId"]
    {
        return Err("requested quarantine Work changed or has stale configuration".into());
    }
    if followup {
        followup::validate_first_failure(&requested_events, &requested_state)?;
    }
    let binding = options
        .current_binding
        .filter(|value| !value.trim().is_empty())
        .ok_or("work quarantine requires --current-binding from current Work detail")?;
    let next = super::next_for(loaded, old_work, &requested_ledger)?;
    if next["current"]["binding"] != binding {
        return Err(format!(
            "current action form binding is stale; refresh with work next {old_work} --json"
        ));
    }
    let lead = configured_lead(&old_start["plan"])?;
    if options.reason.trim().is_empty()
        || options.reason.len() > 1024
        || options.reason.contains('\0')
    {
        return Err("--reason must be non-empty and at most 1024 bytes".into());
    }
    if options.request_id.trim().is_empty()
        || options.request_id.len() > 128
        || options.request_id.contains('\0')
    {
        return Err("--request-id must be non-empty and at most 128 bytes".into());
    }

    ensure_no_children(loaded, original_work)?;
    if followup {
        ensure_no_children(loaded, old_work)?;
    }

    let feature_loaded = crate::config::load(Some(options.feature_config))?;
    same_roots(loaded, &feature_loaded)?;
    if std::fs::read_to_string(&feature_loaded.path).map_err(|e| e.to_string())?
        != feature_loaded.source
    {
        return Err("feature configuration changed during quarantine admission".into());
    }
    let owner_path = match options.owner_config {
        Some(path) => path,
        None => loaded
            .path
            .to_str()
            .ok_or("configuration path is not valid UTF-8")?,
    };
    let owner_loaded = crate::config::load(Some(owner_path))?;
    let owner_sha256 = hash::text(&owner_loaded.source);
    if std::fs::read_to_string(&owner_loaded.path).map_err(|e| e.to_string())?
        != owner_loaded.source
    {
        return Err("native Goal owner configuration changed during quarantine admission".into());
    }
    if options.apply && options.owner_config_sha256 != Some(owner_sha256.as_str()) {
        return Err("--owner-config-sha256 is required and must match the current native Goal owner configuration".into());
    }
    let owner_scope = crate::session_goal::native::scope(&owner_loaded, false)?;
    owner_scope.require_owner()?;
    if owner_scope.session.is_none() || owner_scope.owner.is_none() {
        return Err("quarantine requires the current native session and fixed Goal owner".into());
    }
    let owner_record = crate::session_goal::read(&owner_scope.root)?
        .ok_or("the fixed native Goal owner journal is unavailable")?;
    let old_context =
        crate::session_goal::association::context_for_work_with_root(loaded, original_work);
    let old_goal = match old_context.context {
        crate::session_goal::association::WorkContext::Owner(record)
        | crate::session_goal::association::WorkContext::Session(record) => record,
        _ => {
            return Err("old Work has no verified association with the original native Goal".into())
        }
    };
    if old_goal["goalId"] != owner_record["goalId"] {
        return Err(
            "current native Goal differs from the original Goal associated with old Work".into(),
        );
    }
    if let Some(root) = old_context.owner_root.as_ref() {
        if root != &owner_scope.root {
            return Err(
                "old Work association belongs to a different native Goal owner root".into(),
            );
        }
    }
    let feature_ledger = super::resolve(&feature_loaded, options.feature_work)?;
    let (_, feature_events, _) = run::ledger::load(&feature_loaded, &feature_ledger)?;
    let feature_start = feature_events
        .first()
        .ok_or("feature Work ledger is empty")?;
    let feature_head = feature_events
        .last()
        .ok_or("feature Work ledger has no head")?;
    let feature_state = run::state::reduce(&feature_events)?;
    if feature_state["status"] != "accepted" {
        return Err("feature Work must be currently accepted".into());
    }
    let check = feature_events
        .iter()
        .rev()
        .find(|event| {
            event["action"] == "check"
                && event["acquisition"] == "observed"
                && event["configSha256"] == hash::text(&feature_loaded.source)
                && event["inputsSha256"] == feature_state["inputsSha256"]
                && event["subjectSha256"] == feature_state["subject"]["sha256"]
        })
        .ok_or("feature Work has no observed check")?;
    let reviewer = feature_events
        .iter()
        .rev()
        .find(|event| {
            event["action"] == "submit"
                && event["role"] == "reviewer"
                && event["outcome"] == "approved"
        })
        .ok_or("feature Work has no reviewer approval")?;
    let accepted = feature_events
        .iter()
        .rev()
        .find(|event| {
            event["action"] == "submit" && event["role"] == "lead" && event["outcome"] == "accepted"
        })
        .ok_or("feature Work has no Lead acceptance")?;
    let worker = feature_events
        .iter()
        .rev()
        .find(|event| {
            event["action"] == "submit"
                && event["role"] == "worker"
                && event["outcome"] == "completed"
        })
        .ok_or("feature Work has no completed worker artifact")?;
    let report_bytes = run::artifact::read_exact(
        &feature_loaded,
        &worker["artifact"],
        "quarantine verification plan",
        256 * 1024,
    )?;
    let report: Value = serde_json::from_slice(&report_bytes)
        .map_err(|_| "feature worker artifact is not valid JSON")?;
    let plan = &report["plan"];
    if report["kind"] != "quarantine-verification-plan"
        || report["protocol"] != 1
        || !plan.is_object()
    {
        return Err(
            "feature worker artifact does not contain the required quarantine verification plan"
                .into(),
        );
    }
    let plan_artifact_sha = plan["artifactSha256"]
        .as_str()
        .ok_or("verification artifact hash is missing")?;
    let artifact_meta = run::artifact::evidence(loaded, options.artifact_root, options.artifact)?;
    let artifact_bytes = exact_artifact_bytes(loaded, &artifact_meta, options.artifact)?;
    std::str::from_utf8(&artifact_bytes).map_err(|_| "verification artifact must be UTF-8")?;
    let artifact_sha = hash::bytes(&artifact_bytes);
    if artifact_sha != plan_artifact_sha {
        return Err("verification artifact bytes differ from the accepted feature plan".into());
    }
    let inputs_sha = run::inputs::fingerprint(loaded)?;
    if plan["inputsSha256"].as_str() != Some(inputs_sha.as_str()) {
        return Err("verification plan inputs are no longer current".into());
    }
    let command = plan["command"]
        .as_str()
        .ok_or("quarantine verification plan field verification.operation must be a string")?;
    run::quarantine::validate_operation(command).map_err(|error| {
        format!("quarantine verification plan field verification.operation {error}")
    })?;
    let environment = plan
        .get("environment")
        .filter(|value| value.is_object())
        .ok_or("verification environment must be an object")?;
    let effects = plan
        .get("effectInventory")
        .filter(|value| value.is_object())
        .ok_or("verification effect inventory must be an object")?;

    let governor = run::carry::accounting(&old_state, &old_start["governor"])?;
    let usage = crate::session_goal::usage::details_for_work(loaded, original_work);
    let predecessor = json!({
        "work": original_work, "ledger": old_ledger, "runId": old_start["runId"],
        "configSha256": old_start["configSha256"], "startEventSha256": old_start["eventSha256"],
        "headEventSha256": old_head["eventSha256"], "ledgerSha256": hash::bytes(old_bytes.as_bytes()),
        "observationEventSha256": failure["eventSha256"], "admissionEventSha256": admission["eventSha256"],
        "accounting": {"governor": governor, "usageAccount": usage}
    });
    let review = json!({
        "work": options.feature_work,
        "startEventSha256": feature_start["eventSha256"], "headEventSha256": feature_head["eventSha256"],
        "subjectSha256": feature_state["subject"]["sha256"], "checkEventSha256": check["eventSha256"],
        "reviewEventSha256": reviewer["eventSha256"], "acceptanceEventSha256": accepted["eventSha256"],
        "planSha256": hash::value(plan), "effectInventorySha256": hash::value(effects),
        "environmentSha256": hash::value(environment)
    });
    let mut marker = json!({
        "protocol": if followup { run::quarantine::FOLLOWUP_PROTOCOL } else { run::quarantine::PROTOCOL }, "requestId": options.request_id, "reason": options.reason,
        "leadOwner": lead, "predecessor": predecessor,
        "verification": {
            "artifactSha256": artifact_sha, "inputsSha256": inputs_sha,
            "commandSha256": hash::text(command), "configSha256": hash::text(&loaded.source),
            "environmentSha256": hash::value(environment), "operation": command,
            "isolationReview": review
        },
        "allowance": {"checks": 1, "workerMutations": 0, "replans": 0, "replacements": 0},
        "oldCredited": false, "oldReset": false
    });
    if followup {
        let check = requested_events
            .iter()
            .find(|event| {
                event["action"] == "check"
                    && event["eventSha256"] == requested_state["checks"][0]["eventSha256"]
            })
            .ok_or("first verification check event is missing")?;
        let custody_evidence = plan
            .get("custodyEvidence")
            .ok_or("accepted follow-up plan must bind the retained custody evidence")?;
        marker["followUp"] = json!({
            "protocol":run::quarantine::PROTOCOL,"work":old_work,"ledger":requested_ledger,
            "runId":requested_start["runId"],"startEventSha256":requested_start["eventSha256"],
            "headEventSha256":requested_events.last().unwrap()["eventSha256"],
            "ledgerSha256":hash::bytes(requested_bytes.as_bytes()),
            "decisionSha256":requested_start["quarantineVerification"]["decisionSha256"],
            "checkEventSha256":check["eventSha256"],
            "custodyEvidence":custody_evidence
        });
    }
    let decision_sha = hash::value(&marker);
    marker["decisionSha256"] = json!(decision_sha);
    let verified = run::quarantine::admission::prepare(loaded, loaded, &feature_loaded, &marker)?;
    let owner_goal_id = owner_record["goalId"]
        .as_str()
        .ok_or("native Goal identity is missing")?;
    ensure_pending_goal(
        loaded,
        &owner_loaded,
        &owner_sha256,
        owner_goal_id,
        &verified,
    )?;

    let argv = apply_argv(
        old_work,
        &options,
        loaded,
        owner_path,
        &owner_sha256,
        followup,
    );
    if !options.apply {
        return Ok(
            json!({"work": old_work, "verificationWork": verified.work(), "preview": true,
            "decisionSha256": marker["decisionSha256"],
            "ownerConfigPath": owner_loaded.path, "ownerConfigSha256": owner_sha256,
            "apply": argv.argv,
            "sameConfigRequired": argv.same_config, "sameExecutableRequired": argv.same_executable,
            "effect": "no-change"}),
        );
    }
    let started = run::quarantine::admission::start(
        loaded,
        loaded,
        &feature_loaded,
        &verified,
        |work, intent| {
            ensure_pending_goal(
                loaded,
                &owner_loaded,
                &owner_sha256,
                owner_goal_id,
                &verified,
            )?;
            let current_scope = crate::session_goal::native::scope(&owner_loaded, false)?;
            current_scope.require_owner()?;
            let record = crate::session_goal::read(&current_scope.root)?
                .ok_or("native session Goal disappeared before quarantine admission")?;
            let prior = crate::session_goal::journey::find_admission(&record, work)?;
            if prior.is_some() {
                return Ok(());
            }
            let goal = crate::session_goal::association::prepare(&current_scope)?
                .ok_or("quarantine Work requires an open native session Goal")?;
            crate::session_goal::association::admit(
                &current_scope.root,
                loaded,
                &goal,
                work,
                intent,
                verified.goal(),
            )
            .map(|_| ())
        },
    )?;
    match crate::session_goal::association::link_session(loaded, verified.work()) {
        Ok(link) => Ok(
            json!({"work": verified.work(), "started": started, "sessionLink": link, "effect": "started"}),
        ),
        Err(error) => Ok(json!({"work": verified.work(), "started": started,
            "sessionLink": {"error": error, "recovery": format!("exitbind work link-session {} --json", verified.work())},
            "effect": "started-link-pending"})),
    }
}

fn configured_lead(plan: &Value) -> Result<Value, String> {
    let agents = plan["stages"]
        .as_array()
        .into_iter()
        .flatten()
        .flat_map(|stage| stage["agents"].as_array().into_iter().flatten())
        .filter(|agent| agent["role"] == "lead")
        .filter_map(|agent| Some((agent["name"].as_str()?, agent["nativeTaskName"].as_str()?)))
        .collect::<std::collections::BTreeSet<_>>();
    if agents.len() != 1 {
        return Err("old Work must have exactly one configured Lead owner".into());
    }
    let (agent, native_task_name) = agents
        .into_iter()
        .next()
        .ok_or("old Work Lead owner is missing")?;
    if native_task_name.trim().is_empty() {
        return Err("configured Lead native task name is missing".into());
    }
    Ok(json!({"agent": agent, "nativeTaskName": native_task_name}))
}

fn ensure_no_children(loaded: &Loaded, work: &str) -> Result<(), String> {
    let prepared = crate::session_goal::prepared_children_for_work(loaded, work);
    if prepared["error"].is_string() {
        return Err("prepared child state is unreadable; quarantine is blocked".into());
    }
    if prepared.as_array().into_iter().flatten().any(|child| {
        !matches!(
            child["state"].as_str(),
            Some("recorded" | "failed" | "abandoned" | "superseded")
        ) || child["retainedResultBytes"]
            .as_u64()
            .is_some_and(|bytes| bytes > 0)
    }) {
        return Err("quarantine is blocked by a live, prepared, or retained child".into());
    }
    Ok(())
}

fn ensure_pending_goal(
    loaded: &Loaded,
    owner_loaded: &Loaded,
    owner_sha256: &str,
    owner_goal_id: &str,
    verified: &run::quarantine::admission::Verified,
) -> Result<(), String> {
    let owner_config_path = owner_loaded
        .path
        .to_str()
        .ok_or("native Goal owner configuration path is not valid UTF-8")?;
    let current = crate::config::load(Some(owner_config_path))?;
    let config_path = loaded
        .path
        .to_str()
        .ok_or("configuration path is not valid UTF-8")?;
    let product_root = loaded
        .product_root
        .to_str()
        .ok_or("product root path is not valid UTF-8")?;
    let state_root = loaded
        .state_root
        .to_str()
        .ok_or("state root path is not valid UTF-8")?;
    if current.source != owner_loaded.source
        || hash::text(&current.source) != owner_sha256
        || std::fs::read_to_string(&current.path).map_err(|e| e.to_string())? != current.source
    {
        return Err("native Goal owner configuration changed after quarantine review".into());
    }
    let scope = crate::session_goal::native::scope(&current, false)?;
    scope.require_owner()?;
    let Some(record) = crate::session_goal::read(&scope.root)? else {
        return Err("fixed native Goal journal disappeared before quarantine admission".into());
    };
    if record["goalId"].as_str() != Some(owner_goal_id) {
        return Err("native Goal changed after quarantine review".into());
    }
    let existing = crate::session_goal::journey::find_admission(&record, verified.work())?;
    if let Some(admission) = existing {
        let admissions = record["journeyAdmissions"]
            .as_array()
            .ok_or("native Goal admission list is unreadable")?;
        if scope.session.is_none()
            || scope.owner.is_none()
            || !scope.owner_matches
            || admissions.len() != 1
            || admission.start_intent_sha256 != verified.start_intent_sha256()
            || admission.work_goal_sha256 != hash::text(verified.goal())
            || record["goalId"].as_str() != Some(admission.parent_goal_id.as_str())
            || admission.host
                != scope
                    .session
                    .as_ref()
                    .map(|value| value.host)
                    .unwrap_or_default()
            || admission.session_key
                != scope
                    .session
                    .as_ref()
                    .map(|value| value.key.as_str())
                    .unwrap_or_default()
            || admission.owner_identity_sha256
                != scope
                    .owner
                    .as_ref()
                    .map(|value| value.identity_sha256.as_str())
                    .unwrap_or_default()
            || admission.owner_project_id
                != scope
                    .owner
                    .as_ref()
                    .and_then(|value| value.project_id.clone())
            || admission.config_path != config_path
            || admission.config_sha256 != hash::text(&loaded.source)
            || admission.product_root != product_root
            || admission.state_root != state_root
        {
            return Err("an unresolved native Goal admission conflicts with the exact quarantine start; inspect before recovery".into());
        }
        return Ok(());
    }
    crate::session_goal::journey::require_resolved_replacement_admissions(&record)
}

fn exact_artifact_bytes(
    loaded: &Loaded,
    evidence: &Value,
    requested: &str,
) -> Result<Vec<u8>, String> {
    let bytes = match evidence["root"].as_str() {
        Some("state") => {
            run::artifact::read_exact(loaded, evidence, "verification artifact", 16 * 1024 * 1024)?
        }
        Some("product") => match crate::project::path::secure_bytes_observation_bounded(
            &loaded.product_root,
            requested,
            "verification artifact",
            16 * 1024 * 1024,
        ) {
            crate::project::path::SecureBytesResult::Bytes(bytes) => bytes,
            crate::project::path::SecureBytesResult::Absent(error)
            | crate::project::path::SecureBytesResult::Unsafe(error)
            | crate::project::path::SecureBytesResult::Unreadable(error) => return Err(error),
            #[cfg(not(unix))]
            crate::project::path::SecureBytesResult::Unsupported(error) => return Err(error),
        },
        _ => return Err("verification artifact root is invalid".into()),
    };
    if hash::bytes(&bytes) != evidence["sha256"] {
        return Err("verification artifact changed while reading".into());
    }
    Ok(bytes)
}

fn same_roots(a: &Loaded, b: &Loaded) -> Result<(), String> {
    if a.product_root != b.product_root
        || a.state_root != b.state_root
        || a.control_root != b.control_root
        || a.project_id != b.project_id
    {
        return Err("feature Work must use the same product and state roots".into());
    }
    Ok(())
}

fn apply_argv(
    old_work: &str,
    options: &Options<'_>,
    loaded: &Loaded,
    owner_config: &str,
    owner_config_sha256: &str,
    followup: bool,
) -> crate::work::response_recovery::RecoveryCommand {
    let mut args = vec![
        "work".into(),
        if followup {
            "quarantine-followup"
        } else {
            "quarantine"
        }
        .into(),
        old_work.into(),
        "--feature-config".into(),
        options.feature_config.into(),
        "--feature-work".into(),
        options.feature_work.into(),
        "--artifact-root".into(),
        options.artifact_root.unwrap_or("product").into(),
        "--artifact".into(),
        options.artifact.into(),
        "--reason".into(),
        options.reason.into(),
        "--request-id".into(),
        options.request_id.into(),
    ];
    if let Some(binding) = options.current_binding {
        args.extend(["--current-binding".into(), binding.into()]);
    }
    args.extend([
        "--owner-config".into(),
        owner_config.into(),
        "--owner-config-sha256".into(),
        owner_config_sha256.into(),
    ]);
    args.extend(["--apply".into(), "--json".into(), "--config".into()]);
    crate::work::response_recovery::bounded_argv(
        args,
        loaded.path.to_str(),
        crate::work::compact::MAX_RESPONSE_BYTES - 1024,
    )
}

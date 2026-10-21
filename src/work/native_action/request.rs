use super::{result, Options, ReviewEvidence};
use crate::config::Loaded;
use crate::evidence::hash;
use crate::host::codex_exec::{self, Request};
use crate::project::{agent_context, path as project_path};
use serde_json::Value;
use std::path::Path;
use std::time::Duration;

const DEFAULT_TIMEOUT_MS: u64 = 1_800_000;
const MAX_PROFILE_BYTES: usize = 64 * 1024;

#[allow(clippy::too_many_arguments)]
pub(super) fn build(
    loaded: &Loaded,
    options: Options<'_>,
    role: &str,
    work: &str,
    current: &Value,
    assignment: &Value,
    packet: String,
    profile: String,
    review_evidence: Option<&Vec<ReviewEvidence>>,
    schema_path: &Path,
    thread_id: Option<&str>,
) -> Result<Request, String> {
    let (host, model, effort) = effective_binding(loaded, role, assignment, &options)?;
    if host != "codex" {
        return Err("native Codex action requires runtime.host=codex".into());
    }
    let timeout = options
        .timeout_ms
        .map(|value| {
            value
                .parse::<u64>()
                .map_err(|_| "timeout must be an integer".to_owned())
        })
        .transpose()?
        .unwrap_or(DEFAULT_TIMEOUT_MS);
    if timeout == 0 {
        return Err("timeout must be positive".into());
    }
    let executable = codex_exec::resolve_codex(options.codex_bin.map(Path::new))
        .map_err(|error| error.to_string())?;
    if thread_id.is_some() && options.sandbox_mode.is_some() {
        return Err(
            "native resume cannot apply --sandbox; omit --sandbox for the persisted session or start a fresh assignment"
                .into(),
        );
    }
    if options.controlled_effects
        && (role != "worker"
            || thread_id.is_some()
            || options.sandbox_mode.is_some_and(|s| s != "read-only"))
    {
        return Err(
            "controlled effects require a fresh read-only worker; sandbox overrides are refused"
                .into(),
        );
    }
    let sandbox = if options.controlled_effects {
        Some("read-only".to_owned())
    } else if thread_id.is_some() {
        None
    } else {
        Some(
            options
                .sandbox_mode
                .map(str::to_owned)
                .unwrap_or_else(|| match role {
                    "worker" => "workspace-write".to_owned(),
                    _ => "read-only".to_owned(),
                }),
        )
    };
    let context = agent_context::project_parts(
        loaded,
        work,
        current,
        agent_context::RequestedBinding {
            host: "codex",
            model: model.as_deref(),
            reasoning_effort: effort.as_deref(),
            sandbox: sandbox.as_deref(),
        },
    )?;
    let evidence_route = if let Some(evidence) = review_evidence {
        let mut route = String::from("Use every verified current worker result below. The evidenceReferences array must contain every listed ref token in order; put file lines and check observations in summary. The exact originals are already included in CURRENT READABLE ASSIGNMENT, EVIDENCE AND TASKS; use CURRENT SCOPED READ ROUTES only for a later refresh; surrounding ledger references use that same executable/config with work expand. Do not reconstruct upstream artifacts from git or summaries. For unavailable, reason must be provider_quota, rate_limit, or provider_unavailable; for rework use review_finding; for blocked use blocked; for approved use an empty reason.\n");
        for item in evidence {
            route.push_str(&format!(
                "\nVERIFIED WORKER RESULT (reference {}, sha256 {}):\n{}\n",
                item.reference, item.sha256, "included in readable evidence below"
            ));
        }
        route
    } else if role == "reviewer" {
        return Err("reviewer evidence route is unavailable".into());
    } else {
        "Use the exact declared upstream evidence included below. Do not reconstruct it from git or summaries.".to_owned()
    };
    // Keep the managed-edit instructions stable across assignments.  The
    // selected path is volatile and is carried once below, so changing the
    // current tool location does not duplicate a long command-shaped prefix.
    let mut controlled_tool = None;
    let mediator_route = if role == "worker" {
        let current_assignment = current["assignment"]
            .as_str()
            .ok_or("native assignment has no assignment identity")?;
        let prepared = crate::work::managed_edit::prepare(loaded, work, current_assignment)?;
        let tool = prepared["tool"]
            .as_str()
            .ok_or("managed edit tool unavailable")?;
        if options.controlled_effects {
            let permit =
                crate::work::managed_edit::controlled_permit(loaded, work, current_assignment)?;
            controlled_tool = Some(codex_exec::ControlledTool {
                executable: crate::emitted::executable()?,
                session: prepared["session"]
                    .as_str()
                    .ok_or("effect adapter session unavailable")?
                    .into(),
                config: loaded
                    .path
                    .to_str()
                    .ok_or("effect adapter config path is not UTF-8")?
                    .into(),
                protected_roots: vec![
                    loaded.product_root.clone(),
                    loaded.control_root.clone(),
                    loaded
                        .state_root
                        .join(crate::project::layout_types::state_namespace()),
                    std::env::current_exe().map_err(|e| e.to_string())?,
                    std::env::current_exe()
                        .map_err(|e| e.to_string())?
                        .parent()
                        .ok_or("effect executable has no parent")?
                        .to_path_buf(),
                ],
            });
            (controlled_managed_edit_guidance(&permit), String::new())
        } else {
            managed_edit_guidance(tool)
        }
    } else {
        (String::new(), String::new())
    };
    let role_guidance =
        native_lifecycle_guidance(role, options.controlled_effects, sandbox.as_deref());
    let prompt = format!(
        "You are the native Codex {role} for one governed Exitbind assignment. Follow the supplied profile and verified assignment. Work only within the declared boundary. This packet is already bound; a continuation lookup is unnecessary. Required assignment, evidence and goal/task conditions are resolved and verified below before this launch. No paging, hex decoding, checksum or JSON-join code is needed. {result_text}\n{role_guidance}\n\nPROFILE BYTES (reviewed role guidance; exact bytes):\n{profile}\n\n{mediator_instructions}\nCURRENT STABLE PROJECT RULES (verified complete current bytes):\n{stable}\nCURRENT VOLATILE NATIVE PROJECT CONTEXT (verified immediately before launch):\n{tool_binding}{volatile}\n\n{evidence_route}\nCURRENT READABLE ASSIGNMENT, EVIDENCE AND TASKS (canonical packet SHA-256 {canonical_sha}; context.digest belongs to the full canonical context; equal aliases are described by projection):\n{packet}\n\nCURRENT SCOPED READ ROUTES (read only; current binding):\n{current_binding}\n",
        result_text = result::TEXT_GUIDANCE,
        role_guidance = role_guidance,
        canonical_sha = hash::value(assignment),
        current_binding = current["current"],
        mediator_instructions = mediator_route.0,
        tool_binding = mediator_route.1,
        stable = context.stable,
        volatile = context.volatile,
    );
    Ok(Request {
        executable,
        cwd: loaded.product_root.clone(),
        prompt,
        model,
        effort,
        sandbox,
        output_schema: Some(schema_path.to_path_buf()),
        resume_thread_id: thread_id.map(str::to_owned),
        persist_session: !options.controlled_effects,
        controlled_tool,
        timeout: Duration::from_millis(timeout),
    })
}

fn effective_binding(
    loaded: &Loaded,
    role: &str,
    assignment: &Value,
    options: &Options<'_>,
) -> Result<(String, Option<String>, Option<String>), String> {
    let marked_adviser = role == "adviser"
        && assignment["guidanceContinuityProtocol"].as_u64()
            == Some(crate::run::guidance_continuity::PROTOCOL);
    let floor_configured = !loaded.config["project"]["adviserFloor"].is_null();
    if marked_adviser && !floor_configured {
        // Projects created before the floor existed keep their configured
        // adviser; the owner opts into enforcement by adding the floor.
        eprintln!(
            "notice: no project.adviserFloor is configured, so the adviser capability floor is not enforced; add project.adviserFloor to require an approved host/model/effort"
        );
    }
    if marked_adviser && floor_configured {
        current_adviser_binding(loaded, assignment, options)
    } else {
        Ok((
            assignment["runtime"]["host"]
                .as_str()
                .unwrap_or("codex")
                .to_owned(),
            options
                .model
                .map(str::to_owned)
                .or_else(|| assignment["runtime"]["model"].as_str().map(str::to_owned)),
            options.reasoning_effort.map(str::to_owned).or_else(|| {
                assignment["runtime"]["reasoningEffort"]
                    .as_str()
                    .map(str::to_owned)
            }),
        ))
    }
}

fn current_adviser_binding(
    loaded: &Loaded,
    assignment: &Value,
    options: &Options<'_>,
) -> Result<(String, Option<String>, Option<String>), String> {
    let agent_id = assignment["agent"]
        .as_str()
        .ok_or("marked adviser assignment has no configured agent")?;
    if !loaded.roles_for(agent_id).contains("adviser") {
        return Err("marked adviser is no longer configured for the adviser role".into());
    }
    let configured = loaded
        .agent(agent_id)
        .ok_or("marked adviser is no longer configured")?;
    let host = configured
        .runtime
        .host
        .as_deref()
        .unwrap_or("codex")
        .to_owned();
    if host != "codex" {
        return Err("native work act supports the Codex adviser host only".into());
    }
    let model = options
        .model
        .map(str::to_owned)
        .or_else(|| configured.runtime.model.clone());
    let effort = options
        .reasoning_effort
        .map(str::to_owned)
        .or_else(|| configured.runtime.reasoning_effort.clone());
    let model_ref = model
        .as_deref()
        .ok_or("marked adviser model is unresolved")?;
    let effort_ref = effort
        .as_deref()
        .ok_or("marked adviser reasoning effort is unresolved")?;
    let floor = &loaded.config["project"]["adviserFloor"];
    let binding = floor["bindings"]
        .as_array()
        .and_then(|bindings| {
            bindings.iter().find(|binding| {
                binding["host"] == host
                    && binding["model"] == model_ref
                    && binding["reasoningEffort"] == effort_ref
            })
        })
        .ok_or("adviser runtime is outside the approved host/model/effort floor")?;
    let required = binding["requiredCapabilities"]
        .as_array()
        .ok_or("adviser floor requiredCapabilities is malformed")?;
    if !required.is_empty() {
        let delivered = crate::project::capabilities::delivery(loaded, assignment, &host)?;
        let available = delivered
            .as_ref()
            .and_then(|value| value["items"].as_array())
            .into_iter()
            .flatten()
            .filter_map(|item| item["id"].as_str().map(str::to_owned))
            .collect::<std::collections::BTreeSet<_>>();
        validate_adviser_candidate(floor, &host, model_ref, effort_ref, &available)?;
    } else {
        validate_adviser_candidate(
            floor,
            &host,
            model_ref,
            effort_ref,
            &std::collections::BTreeSet::new(),
        )?;
    }
    Ok((host, model, effort))
}

pub(crate) fn validate_adviser_candidate(
    floor: &Value,
    host: &str,
    model: &str,
    effort: &str,
    available_capabilities: &std::collections::BTreeSet<String>,
) -> Result<(), String> {
    let binding = floor["bindings"]
        .as_array()
        .and_then(|bindings| {
            bindings.iter().find(|binding| {
                binding["host"] == host
                    && binding["model"] == model
                    && binding["reasoningEffort"] == effort
            })
        })
        .ok_or("adviser runtime is outside the approved host/model/effort floor")?;
    let required = binding["requiredCapabilities"]
        .as_array()
        .ok_or("adviser floor requiredCapabilities is malformed")?;
    if required.iter().any(|id| {
        id.as_str()
            .map_or(true, |id| !available_capabilities.contains(id))
    }) {
        return Err(
            "adviser floor requires a capability unavailable to this agent/task/host".into(),
        );
    }
    Ok(())
}

fn native_lifecycle_guidance(
    role: &str,
    controlled_effects: bool,
    sandbox: Option<&str>,
) -> String {
    let mut guidance = String::from(
        "Return only the schema-valid final JSON object required for your role. The parent host submits this result and owns subsequent Work lifecycle, including check and acceptance progression. Embedded action forms are context and read-only navigation; do not use them to return or submit this child result, run the parent check, replan, or record parent acceptance. Supported read-only inspection remains allowed. Report uncertain effects as unknown, preserve retained results, and report real failures truthfully. Implementation completion may leave parent checks outstanding; this child result is not a Work check, separate review, Lead acceptance, or parent completion.",
    );
    if controlled_effects {
        guidance.push_str(" A controlled read-only Worker uses only the host pre-issued current grant through the managed file MCP tool; do not request or execute a new permit in the child. Checks requiring temporary writes stay on the existing parent check route, with no child filesystem permission or exemption from final checks.");
    } else if role == "worker" && sandbox == Some("workspace-write") {
        guidance.push_str(" An ordinary workspace-write Worker retains the existing governor-authorized `work permit` route where offered for implementation changes; this does not transfer result submission or parent check and acceptance progression to the child.");
    } else if sandbox == Some("read-only") {
        guidance.push_str(" This read-only child leaves checks requiring temporary writes to the existing parent check route; this grants no child filesystem permission and does not exempt the task from final checks.");
    } else if sandbox.is_none() {
        guidance.push_str(" A resumed session retains its host-selected existing rights; this guidance neither grants nor revokes Work authority.");
    }
    guidance
}

fn managed_snapshot_guidance() -> &'static str {
    "A read captures the original baseline; repeating read may continue to return that baseline after a successful edit. Use the edit receipt, current inspect digest, or a read-only Source observation allowed by the assignment to distinguish the current effect. Refresh only for an intentional next edit, never to resolve uncertainty."
}

fn controlled_managed_edit_guidance(permit: &str) -> String {
    format!("The host verified an already-issued current unconsumed worker-mutation grant for this assignment, governor event SHA-256 {permit}. Use that exact current grant through the exitbind-file MCP file tool; it revalidates grant identity under the Work lock for each effect. `replan_required` or `evidence_required` governs the next new mutation request and does not revoke this present grant. Any retained worker result takes priority over editing; the current recovery form must resolve it first. Worker completion still requires the existing ready-governor state. Your shell cannot issue permits. Action read captures PATH, action edit replaces PATH with full UTF-8 content; repeat exactly to recover a lost reply. {} Supply only action, path and (for edit) content. Direct shell and apply_patch writes are read-only. This file effect does not submit your result or complete checks/review.", managed_snapshot_guidance())
}

fn managed_edit_guidance(tool_path: &str) -> (String, String) {
    let instructions = format!("Use the supplied managed file tool (`TOOL` in the current binding; quote its path). `TOOL read PATH` captures UTF-8 or absence; prepare edits from it. `TOOL edit PATH` uses full replacement content as UTF-8 bytes on stdin; do not truncate. Retry the same command and content only. `TOOL refresh PATH` captures a new baseline only for a deliberate next edit. On conflict or uncertainty use `TOOL inspect PATH`; never replay the write blindly. Hashes, assignment details and retry identity are product-owned. Edits do not submit results or complete checks/review. Native tools remain host-controlled; this grants no OS sandbox permission. {}\n", managed_snapshot_guidance());
    (instructions, format!("MANAGED FILE TOOL: {tool_path}\n"))
}

pub(super) fn profile_bytes(
    loaded: &Loaded,
    agent: &str,
    assignment: &Value,
) -> Result<String, String> {
    let configured = loaded
        .agent(agent)
        .ok_or_else(|| format!("configured agent '{agent}' is unavailable"))?;
    let bytes = project_path::secure_bytes(&loaded.control_root, &configured.profile, "profile")?;
    if bytes.len() > MAX_PROFILE_BYTES {
        return Err("native profile exceeds its bound".into());
    }
    let expected = assignment["profile"]["sha256"]
        .as_str()
        .ok_or("native assignment profile hash is unavailable")?;
    let current = hash::bytes(&bytes);
    if current != expected {
        return Err("native assignment profile drifted before launch".into());
    }
    String::from_utf8(bytes).map_err(|_| "native profile is not UTF-8".into())
}

#[cfg(test)]
mod tests {
    use super::{
        build, controlled_managed_edit_guidance, current_adviser_binding, effective_binding,
        managed_edit_guidance, native_lifecycle_guidance, validate_adviser_candidate, Options,
    };
    use crate::config::{types, Loaded};
    use serde_json::json;
    use std::{collections::BTreeSet, path::PathBuf};

    fn adviser_loaded(runtime: serde_json::Value) -> Loaded {
        let config = json!({
            "project":{"adviserFloor":{"version":1,"bindings":[{
                "host":"codex","model":"gpt-approved","reasoningEffort":"xhigh",
                "requiredCapabilities":[]
            }]}},
            "agents":{"adviser":{"profile":"profile.md","purpose":"advise",
                "retention":"task","crossContext":"none","runtime":runtime}},
            "workflows":{"change":{"advisers":["adviser"]}}
        });
        Loaded {
            agents: types::from_validated_agents(&config["agents"]).unwrap(),
            workflows: serde_json::from_value(config["workflows"].clone()).unwrap(),
            config,
            lead: None,
            architecture_contract: None,
            capabilities: Vec::new(),
            verification_preparation: None,
            path: PathBuf::from("/control/exitbind.json"),
            control_root: PathBuf::from("/control"),
            product_root: PathBuf::from("/product"),
            state_root: PathBuf::from("/state"),
            mode: crate::project::layout_types::Mode::Portable,
            project_id: None,
            source: String::new(),
        }
    }

    fn options<'a>(model: Option<&'a str>, reasoning_effort: Option<&'a str>) -> Options<'a> {
        Options {
            codex_bin: None,
            model,
            reasoning_effort,
            sandbox_mode: None,
            timeout_ms: None,
            resume: false,
            controlled_effects: false,
        }
    }

    #[test]
    fn managed_edit_guidance_keeps_syntax_fixed_and_path_singleton() {
        let path = "/tmp/managed edit tool";
        let (fixed, binding) = managed_edit_guidance(path);
        let guidance = fixed + &binding;
        assert_eq!(guidance.matches(path).count(), 1);
        for command in ["read PATH", "edit PATH", "refresh PATH", "inspect PATH"] {
            assert!(guidance.contains(command), "missing {command}");
        }
        assert!(guidance.contains("same command and content"));
        assert!(guidance.contains("deliberate next edit"));
        assert!(guidance.contains("original baseline"));
        assert!(guidance.contains("current inspect digest"));
        assert!(guidance.contains("never to resolve uncertainty"));
    }

    #[test]
    fn native_lifecycle_guidance_preserves_parent_ownership_and_worker_permits() {
        let ordinary_worker = native_lifecycle_guidance("worker", false, Some("workspace-write"));
        assert!(ordinary_worker.contains("parent host submits this result"));
        assert!(ordinary_worker.contains("do not use them to return or submit this child result"));
        assert!(
            ordinary_worker.contains("run the parent check, replan, or record parent acceptance")
        );
        assert!(ordinary_worker.contains("existing governor-authorized `work permit` route"));

        let controlled_worker = native_lifecycle_guidance("worker", true, Some("read-only"));
        assert!(controlled_worker.contains("host pre-issued current grant"));
        assert!(controlled_worker.contains("do not request or execute a new permit in the child"));
        assert!(controlled_worker
            .contains("Checks requiring temporary writes stay on the existing parent check route"));
        assert!(controlled_worker.contains("no child filesystem permission"));

        for role in ["reviewer", "adviser"] {
            let read_only_child = native_lifecycle_guidance(role, false, Some("read-only"));
            assert!(read_only_child.contains("schema-valid final JSON"));
            assert!(read_only_child.contains("parent host submits this result"));
            assert!(read_only_child.contains("Supported read-only inspection remains allowed"));
            assert!(read_only_child.contains(
                "not a Work check, separate review, Lead acceptance, or parent completion"
            ));
            assert!(read_only_child
                .contains("checks requiring temporary writes to the existing parent check route"));
            assert!(!read_only_child.contains("work permit"));
        }
    }

    #[test]
    fn marked_adviser_candidate_requires_an_exact_approved_binding_and_capabilities() {
        let floor = json!({"version":1,"bindings":[{
            "host":"codex","model":"gpt-approved","reasoningEffort":"xhigh",
            "requiredCapabilities":["review-lens"]
        }]});
        let available = ["review-lens".to_owned()].into_iter().collect();
        assert!(
            validate_adviser_candidate(&floor, "codex", "gpt-approved", "xhigh", &available)
                .is_ok()
        );
        for (host, model, effort) in [
            ("claude", "gpt-approved", "xhigh"),
            ("codex", "gpt-unknown", "xhigh"),
            ("codex", "gpt-approved", "medium"),
        ] {
            assert!(validate_adviser_candidate(&floor, host, model, effort, &available).is_err());
        }
        assert!(validate_adviser_candidate(
            &floor,
            "codex",
            "gpt-approved",
            "xhigh",
            &BTreeSet::new()
        )
        .unwrap_err()
        .contains("capability unavailable"));
    }

    #[test]
    fn marked_work_act_rejects_unapproved_adviser_before_request_or_provider_admission() {
        let loaded = adviser_loaded(json!({
            "host":"codex","model":"gpt-unresolved","reasoningEffort":"medium"
        }));
        let assignment = json!({
            "agent":"adviser","role":"adviser","guidanceContinuityProtocol":1,
            "runtime":{"host":"codex","model":"legacy-model","reasoningEffort":"low"},
            "goal":"review the change"
        });
        let error = build(
            &loaded,
            options(None, None),
            "adviser",
            "work",
            &json!({}),
            &assignment,
            String::new(),
            String::new(),
            None,
            std::path::Path::new("/unreached/schema.json"),
            None,
        )
        .unwrap_err();
        assert!(error.contains("outside the approved host/model/effort floor"));
    }

    #[test]
    fn marked_work_act_uses_only_an_exact_current_allowed_adviser_binding() {
        let loaded = adviser_loaded(json!({
            "host":"codex","model":"gpt-approved","reasoningEffort":"xhigh"
        }));
        let assignment = json!({"agent":"adviser","guidanceContinuityProtocol":1});
        let (host, model, effort) =
            current_adviser_binding(&loaded, &assignment, &options(None, None)).unwrap();
        assert_eq!(
            (host.as_str(), model.as_deref(), effort.as_deref()),
            ("codex", Some("gpt-approved"), Some("xhigh"))
        );

        for options in [
            options(Some("gpt-unlisted"), None),
            options(None, Some("medium")),
        ] {
            assert!(current_adviser_binding(&loaded, &assignment, &options)
                .unwrap_err()
                .contains("outside the approved host/model/effort floor"));
        }
    }

    #[test]
    fn unmarked_adviser_launch_keeps_the_legacy_assignment_runtime() {
        let loaded = adviser_loaded(json!({
            "host":"codex","model":"new-config-value","reasoningEffort":"max"
        }));
        let assignment = json!({"agent":"adviser","role":"adviser",
            "runtime":{"host":"codex","model":"frozen-legacy","reasoningEffort":"low"}});
        let (host, model, effort) =
            effective_binding(&loaded, "adviser", &assignment, &options(None, None)).unwrap();
        assert_eq!(
            (host.as_str(), model.as_deref(), effort.as_deref()),
            ("codex", Some("frozen-legacy"), Some("low"))
        );
    }

    #[test]
    fn controlled_and_ordinary_edit_guidance_distinguish_baseline_from_effect() {
        let path = "/tmp/current managed tool";
        let (ordinary, ordinary_binding) = managed_edit_guidance(path);
        let controlled = controlled_managed_edit_guidance("permit-event");
        for guidance in [&ordinary, &controlled] {
            for wording in [
                "original baseline",
                "repeating read may continue to return that baseline",
                "successful edit",
                "edit receipt",
                "current inspect digest",
                "read-only Source observation allowed by the assignment",
                "Refresh only for an intentional next edit, never to resolve uncertainty",
            ] {
                assert!(guidance.contains(wording), "missing {wording}");
            }
        }
        assert_eq!(ordinary_binding.matches(path).count(), 1);
        assert!(!controlled.contains(path));
        assert!(controlled.contains("already-issued current unconsumed worker-mutation grant"));
        assert!(controlled.contains("revalidates grant identity under the Work lock"));
        assert!(controlled.contains("Your shell cannot issue permits"));
    }

    #[test]
    fn matched_guidance_fixture_reduces_repeated_tool_path() {
        let path =
            "/tmp/current project/native-actions/current-work/current-assignment/managed file tool";
        let tool = crate::presentation::shell_quote(path);
        // Exact accepted R23 template, with identical current path and
        // behavior obligations on both sides; bytes are not provider tokens.
        let baseline = format!(
            " For supported project edits, use the supplied managed file tool: `{tool} read PATH` returns the captured UTF-8 file or explicit absence. Prepare your edit from that content, then run `{tool} edit PATH` with full replacement content as UTF-8 bytes on stdin; do not truncate content. Retry the same submitted edit with that same command and content. For a deliberate next edit, use `{tool} refresh PATH` to capture a new baseline. On a conflict or uncertain result use `{tool} inspect PATH`; never replay the write blindly or reset uncertain state. Hashes, assignment details and retry identity are product-owned. A file edit does not submit your result or complete checks/review. Native tools remain controlled by the host and this route grants no OS sandbox permission.\nMANAGED FILE TOOL: {path}\n"
        );
        let (fixed, binding) = managed_edit_guidance(path);
        assert!(!fixed.contains(path));
        let candidate = fixed + &binding;
        assert_eq!(baseline.matches(path).count(), 5);
        assert_eq!(candidate.matches(path).count(), 1);
        assert!(candidate.len() < baseline.len());
        println!("matched managed guidance: baseline_bytes={} candidate_bytes={} path_occurrences=5->1 provider_tokens=unmeasured", baseline.len(), candidate.len());
    }
}

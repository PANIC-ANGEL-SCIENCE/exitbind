use super::{Options, ReviewEvidence};
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
    if assignment["runtime"]["host"]
        .as_str()
        .is_some_and(|host| host != "codex")
    {
        return Err("native Codex action requires runtime.host=codex".into());
    }
    let model = options
        .model
        .map(str::to_owned)
        .or_else(|| assignment["runtime"]["model"].as_str().map(str::to_owned));
    let effort = options.reasoning_effort.map(str::to_owned).or_else(|| {
        assignment["runtime"]["reasoningEffort"]
            .as_str()
            .map(str::to_owned)
    });
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
    let sandbox = if thread_id.is_some() {
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
    let context = agent_context::project(
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
        let mut route = format!("Use every verified current worker result below. The evidenceReferences array must contain every listed ref token in order; put file lines and check observations in summary. Use exitbind work expand {work} REFERENCE for surrounding ledger events. Do not reconstruct upstream artifacts from git or summaries. For unavailable, reason must be provider_quota, rate_limit, or provider_unavailable; for rework use review_finding; for blocked use blocked; for approved use an empty reason.\n");
        for item in evidence {
            route.push_str(&format!(
                "\nVERIFIED WORKER RESULT (reference {}, sha256 {}):\n{}\n",
                item.reference, item.sha256, item.content
            ));
        }
        route
    } else if role == "reviewer" {
        return Err("reviewer evidence route is unavailable".into());
    } else {
        "Use the packet's declared evidence routes when checking the assignment. Do not copy upstream artifacts into the prompt or reconstruct them from git.".to_owned()
    };
    let mediator_route = if role == "worker" {
        let current_assignment = current["assignment"]
            .as_str()
            .ok_or("native assignment has no assignment identity")?;
        let prepared = crate::work::managed_edit::prepare(loaded, work, current_assignment)?;
        let tool = prepared["tool"]
            .as_str()
            .ok_or("managed edit tool unavailable")?;
        let tool_path = tool;
        let tool = crate::presentation::shell_quote(tool);
        format!(
            " For supported project edits, use the supplied managed file tool: `{tool} read PATH` returns the captured UTF-8 file or explicit absence. Prepare your edit from that content, then run `{tool} edit PATH` with full replacement content as UTF-8 bytes on stdin; do not truncate content. Retry the same submitted edit with that same command and content. For a deliberate next edit, use `{tool} refresh PATH` to capture a new baseline. On a conflict or uncertain result use `{tool} inspect PATH`; never replay the write blindly or reset uncertain state. Hashes, assignment details and retry identity are product-owned. A file edit does not submit your result or complete checks/review. Native tools remain controlled by the host and this route grants no OS sandbox permission."
        ) + &format!("\nMANAGED FILE TOOL: {tool_path}\n")
    } else {
        String::new()
    };
    let prompt = format!(
        "You are the native Codex {role} for one governed Exitbind assignment. Follow the supplied profile and verified assignment. Work only within the declared boundary. This packet is already bound; a continuation lookup is unnecessary. {evidence_route}{mediator_route} Return only the JSON object required by the output schema; do not include markdown or commentary.\n\nPROFILE BYTES:\n{profile}\n\nCURRENT ASSIGNMENT DELIVERY PROJECTION (canonical packet SHA-256 {canonical_sha}; context.digest belongs to the full canonical context; omitted recovery goal/scope/subject/current/missing/loop/next fields equal context goal/scope/subject/evidence/obligations/loop/next respectively):\n{packet}\n\nCURRENT NATIVE PROJECT CONTEXT (verified immediately before launch):\n{context}\n",
        canonical_sha = hash::value(assignment),
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
        persist_session: true,
        timeout: Duration::from_millis(timeout),
    })
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

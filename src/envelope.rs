use crate::config::{self, Loaded};
use crate::hash;
use serde_json::{json, Value};

const BRIEF_NOTICE: &str = "This is a plan-only brief. Runtime fields are requested bindings, not a model invocation or OS sandbox. The lead remains responsible for scope and final acceptance.";
const PLAN_NOTICE: &str = "This is a deterministic plan. maxParallel is declared coordination intent only; the native host launches nothing here. Runtime fields are requested bindings only; no model was selected, invoked, or granted runtime authority by this plan.";

pub fn brief(loaded: &Loaded, agent_name: &str, task: &str) -> Result<Value, String> {
    let agent = loaded
        .agent(agent_name)
        .ok_or_else(|| format!("unknown agent '{agent_name}'"))?;
    if task.trim().is_empty() {
        return Err("--task requires a non-empty value".into());
    }
    let profile_path = config::file(&loaded.control_root, &agent.profile)?;
    let mut envelope = json!({
        "version": 1,
        "evidence": "profile-selected-and-presented",
        "agent": agent_name,
        "displayName": agent.display_name.as_deref().unwrap_or(agent_name),
        "nativeTaskName": agent.native_name(agent_name),
        "purpose": agent.purpose,
        "task": task,
        "profile": {
            "path": config::rel(&loaded.control_root, &profile_path)?,
            "sha256": hash::file(&profile_path)?,
        },
        "runtime": agent.runtime_value(),
        "declaredBoundary": agent.boundary_value(),
        "notice": BRIEF_NOTICE,
    });
    attach_memory_references(loaded, agent_name, &mut envelope)?;
    Ok(envelope)
}

pub fn plan(loaded: &Loaded, workflow_name: &str, goal: &str) -> Result<Value, String> {
    let workflow = loaded.config["workflows"]
        .get(workflow_name)
        .ok_or_else(|| format!("unknown workflow '{workflow_name}'"))?;
    if goal.trim().is_empty() {
        return Err("--goal requires a non-empty value".into());
    }
    let lead = loaded.config["orchestration"]["lead"]
        .as_str()
        .unwrap_or_default();
    let groups = [
        ("lead", vec![lead.to_owned()]),
        ("adviser", names(workflow, "advisers")),
        ("worker", names(workflow, "workers")),
        ("reviewer", names(workflow, "reviewers")),
        ("lead", vec![lead.to_owned()]),
    ];
    let mut stages = Vec::new();
    for (role, names) in groups.into_iter().filter(|(_, names)| !names.is_empty()) {
        let mut selected = Vec::new();
        for name in names {
            let agent = loaded
                .agent(&name)
                .ok_or_else(|| format!("unknown agent '{name}'"))?;
            let profile_path = config::file(&loaded.control_root, &agent.profile)?;
            let mut selected_agent = json!({
                "name": name,
                "displayName": agent.display_name.as_deref().unwrap_or(&name),
                "nativeTaskName": agent.native_name(&name),
                "role": role,
                "purpose": agent.purpose,
                "profile": config::rel(&loaded.control_root, &profile_path)?,
                "profileSha256": hash::file(&profile_path)?,
                "runtime": agent.runtime_value(),
                "declaredBoundary": agent.boundary_value(),
            });
            attach_memory_references(loaded, &name, &mut selected_agent)?;
            selected.push(selected_agent);
        }
        let stage_number = stages.len() + 1;
        let depends_on: Vec<usize> = if stage_number == 1 {
            Vec::new()
        } else {
            vec![stage_number - 1]
        };
        stages.push(json!({
            "stage": stage_number,
            "agents": selected,
            "dependsOn": depends_on,
        }));
    }
    Ok(json!({
        "version": 1,
        "evidence": "profiles-selected-for-task-plan",
        "workflow": workflow_name,
        "goal": goal,
        "lead": lead,
        "maxParallel": loaded.config["orchestration"]["maxParallel"],
        "finalVerification": "serial-under-lead",
        "stages": stages,
        "notice": PLAN_NOTICE,
    }))
}

fn attach_memory_references(
    loaded: &Loaded,
    agent_name: &str,
    envelope: &mut Value,
) -> Result<(), String> {
    if crate::memory_policy::get(&loaded.config).is_some() {
        envelope["memoryReferences"] = json!(crate::memory_selection::resolve(loaded, agent_name)?);
    }
    Ok(())
}

pub fn render(envelope: &Value) -> String {
    let boundary = &envelope["declaredBoundary"];
    let runtime = &envelope["runtime"];
    let mut rendered = format!(
        concat!(
            "# {} task envelope: {}\n\n",
            "Display name: {}\nNative task name: {}\nPurpose: {}\nTask: {}\n",
            "Profile: {}\nProfile SHA-256: {}\n",
            "Requested runtime: host={}, model={}, reasoning effort={}, fallback={}\n\n",
            "## Declared boundary\n\n",
            "- Observe: {}\n- Write: {}\n- Commands: {}\n- Skills: {}\n",
            "- Memory read: {}\n- Memory write: {}\n- Memory review: {}\n",
            "- Memory promote: {}\n- Memory reject: {}\n- Memory revoke: {}\n",
            "- Memory expire: {}\n- Memory forget: {}\n",
            "- Retention: {}\n- Cross-context: {}\n\n> {}\n\n"
        ),
        if crate::producer::exitbind_surface() {
            "Exitbind"
        } else {
            "Soulmate"
        },
        string(&envelope["agent"]),
        string(&envelope["displayName"]),
        string(&envelope["nativeTaskName"]),
        string(&envelope["purpose"]),
        string(&envelope["task"]),
        string(&envelope["profile"]["path"]),
        string(&envelope["profile"]["sha256"]),
        optional(&runtime["host"]),
        optional(&runtime["model"]),
        optional(&runtime["reasoningEffort"]),
        optional(&runtime["fallback"]),
        list(boundary, "observe"),
        list(boundary, "write"),
        list(boundary, "commands"),
        list(boundary, "skills"),
        list(boundary, "memoryRead"),
        list(boundary, "memoryWrite"),
        list(boundary, "memoryReview"),
        list(boundary, "memoryPromote"),
        list(boundary, "memoryReject"),
        list(boundary, "memoryRevoke"),
        list(boundary, "memoryExpire"),
        list(boundary, "memoryForget"),
        string(&boundary["retention"]),
        string(&boundary["crossContext"]),
        string(&envelope["notice"]),
    );
    if let Some(references) = envelope["memoryReferences"].as_array() {
        rendered.push_str("## Memory references\n\n");
        if references.is_empty() {
            rendered.push_str("none\n");
        } else {
            for reference in references {
                rendered.push_str(&format!(
                    "- {} [{}] {} ({} bytes, {})\n",
                    string(&reference["itemId"]),
                    string(&reference["scope"]),
                    string(&reference["sourcePath"]),
                    reference["byteLength"],
                    string(&reference["sourceSha256"]),
                ));
            }
        }
    }
    rendered
}

fn names(workflow: &Value, field: &str) -> Vec<String> {
    workflow[field]
        .as_array()
        .map(|values| {
            values
                .iter()
                .filter_map(Value::as_str)
                .map(str::to_owned)
                .collect()
        })
        .unwrap_or_default()
}

fn list(boundary: &Value, field: &str) -> String {
    let values = boundary[field]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(Value::as_str)
        .collect::<Vec<_>>();
    if values.is_empty() {
        "none".into()
    } else {
        values.join(", ")
    }
}

fn string(value: &Value) -> &str {
    value.as_str().unwrap_or_default()
}

fn optional(value: &Value) -> &str {
    value.as_str().unwrap_or("none")
}

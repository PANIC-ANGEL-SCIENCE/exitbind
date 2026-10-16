//! Shared actual-assignment eligibility; routing and native consumers use these same bytes.
use super::*;

pub(crate) fn is_scope(scope: &str) -> bool {
    scope
        .strip_prefix("specialist.")
        .and_then(|s| s.strip_suffix(".v1"))
        .is_some_and(|name| !name.is_empty())
}

pub(crate) fn enabled(loaded: &Loaded, name: &str) -> bool {
    loaded
        .agent(name)
        .is_some_and(|a| a.memory_read.contains(&format!("specialist.{name}.v1")))
        && memory::policy::get(&loaded.config).is_some()
}

pub(super) fn exclusion(
    loaded: &Loaded,
    name: &str,
    role: &str,
    host: &str,
) -> Option<&'static str> {
    let agent = loaded.agent(name)?;
    if !loaded.roles_for(name).contains(role) {
        Some("role_not_configured")
    } else if agent
        .runtime
        .host
        .as_deref()
        .is_some_and(|primary| primary != host)
    {
        Some("primary_host_incompatible")
    } else if loaded
        .agents
        .iter()
        .filter(|(n, a)| a.native_name(n) == agent.native_name(name))
        .count()
        != 1
    {
        Some("native_mapping_ambiguous")
    } else {
        None
    }
}

pub(crate) fn delivery(
    loaded: &Loaded,
    name: &str,
    role: &str,
    task: &str,
    host: &str,
) -> Result<Option<Value>, String> {
    memory::choices::assert_host(loaded, name, task, host)?;
    if !enabled(loaded, name) || task.trim().is_empty() || !matches!(host, "codex" | "claude") {
        return Ok(None);
    }
    if exclusion(loaded, name, role, host).is_some() {
        return Ok(None);
    }
    let selected = select_after_source(loaded, name, task, role, host, || Ok(()))?;
    let generic = memory::selection::resolve_for_task(loaded, name, Some(task))?;
    let resources = selected["experience"]
        .as_array()
        .ok_or("invalid specialist selection")?;
    let bytes = generic
        .iter()
        .chain(resources.iter().map(|r| &r["reference"]))
        .try_fold(0usize, |total, reference| -> Result<usize, String> {
            let length = reference["byteLength"]
                .as_u64()
                .ok_or("memory source byte length unavailable")?;
            Ok(total.saturating_add(
                usize::try_from(length).map_err(|_| "memory source byte length overflow")?,
            ))
        })?;
    memory::selection::check_budget(
        loaded,
        generic.len().saturating_add(resources.len()),
        bytes,
        "combined",
    )?;
    Ok(Some(selected))
}

pub(super) fn select_after_source(
    loaded: &Loaded,
    name: &str,
    task: &str,
    role: &str,
    host: &str,
    after_source: impl FnMut() -> Result<(), String>,
) -> Result<Value, String> {
    select_resources(loaded, name, task, role, host, false, after_source)
}

pub(super) fn select_resources(
    loaded: &Loaded,
    name: &str,
    task: &str,
    role: &str,
    host: &str,
    candidates: bool,
    mut after_source: impl FnMut() -> Result<(), String>,
) -> Result<Value, String> {
    if !candidates {
        memory::choices::assert_host(loaded, name, task, host)?;
    }
    let agent = loaded
        .agent(name)
        .ok_or("selected specialist disappeared")?;
    // Ordinary recipients keep their existing complete profile bounds/expansion.
    let profile = path::secure_bytes(&loaded.control_root, &agent.profile, "specialist profile")?;
    let project = crate::host::assignment_context::project_identity(loaded)?;
    let references = references(loaded, name)?;

    let mut resources = Vec::new();
    let mut ids = BTreeSet::new();
    let mut observed_sources = Vec::new();
    let mut observed_guards = Vec::new();
    let max_bytes = memory::policy::get(&loaded.config).map_or(0, |policy| policy.max_bytes);
    let mut source_bytes = 0usize;
    for reference in &references {
        let source = reference["sourcePath"]
            .as_str()
            .ok_or("invalid specialist source")?;
        let bytes = read(&loaded.product_root, source)?;
        source_bytes = source_bytes.saturating_add(bytes.len());
        if source_bytes > max_bytes {
            return Err("specialist experience exceeds the configured recall byte budget".into());
        }
        observed_sources.push((source.to_owned(), bytes.clone()));
        if reference["sourceSha256"] != hash::bytes(&bytes) {
            return Err("specialist experience changed; no snapshot delivered".into());
        }
        let item = parse(&bytes, name, &project)?;
        after_source()?;
        let ledger = memory::snapshot(
            loaded,
            reference["ledgerPath"]
                .as_str()
                .ok_or("invalid specialist ledger")?,
        )
        .map_err(|_| "specialist lifecycle changed or became unavailable during snapshot")?;
        memory::validate_history_current(loaded, &ledger)
            .map_err(|_| "specialist recorded policy or actor profile changed during snapshot")?;
        let proposed = ledger.events.first().ok_or("empty specialist history")?;
        let reviewer = ledger.events.iter().find(|e| e["action"] == "review");
        let promoter = ledger.events.iter().find(|e| e["action"] == "promote");
        let owned = proposed["actor"] == name && proposed["action"] == "propose";
        let reviewed = reviewer.is_some_and(|e| e["actor"].as_str().is_some_and(|a| a != name));
        let promoted = promoter.is_some_and(|e| e["actor"].as_str() == loaded.lead());
        if !owned || !reviewed || !promoted || proposed["expiresAt"].as_str().is_none() {
            return Err("specialist experience needs attributable author, independent review, Lead promotion and explicit expiry".into());
        }
        // Duplicate accepted revisions are a conflict, including inapplicable ones.
        // Retire the predecessor through the existing lifecycle before adoption.
        if !ids.insert(item.id.clone()) {
            return Err(
                "conflicting accepted specialist revisions; Lead must retire the predecessor"
                    .into(),
            );
        }
        if !item.hosts.iter().any(|h| h == host)
            || (!candidates
                && !memory::contextual::enabled(loaded)
                && !item
                    .task_terms
                    .iter()
                    .any(|term| task.to_lowercase().contains(&term.to_lowercase())))
            || !item
                .required_commands
                .iter()
                .all(|c| agent.commands.contains(c))
        {
            continue;
        }
        let mut current = true;
        for guard in &item.guards {
            let bytes = read(&loaded.product_root, &guard.path)?;
            observed_guards.push((guard.path.clone(), bytes.clone()));
            if hash::bytes(&bytes) != guard.sha256 {
                current = false;
            }
        }
        if !current {
            continue;
        }
        let mut current_reference = reference.clone();
        current_reference["byteLength"] = json!(bytes.len());
        if !candidates && memory::contextual::enabled(loaded) {
            let Some(identity) =
                memory::contextual::identity(loaded, name, Some(task), &current_reference)?
            else {
                continue;
            };
            if identity["role"] != role {
                return Err("contextual specialist role differs from the actual recipient".into());
            }
            current_reference["contextSelection"] = identity;
        }
        resources.push(json!({"id":item.id,"revision":item.revision,"kind":item.kind,
            "reference":current_reference,"content":if candidates { Value::Null } else { serde_json::from_slice::<Value>(&bytes).map_err(|e| e.to_string())? },
            "coverage":"complete","state":"qualified_snapshot"}));
    }
    if !candidates && resources.len() > 3 {
        return Err("more than three applicable specialist resources; narrow the policy".into());
    }
    if path::secure_bytes(&loaded.control_root, &agent.profile, "specialist profile")? != profile
        || self::references(loaded, name)? != references
        || observed_sources
            .iter()
            .chain(&observed_guards)
            .any(|(source, expected)| read(&loaded.product_root, source).as_ref() != Ok(expected))
    {
        return Err("specialist inputs changed during snapshot; refresh before launch".into());
    }
    memory::ensure_config_current(loaded)?;
    let value = json!({"projectIdentity":project,"configurationSha256":hash::text(&loaded.source),
        "agent":name,"nativeName":agent.native_name(name),"role":role,"task":task,"host":host,
        "profileSha256":hash::bytes(&profile),"experience":resources,
        "delivery":"prepared current bytes; native presentation, receipt and observed use require separate evidence"});
    if serde_json::to_vec(&value).map_err(|e| e.to_string())?.len() > SNAPSHOT_LIMIT {
        return Err("specialist delivery exceeds 16384 bytes; narrow the selected context".into());
    }
    let mut result = value.clone();
    result["snapshotId"] = json!(hash::value(&value));
    Ok(result)
}

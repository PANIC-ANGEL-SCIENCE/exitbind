//! Typed inactive authoring and actor-correct forms over the existing lifecycle.
use super::*;

pub(crate) fn input_form(
    loaded: &Loaded,
    scope: &str,
    source: Option<&str>,
) -> Result<Value, String> {
    let name = scope
        .strip_prefix("specialist.")
        .and_then(|s| s.strip_suffix(".v1"))
        .ok_or("unknown specialist scope")?;
    let agent = loaded
        .agent(name)
        .ok_or("specialist scope requires a configured author")?;
    let validation_command = json!({"command":[crate::emitted::executable()?,"memory","form",crate::emitted::slot("SOURCE"),"--scope",scope,"--json","--config",loaded.path.to_str().ok_or("configuration path is not UTF-8")?],
        "readOnly":true,"placeholders":["SOURCE"],"meaning":"exact typed source and current author/project/config/profile binding only; no lifecycle, trial or qualification decision"});
    let validation = source
        .map(|source| validate_source(loaded, name, source))
        .transpose()?;
    Ok(json!({"version":1,"scope":scope,"effect":"no-change",
        "bounds":{"sourceMaximumBytes":SOURCE_LIMIT,"maximumApplicableResources":3,"maximumGuardFileBytes":SOURCE_LIMIT},
        "template":{"version":1,"id":"stable-skill-id","revision":"draft-1","kind":"skill",
            "author":name,"projectIdentity":crate::host::assignment_context::project_identity(loaded)?,
            "taskTerms":["<actual relevant task term>"],"hosts":agent.runtime.host.as_deref().map_or_else(|| vec!["codex", "claude"], |host| vec![host]),
            "summary":"<finding from your permitted task>","preconditions":["<when it applies>"],
            "procedure":["<bounded reusable procedure>"],"verification":["<meaningful trial and inappropriate-use negative>"],
            "requiredCommands":[],"evidence":["<actual Task A and distinct trial references>"],"guards":[]},
        "configuredCommands":agent.commands,
        "fieldBounds":{"id":"1..64 UTF-8 bytes; lowercase ASCII letters, digits, underscore or hyphen",
            "revision":"1..64 UTF-8 bytes; nonblank, no controls","kind":["observation","skill"],
            "taskTerms":{"minimumItems":1,"maximumItems":4,"maximumItemBytes":64},
            "hosts":{"minimumItems":1,"maximumItems":2,"values":["codex","claude"]},
            "summary":{"maximumBytes":512},"preconditions":{"minimumItems":1,"maximumItems":8,"maximumItemBytes":512},
            "procedure":{"minimumItems":1,"maximumItems":12,"maximumItemBytes":512},
            "verification":{"minimumItems":1,"maximumItems":8,"maximumItemBytes":512},
            "requiredCommands":{"minimumItems":0,"maximumItems":8,"maximumItemBytes":256},
            "evidence":{"minimumItems":1,"maximumItems":8,"maximumItemBytes":256},
            "guards":{"type":"array","minimumItems":0,"maximumItems":4,
                "element":{"path":"confined relative project file path; 1..256 UTF-8 bytes; no parent traversal",
                    "sha256":"exact expected file digest; 64 lowercase hexadecimal characters"},
                "example":{"path":"docs/current-contract.md","sha256":"<64 lowercase hexadecimal characters>"},
                "meaning":"Optional file-byte prerequisites only. Use [] for none; narrative authority or applicability restrictions belong in preconditions."}},
        "textRules":"All source strings are nonblank and contain no control characters; array bounds use UTF-8 bytes per item.",
        "validationRoute":validation_command,"validation":validation,
        "lifecycle":{"propose":["memory","propose",name,crate::emitted::slot("SOURCE"),"--scope",scope,"--ledger",crate::emitted::slot("NEW_LEDGER"),"--expires-at",crate::emitted::slot("RFC3339_EXPIRY")],
            "next":"Use the returned nextAction; a separate authorized reviewer reviews exact bytes, then the configured Lead promotes.",
            "inactiveStates":["draft","proposed","reviewed"]},
        "evidence":"References and lifecycle labels are reported claims, not verified trials or authenticated actor independence.",
        "authority":"No write, command, review or promotion rights are granted; existing policy and explicit expiry apply."}))
}

fn validate_source(loaded: &Loaded, name: &str, source: &str) -> Result<Value, String> {
    let bytes = read(&loaded.product_root, source)?;
    let project = crate::host::assignment_context::project_identity(loaded)?;
    let item = parse(loaded, &bytes, name, &project)?;
    let agent = loaded.agent(name).ok_or("specialist is not configured")?;
    let profile = profile_bytes(loaded, &agent.profile)?;
    memory::ensure_config_current(loaded)?;
    if read(&loaded.product_root, source)? != bytes
        || profile_bytes(loaded, &agent.profile)? != profile
    {
        return Err("specialist preparation inputs changed; read the exact source again".into());
    }
    Ok(json!({"valid":true,"typedSource":true,"effect":"no-change",
        "source":{"path":source,"sha256":hash::bytes(&bytes),"bytes":bytes.len()},
        "bindings":{"author":name,"projectIdentity":project,"configurationSha256":hash::text(&loaded.source),"profileSha256":hash::bytes(&profile)},
        "guardCount":item.guards.len(),"guardCurrentness":"file bytes are checked at actual task delivery",
        "qualification":"not_evaluated","lifecycle":"unchanged",
        "meaning":"The common delivery parser accepts these exact bytes and current identity; meaningful trials, independent review, promotion and task applicability remain separate."}))
}

pub(crate) fn next_action(
    loaded: &Loaded,
    scope: &str,
    ledger: &str,
    state: &str,
) -> Result<Value, String> {
    let owner = scope
        .strip_prefix("specialist.")
        .and_then(|s| s.strip_suffix(".v1"))
        .ok_or("invalid specialist scope")?;
    let action = match state {
        "proposed" => "review",
        "reviewed" => "promote",
        _ => "inspect",
    };
    let actors: Vec<&str> = match action {
        "review" => loaded
            .agents
            .iter()
            .filter(|(name, a)| {
                name.as_str() != owner && a.memory_review.iter().any(|s| s == scope)
            })
            .map(|(name, _)| name.as_str())
            .collect(),
        "promote" => loaded
            .lead()
            .filter(|name| {
                loaded
                    .agent(name)
                    .is_some_and(|a| a.memory_promote.iter().any(|s| s == scope))
            })
            .into_iter()
            .collect(),
        _ => Vec::new(),
    };
    let exe = crate::emitted::executable()?;
    let command = |actor: Option<&str>| -> Result<Vec<String>, String> {
        let mut argv = vec![exe.clone(), "memory".into(), action.into()];
        if let Some(actor) = actor {
            argv.push(actor.to_owned());
        }
        argv.extend([
            ledger.into(),
            "--config".into(),
            loaded
                .path
                .to_str()
                .ok_or("configuration is not UTF-8")?
                .into(),
        ]);
        Ok(argv)
    };
    let choices = actors
        .iter()
        .map(|actor| Ok(json!({"actor":actor,"command":command(Some(actor))?})))
        .collect::<Result<Vec<Value>, String>>()?;
    Ok(json!({"state":state,"readOnly":action == "inspect",
        "command":if action == "inspect" { Some(command(None)?) } else if actors.len() == 1 { Some(command(Some(actors[0]))?) } else { None },
        "choices":choices,"leadChoiceRequired":action != "inspect" && actors.len() != 1,
        "meaning":"Existing lifecycle only; supply actual independent review and exact-version trial evidence before promotion."}))
}

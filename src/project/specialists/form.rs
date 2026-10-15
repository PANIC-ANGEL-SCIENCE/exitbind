//! Typed inactive authoring and actor-correct forms over the existing lifecycle.
use super::*;

pub(crate) fn input_form(loaded: &Loaded, scope: &str) -> Result<Value, String> {
    let name = scope
        .strip_prefix("specialist.")
        .and_then(|s| s.strip_suffix(".v1"))
        .ok_or("unknown specialist scope")?;
    let agent = loaded
        .agent(name)
        .ok_or("specialist scope requires a configured author")?;
    Ok(json!({"version":1,"scope":scope,"effect":"no-change",
        "bounds":{"sourceMaximumBytes":SOURCE_LIMIT,"maximumApplicableResources":3},
        "template":{"version":1,"id":"stable-skill-id","revision":"draft-1","kind":"skill",
            "author":name,"projectIdentity":crate::host::assignment_context::project_identity(loaded)?,
            "taskTerms":["<actual relevant task term>"],"hosts":agent.runtime.host.as_deref().map_or_else(|| vec!["codex", "claude"], |host| vec![host]),
            "summary":"<finding from your permitted task>","preconditions":["<when it applies>"],
            "procedure":["<bounded reusable procedure>"],"verification":["<meaningful trial and inappropriate-use negative>"],
            "requiredCommands":[],"evidence":["<actual Task A and distinct trial references>"],"guards":[]},
        "configuredCommands":agent.commands,
        "lifecycle":{"propose":["memory","propose",name,"<SOURCE>","--scope",scope,"--ledger","<NEW_LEDGER>","--expires-at","<RFC3339_EXPIRY>"],
            "next":"Use the returned nextAction; a separate authorized reviewer reviews exact bytes, then the configured Lead promotes.",
            "inactiveStates":["draft","proposed","reviewed"]},
        "evidence":"References and lifecycle labels are reported claims, not verified trials or authenticated actor independence.",
        "authority":"No write, command, review or promotion rights are granted; existing policy and explicit expiry apply."}))
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
    let exe = std::env::current_exe().map_err(|e| e.to_string())?;
    let command = |actor: Option<&str>| -> Result<Vec<String>, String> {
        let mut argv = vec![
            exe.to_str().ok_or("executable is not UTF-8")?.to_owned(),
            "memory".into(),
            action.into(),
        ];
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

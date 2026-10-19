//! Fixed accepted-source consumer. Shipping records never mutate the source Work.
mod consume;
mod observation;
mod store;

use crate::{
    cli::args::{self, Arguments},
    config,
    evidence::{hash, receipt},
};
use serde_json::{json, Value};
use std::io::Read;

pub(crate) fn command(a: &Arguments) -> Result<(), String> {
    args::assert_options(
        "ship",
        a,
        &["config", "json", "apply", "target", "artifact"],
    )?;
    args::assert_positionals("ship", a, 2)?;
    let loaded = config::load(a.options.get("config").map(String::as_str))?;
    let subject = &a.positional[1];
    let apply = a.flags.contains_key("apply");
    let target = a.options.get("target").map(String::as_str);
    if a.positional[0] != "prepare" && a.options.contains_key("artifact") {
        return Err("--artifact applies only to ship prepare".into());
    }
    let value = match a.positional[0].as_str() {
        "prepare" => {
            if !apply { return Err("ship prepare requires --apply and explicit --target local staging name".into()); }
            prepare(&loaded, subject, target.ok_or("ship prepare requires --target")?, a.options.get("artifact").map(String::as_str))?
        }
        "consume" => consume::run(&loaded, subject, target, apply)?,
        "status" if !apply && target.is_none() => consume::status(&loaded, subject)?,
        "observe" if apply && target.is_none() => {
            let mut bytes = Vec::new();
            std::io::stdin().take(8193).read_to_end(&mut bytes).map_err(|e| e.to_string())?;
            observation::record(&loaded, subject, &bytes)?
        }
        _ => return Err("ship requires prepare WORK --target NAME --apply, consume ID --target NAME --apply, status ID, or observe ID --apply (bounded JSON on stdin)".into()),
    };
    let output = serde_json::to_string(&value).map_err(|e| e.to_string())?;
    if output.len() + 1 > 8192 {
        return Err(
            "shipping report exceeds bounded channel; retained facts remain available".into(),
        );
    }
    println!("{output}");
    Ok(())
}

fn current(loaded: &config::Loaded, work: &str) -> Result<Value, String> {
    let closeout = crate::work::closeout::closeout(loaded, work, false, None, None)?;
    if closeout["workAcceptance"]["currentEvidence"] != true {
        return Err(
            "shipping requires current accepted Work evidence; no source Work was changed".into(),
        );
    }
    let ledger = crate::work::resolve(loaded, work)?;
    let mut source = receipt::exit_path(loaded, &ledger).map_err(|e| e.to_string())?;
    let (_, events, _) = crate::run::ledger::load(loaded, &ledger)?;
    let state = crate::run::reduce_live(loaded, &events)?;
    source["testedInputsSha256"] = state["inputsSha256"].clone();
    source["acceptedBasis"] = state["basis"].clone();
    // The recorder may change between shipping calls; acceptance identity may not.
    source
        .as_object_mut()
        .ok_or("invalid receipt")?
        .remove("producer");
    Ok(source)
}

fn prepare(
    loaded: &config::Loaded,
    work: &str,
    target: &str,
    artifact: Option<&str>,
) -> Result<Value, String> {
    store::name(target)?;
    let ledger = crate::work::resolve(loaded, work)?;
    let lock = crate::run::ledger::ledger_path(&loaded.state_root, &ledger, false)?;
    crate::run::ledger::with_lock(&lock, || {
        let source = current(loaded, work)?;
        let artifact = source["artifacts"]
            .as_array()
            .ok_or("receipt has no artifacts")?
            .iter()
            .find(|item| artifact.map_or(true, |path| item["path"] == path))
            .ok_or("selected artifact is not an accepted submission artifact")?
            .clone();
        let packet = json!({"version":1,"format":"accepted-local-ship-v1", "work":work,
            "source":source, "artifact":artifact,
            "authority":{"kind":"owner_selected_local_staging", "target":target,
                "actions":["stage_accepted_artifact"],"externalEffects":false},
            "requiredGates":"current Work receipt checks/review/Lead acceptance",
            "unselectedGates":["remote publication","installation","platform matrix"],
            "policy":{"infrastructureRetryLimit":3,"unknownEffect":"readback_or_hold","productDefect":"hold_separate_repair_request"}});
        let id = hash::value(&packet);
        // Validate artifact bound and confinement before creating a packet.
        consume::artifact_bytes(loaded, &packet)?;
        let existed = store::read(loaded, &id, "packet")?.is_some();
        store::put(loaded, &id, "packet", &packet)?;
        if current(loaded, work)? != packet["source"] {
            return Err("source changed during handoff; packet held".into());
        }
        let route = crate::work::response_recovery::bounded_argv(
            vec![
                "ship".into(),
                "consume".into(),
                id.clone(),
                "--target".into(),
                target.into(),
                "--apply".into(),
                "--json".into(),
                "--config".into(),
            ],
            loaded.path.to_str(),
            1024,
        );
        let mut result = consume::status(loaded, &id)?;
        result["handoffEffect"] = json!(if existed {
            "no-change"
        } else {
            "packet_retained"
        });
        result["consumer"] = json!({"argv":route.argv,"sameConfigRequired":route.same_config,"sameExecutableRequired":route.same_executable});
        Ok(result)
    })
}

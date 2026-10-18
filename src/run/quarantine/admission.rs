//! Typed admission and durable one-shot reservation for quarantine checks.

use serde_json::{json, Value};
use std::{
    fs::{self, OpenOptions},
    io::{Read, Write},
    path::{Path, PathBuf},
};

use crate::{config::Loaded, evidence::hash};

mod followup;

pub(crate) struct Verified {
    work: String,
    ledger: String,
    marker: Value,
    workflow: String,
    goal: String,
    command: String,
    basis: String,
    start_intent: String,
    product_root: PathBuf,
    state_root: PathBuf,
    control_root: PathBuf,
}

impl Verified {
    pub(crate) fn work(&self) -> &str {
        &self.work
    }
    pub(crate) fn ledger(&self) -> &str {
        &self.ledger
    }
    pub(crate) fn marker(&self) -> &Value {
        &self.marker
    }
    pub(crate) fn workflow(&self) -> &str {
        &self.workflow
    }
    pub(crate) fn goal(&self) -> &str {
        &self.goal
    }
    pub(crate) fn command(&self) -> &str {
        &self.command
    }
    pub(crate) fn basis(&self) -> &str {
        &self.basis
    }
    pub(crate) fn start_intent_sha256(&self) -> &str {
        &self.start_intent
    }
}

pub(crate) fn prepare(
    loaded: &Loaded,
    predecessor_loaded: &Loaded,
    feature_loaded: &Loaded,
    marker: &Value,
) -> Result<Verified, String> {
    super::validate_marker(marker, 0)?;
    same_roots(loaded, predecessor_loaded)?;
    same_roots(loaded, feature_loaded)?;
    let config_sha = hash::text(&loaded.source);
    let feature_config_sha = hash::text(&feature_loaded.source);
    if fs::read_to_string(&loaded.path).map_err(|e| e.to_string())? != loaded.source
        || predecessor_loaded.source != loaded.source
        || predecessor_loaded.path != loaded.path
        || fs::read_to_string(&feature_loaded.path).map_err(|e| e.to_string())?
            != feature_loaded.source
        || marker["verification"]["configSha256"] != config_sha
    {
        return Err("quarantine admission requires the exact current configuration bytes".into());
    }

    let predecessor = &marker["predecessor"];
    let (old_path, old_events, old_bytes) =
        crate::run::ledger::load(predecessor_loaded, str_value(predecessor, "ledger")?)?;
    if crate::run::ledger::valid_claim(&crate::run::ledger::claim_path(&old_path))?.is_some() {
        return Err("predecessor already has a supersession claim".into());
    }
    let old_start = old_events.first().ok_or("predecessor ledger is empty")?;
    let old_head = old_events.last().ok_or("predecessor ledger is empty")?;
    let old_state = crate::run::state::reduce(&old_events)?;
    if old_state.get("quarantineVerification").is_some()
        || !matches!(old_state["status"].as_str(), Some("running" | "blocked"))
        || old_start["runId"] != predecessor["runId"]
        || old_start["configSha256"] != predecessor["configSha256"]
        || old_start["configSha256"] != config_sha
        || old_start["eventSha256"] != predecessor["startEventSha256"]
        || old_head["eventSha256"] != predecessor["headEventSha256"]
        || hash::bytes(old_bytes.as_bytes()) != predecessor["ledgerSha256"]
        || crate::run::check_observation::PROTOCOL != old_start["checkObservationProtocol"]
        || old_state["checkObservation"]["state"] != "failed"
        || old_state["checkObservation"]["facts"]["termination"] != "unknown"
        || !crate::run::state::check_observation::unresolved(&old_state)
    {
        return Err("predecessor is no longer the exact unresolved check observation".into());
    }
    let admission = old_events
        .iter()
        .find(|event| {
            event["action"] == "check_observation"
                && event["eventSha256"] == predecessor["admissionEventSha256"]
        })
        .ok_or("predecessor check admission event is missing")?;
    let failure = old_events.iter().rev().find(|event| {
        event["action"] == "check_observation_failed"
            && event["observation"]["admissionEventSha256"] == admission["eventSha256"]
    });
    let Some(failure) = failure else {
        return Err("predecessor has no durable failed observation event".into());
    };
    let expected_observation = &failure["eventSha256"];
    if expected_observation != &predecessor["observationEventSha256"]
        || old_events.iter().any(|event| {
            event["action"] == "check"
                && event["observationEventSha256"] == admission["eventSha256"]
        })
        || old_events.iter().any(|event| {
            event["action"] == "check_observation_recovered"
                && event["observation"]["admissionEventSha256"] == admission["eventSha256"]
        })
    {
        return Err("predecessor observation is not unresolved".into());
    }
    followup::validate_predecessor(predecessor_loaded, marker)?;
    ensure_lead(&old_start["plan"], marker)?;
    let usage = crate::session_goal::usage::details_for_work(
        predecessor_loaded,
        predecessor["work"]
            .as_str()
            .ok_or("predecessor Work is invalid")?,
    );
    let old_governor = crate::run::carry::accounting(&old_state, &old_start["governor"])?;
    if old_governor != predecessor["accounting"]["governor"]
        || usage != predecessor["accounting"]["usageAccount"]
    {
        return Err("predecessor accounting changed since quarantine review".into());
    }

    let review = &marker["verification"]["isolationReview"];
    let feature_work = review["work"]
        .as_str()
        .ok_or("isolation review Work is invalid")?;
    let feature_ledger = work_ledger(feature_work)?;
    let (_, feature_events, _) = crate::run::ledger::load(feature_loaded, &feature_ledger)?;
    let feature_start = feature_events
        .first()
        .ok_or("isolation review ledger is empty")?;
    let feature_head = feature_events
        .last()
        .ok_or("isolation review ledger is empty")?;
    let feature_state = crate::run::state::reduce(&feature_events)?;
    crate::run::artifact::assert_current(feature_loaded, &feature_state)?;
    let feature_inputs = crate::run::inputs::fingerprint(feature_loaded)?;
    if feature_state["inputsSha256"].as_str() != Some(feature_inputs.as_str()) {
        return Err("isolation review Work inputs are no longer current".into());
    }
    if feature_state["status"] != "accepted"
        || feature_start["configSha256"] != feature_config_sha
        || feature_start["eventSha256"] != review["startEventSha256"]
        || feature_head["eventSha256"] != review["headEventSha256"]
        || feature_state["subject"]["sha256"] != review["subjectSha256"]
    {
        return Err(
            "isolation review Work is not currently accepted on the reviewed configuration".into(),
        );
    }
    ensure_lead(&feature_start["plan"], marker)?;
    let feature_workflow = feature_start["workflow"]
        .as_str()
        .ok_or("isolation review workflow is missing")?;
    let feature_goal = feature_start["goal"]
        .as_str()
        .ok_or("isolation review goal is missing")?;
    let mut feature_selected = crate::config::boundary::apply(
        feature_loaded,
        crate::run::reducer::selected_plan(crate::envelope::plan(
            feature_loaded,
            feature_workflow,
            feature_goal,
        )?)?,
        None,
    )?;
    crate::project::architecture::bind_plan(feature_loaded, &mut feature_selected)?;
    if feature_selected != feature_start["plan"] {
        return Err("isolation review Work plan is no longer the current selected plan".into());
    }
    let check = feature_events
        .iter()
        .find(|event| {
            event["action"] == "check" && event["eventSha256"] == review["checkEventSha256"]
        })
        .ok_or("isolation review check event is missing")?;
    let reviewer = feature_events
        .iter()
        .find(|event| {
            event["action"] == "submit"
                && event["role"] == "reviewer"
                && event["outcome"] == "approved"
                && event["eventSha256"] == review["reviewEventSha256"]
        })
        .ok_or("isolation review reviewer approval is missing")?;
    let lead = feature_events
        .iter()
        .find(|event| {
            event["action"] == "submit"
                && event["role"] == "lead"
                && event["outcome"] == "accepted"
                && event["eventSha256"] == review["acceptanceEventSha256"]
        })
        .ok_or("isolation review Lead acceptance is missing")?;
    if check["acquisition"] != "observed"
        || (check["exitCode"].as_i64() != Some(0) && check["result"]["code"].as_i64() != Some(0))
        || check["inputsSha256"] != feature_state["inputsSha256"]
        || check["configSha256"] != feature_config_sha
        || reviewer["subjectSha256"] != review["subjectSha256"]
        || reviewer["agent"] == marker["leadOwner"]["agent"]
        || lead["subjectSha256"] != review["subjectSha256"]
        || lead["agent"] != marker["leadOwner"]["agent"]
    {
        return Err("isolation review lacks a fresh observed check, review, or acceptance".into());
    }
    let worker = feature_events
        .iter()
        .rev()
        .find(|event| {
            event["action"] == "submit"
                && event["role"] == "worker"
                && event["outcome"] == "completed"
        })
        .ok_or("isolation review worker artifact is missing")?;
    let report_bytes = crate::run::artifact::read_exact(
        feature_loaded,
        &worker["artifact"],
        "quarantine verification plan",
        256 * 1024,
    )?;
    let report: Value = serde_json::from_slice(&report_bytes)
        .map_err(|_| "quarantine verification plan is not valid JSON")?;
    let plan = &report["plan"];
    if report["kind"] != "quarantine-verification-plan"
        || report["protocol"] != 1
        || !plan.is_object()
        || hash::value(plan) != review["planSha256"]
        || hash::value(&plan["effectInventory"]) != review["effectInventorySha256"]
        || hash::value(&plan["environment"]) != marker["verification"]["environmentSha256"]
        || plan["artifactSha256"] != marker["verification"]["artifactSha256"]
        || plan["inputsSha256"] != marker["verification"]["inputsSha256"]
    {
        return Err(
            "quarantine verification plan differs from its accepted isolation review".into(),
        );
    }
    if marker["protocol"] == super::FOLLOWUP_PROTOCOL
        && plan.get("custodyEvidence") != Some(&marker["followUp"]["custodyEvidence"])
    {
        return Err(
            "follow-up custody evidence differs from the separately accepted verification plan"
                .into(),
        );
    }
    let command = plan["command"]
        .as_str()
        .filter(|value| !value.trim().is_empty())
        .ok_or("quarantine verification command is missing")?
        .to_owned();
    if hash::text(&command) != marker["verification"]["commandSha256"]
        || crate::run::inputs::fingerprint(loaded)? != marker["verification"]["inputsSha256"]
    {
        return Err("quarantine verification artifact, command, or current inputs changed".into());
    }
    let verification_basis =
        crate::kernel::basis::parse_basis(&plan["basis"], "quarantine verification basis")?;
    let workflow = old_start["workflow"]
        .as_str()
        .ok_or("predecessor workflow is missing")?
        .to_owned();
    let goal = old_start["goal"]
        .as_str()
        .ok_or("predecessor goal is missing")?
        .to_owned();
    let mut selected = crate::config::boundary::apply(
        loaded,
        crate::run::reducer::selected_plan(crate::envelope::plan(loaded, &workflow, &goal)?)?,
        None,
    )?;
    crate::project::architecture::bind_plan(loaded, &mut selected)?;
    ensure_lead(&selected, marker)?;
    ensure_reviewer(&feature_selected, &selected, reviewer, marker)?;
    let basis = serde_json::to_string(&verification_basis.value()).map_err(|e| e.to_string())?;
    let token = super::one_shot_identity(marker)?;
    let ledger = format!(".exitbind/runs/work-{token}.jsonl");
    let start_intent = crate::run::lifecycle::quarantine_start_intent_sha256(
        loaded, &workflow, &goal, &command, &basis, marker,
    )?;
    Ok(Verified {
        work: format!("smw_{token}"),
        ledger,
        marker: marker.clone(),
        workflow,
        goal,
        command,
        basis,
        start_intent,
        product_root: loaded.product_root.clone(),
        state_root: loaded.state_root.clone(),
        control_root: loaded.control_root.clone(),
    })
}

pub(crate) fn intent(loaded: &Loaded, verified: &Verified) -> Result<String, String> {
    if loaded.product_root != verified.product_root
        || loaded.state_root != verified.state_root
        || loaded.control_root != verified.control_root
        || fs::read_to_string(&loaded.path).map_err(|e| e.to_string())? != loaded.source
        || hash::text(&loaded.source) != verified.marker["verification"]["configSha256"]
        || crate::run::inputs::fingerprint(loaded)?
            != verified.marker["verification"]["inputsSha256"]
    {
        return Err(
            "quarantine start intent is stale against current configuration or inputs".into(),
        );
    }
    Ok(verified.start_intent.clone())
}

pub(crate) fn start<F>(
    loaded: &Loaded,
    predecessor_loaded: &Loaded,
    feature_loaded: &Loaded,
    verified: &Verified,
    before_start: F,
) -> Result<Value, String>
where
    F: FnOnce(&str, &str) -> Result<(), String>,
{
    let token = super::one_shot_identity(&verified.marker)?;
    let old_ledger = verified.marker["predecessor"]["ledger"]
        .as_str()
        .ok_or("predecessor ledger is missing")?;
    let (old_path, _, _) = crate::run::ledger::load(predecessor_loaded, old_ledger)?;
    let lock_path = if verified.marker["protocol"] == super::FOLLOWUP_PROTOCOL {
        let first_ledger = verified.marker["followUp"]["ledger"]
            .as_str()
            .ok_or("first verification ledger is missing")?;
        crate::run::ledger::load(predecessor_loaded, first_ledger)?.0
    } else {
        old_path.clone()
    };
    crate::run::ledger::with_lock(&lock_path, || {
        if verified.marker["protocol"] == super::FOLLOWUP_PROTOCOL {
            let link = &verified.marker["followUp"];
            let (_, first_events, first_bytes) =
                crate::run::ledger::load(predecessor_loaded, str_value(link, "ledger")?)?;
            if hash::bytes(first_bytes.as_bytes()) != link["ledgerSha256"]
                || first_events.last().map(|event| &event["eventSha256"])
                    != Some(&link["headEventSha256"])
                || crate::run::state::reduce(&first_events)?["status"] != "blocked"
            {
                return Err("first failed verification changed after follow-up admission".into());
            }
        } else {
            let (_, events, source) = crate::run::ledger::load(predecessor_loaded, old_ledger)?;
            let state = crate::run::state::reduce(&events)?;
            if !matches!(state["status"].as_str(), Some("running" | "blocked"))
                || state.get("quarantineVerification").is_some()
                || hash::text(&source) != verified.marker["predecessor"]["ledgerSha256"]
                || events.last().map(|event| &event["eventSha256"])
                    != Some(&verified.marker["predecessor"]["headEventSha256"])
            {
                return Err("predecessor changed after quarantine admission".into());
            }
        }
        if crate::run::ledger::valid_claim(&crate::run::ledger::claim_path(&old_path))?.is_some() {
            return Err("predecessor already has a supersession claim".into());
        }
        let refreshed = prepare(loaded, predecessor_loaded, feature_loaded, &verified.marker)?;
        if refreshed.work != verified.work
            || refreshed.ledger != verified.ledger
            || refreshed.start_intent != verified.start_intent
        {
            return Err("quarantine admission changed after its typed preparation".into());
        }
        let path = crate::run::ledger::ledger_path(&loaded.state_root, &verified.ledger, true)?;
        let reservation = reservation_path(&loaded.state_root, &token);
        crate::project::git_preflight::refuse_tracked_targets(
            &loaded.state_root,
            &[reservation.as_path()],
        )?;
        reserve(
            &loaded.state_root,
            &reservation,
            &json!({"protocol":1,"identity":token,"requestSha256":hash::value(&verified.marker),
            "work":verified.work,"ledger":verified.ledger,"startIntentSha256":verified.start_intent}),
        )?;
        match fs::symlink_metadata(path.path) {
            Ok(_) => {
                let (_, new_events, _) = crate::run::ledger::load(loaded, &verified.ledger)?;
                if new_events
                    .first()
                    .map(crate::run::lifecycle::start_event_intent_sha256)
                    .transpose()?
                    .as_deref()
                    != Some(verified.start_intent.as_str())
                    || new_events[0]["quarantineVerification"] != verified.marker
                {
                    return Err(
                        "reserved quarantine Work ledger conflicts with its exact start intent"
                            .into(),
                    );
                }
                return Ok(
                    json!({"valid":true,"work":verified.work,"ledger":verified.ledger,
                    "event":new_events[0],"replayed":true}),
                );
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => return Err(error.to_string()),
        }
        intent(loaded, verified)?;
        before_start(&verified.work, &verified.start_intent)?;
        crate::run::lifecycle::start_with_quarantine_verification(loaded, verified)
    })
}

fn same_roots(a: &Loaded, b: &Loaded) -> Result<(), String> {
    if a.product_root != b.product_root
        || a.state_root != b.state_root
        || a.control_root != b.control_root
        || a.project_id != b.project_id
    {
        return Err(
            "quarantine admission contexts do not select the same product and state roots".into(),
        );
    }
    Ok(())
}

fn ensure_lead(plan: &Value, marker: &Value) -> Result<(), String> {
    let owner = &marker["leadOwner"];
    let found = plan["stages"].as_array().is_some_and(|stages| {
        stages.iter().any(|stage| {
            stage["agents"].as_array().is_some_and(|agents| {
                agents.iter().any(|agent| {
                    agent["role"] == "lead"
                        && agent["name"] == owner["agent"]
                        && agent["nativeTaskName"] == owner["nativeTaskName"]
                })
            })
        })
    });
    if found {
        Ok(())
    } else {
        Err("configured Lead owner differs from the reviewed quarantine owner".into())
    }
}

fn ensure_reviewer(
    feature_plan: &Value,
    new_plan: &Value,
    approval: &Value,
    marker: &Value,
) -> Result<(), String> {
    let name = approval["agent"]
        .as_str()
        .ok_or("isolation reviewer identity is missing")?;
    let feature =
        reviewer(feature_plan, name).ok_or("accepted reviewer is not in the feature plan")?;
    let next =
        reviewer(new_plan, name).ok_or("configured verification plan has no matching reviewer")?;
    if name == marker["leadOwner"]["agent"]
        || feature["nativeTaskName"] != next["nativeTaskName"]
        || feature["nativeTaskName"].as_str().is_none()
    {
        return Err("feature and verification reviewer identities are incompatible".into());
    }
    Ok(())
}

fn reviewer<'a>(plan: &'a Value, name: &str) -> Option<&'a Value> {
    plan["stages"].as_array().and_then(|stages| {
        stages
            .iter()
            .flat_map(|stage| stage["agents"].as_array().into_iter().flatten())
            .find(|agent| agent["role"] == "reviewer" && agent["name"] == name)
    })
}

fn str_value<'a>(value: &'a Value, key: &str) -> Result<&'a str, String> {
    value[key]
        .as_str()
        .ok_or_else(|| format!("quarantine {key} is missing"))
}

fn work_ledger(work: &str) -> Result<String, String> {
    if !crate::work::valid_work_handle(work) {
        return Err("quarantine Work handle is invalid".into());
    }
    Ok(format!(".exitbind/runs/work-{}.jsonl", &work[4..]))
}

fn reservation_path(root: &Path, identity: &str) -> PathBuf {
    root.join(".exitbind/runs")
        .join(format!("quarantine-{identity}.intent.json"))
}

fn reserve(root: &Path, path: &Path, value: &Value) -> Result<(), String> {
    crate::project::managed_files::ensure_state_directory(
        root,
        path.parent().ok_or("intent path has no parent")?,
    )?;
    let bytes = serde_json::to_vec(value).map_err(|e| e.to_string())?;
    match open_reserved(path) {
        Ok(mut file) => {
            file.write_all(&bytes).map_err(|e| e.to_string())?;
            file.sync_all().map_err(|e| e.to_string())?;
            if let Some(parent) = path.parent() {
                fs::File::open(parent)
                    .and_then(|file| file.sync_all())
                    .map_err(|e| e.to_string())?;
            }
            Ok(())
        }
        Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {
            let existing = read_reserved(path)?;
            if existing == *value {
                Ok(())
            } else {
                Err(
                    "a different quarantine request already reserved this original check admission"
                        .into(),
                )
            }
        }
        Err(error) => Err(error.to_string()),
    }
}

fn open_reserved(path: &Path) -> std::io::Result<fs::File> {
    let mut options = OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600).custom_flags(libc::O_NOFOLLOW);
    }
    options.open(path)
}

fn read_reserved(path: &Path) -> Result<Value, String> {
    let before = fs::symlink_metadata(path).map_err(|e| e.to_string())?;
    if before.file_type().is_symlink() || !before.is_file() {
        return Err("quarantine intent is not a regular file".into());
    }
    let mut options = OpenOptions::new();
    options.read(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.custom_flags(libc::O_NOFOLLOW);
    }
    let mut file = options.open(path).map_err(|e| e.to_string())?;
    let opened = file.metadata().map_err(|e| e.to_string())?;
    if !opened.is_file() || file_identity(&opened) != file_identity(&before) {
        return Err("quarantine intent changed while opening".into());
    }
    let mut bytes = Vec::new();
    file.read_to_end(&mut bytes).map_err(|e| e.to_string())?;
    let after = fs::symlink_metadata(path).map_err(|e| e.to_string())?;
    if after.file_type().is_symlink()
        || !after.is_file()
        || file_identity(&after) != file_identity(&opened)
        || after.len() != bytes.len() as u64
    {
        return Err("quarantine intent changed while reading".into());
    }
    serde_json::from_slice(&bytes).map_err(|_| {
        "quarantine intent is partial or corrupt; inspect the exact new ledger before recovery"
            .into()
    })
}

#[cfg(unix)]
fn file_identity(metadata: &fs::Metadata) -> (u64, u64) {
    use std::os::unix::fs::MetadataExt;
    (metadata.dev(), metadata.ino())
}

#[cfg(not(unix))]
fn file_identity(metadata: &fs::Metadata) -> (u64, u64) {
    (metadata.len(), metadata.len())
}

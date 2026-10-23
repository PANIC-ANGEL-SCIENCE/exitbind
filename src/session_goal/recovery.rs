//! Explicit, owner-bound recovery for a native Goal whose retained journey
//! contains members from a separately configured Source project.

mod configuration;
mod record;

use self::configuration::config_binding;
use self::record::{decision_digest, valid_text, validated_state};
pub(crate) use self::record::{preserve_same_goal, validate_record};
use crate::{config::Loaded, evidence::hash};
use serde::de::{MapAccess, Visitor};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::{
    collections::{BTreeMap, BTreeSet},
    path::Path,
};

const FIELD: &str = "nativeGoalRecovery";
const VERSION: u64 = 1;
const MAX_REQUESTS: usize = 32;
const MAX_FIXTURES: usize = 32;

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct ConfigBinding {
    pub(crate) path: String,
    pub(crate) config_sha256: String,
    pub(crate) product_root: String,
    pub(crate) state_root: String,
    pub(crate) project_id: Option<String>,
    pub(crate) project_identity_sha256: String,
    pub(crate) local_identity_sha256: String,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct SourceMember {
    member: super::journey::Member,
    member_sha256: String,
    endpoint_config: ConfigBinding,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct HistoricalFixture {
    work: String,
    start_event_sha256: String,
    member_sha256: String,
    reason: String,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct AppliedRequest {
    request_id: String,
    request_sha256: String,
    preview_sha256: String,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct ActiveDecision {
    pub(crate) goal_id: String,
    pub(crate) applied_from_revision: u64,
    pub(crate) applied_from_event_sha256: String,
    pub(crate) host: String,
    pub(crate) session_key: String,
    pub(crate) owner_identity_sha256: String,
    pub(crate) owner_project_id: Option<String>,
    pub(crate) reason: String,
    pub(crate) source_config: ConfigBinding,
    source_members: Vec<SourceMember>,
    historical_fixtures: Vec<HistoricalFixture>,
    pub(crate) decision_sha256: String,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct RecoveryState {
    version: u64,
    active: ActiveDecision,
    requests: Vec<AppliedRequest>,
}

pub(crate) struct Context {
    pub(crate) source: Loaded,
    pub(crate) active: ActiveDecision,
    members: Vec<super::journey::Member>,
    pub(crate) endpoints: BTreeMap<String, Loaded>,
    pub(crate) historical: BTreeSet<String>,
    pub(crate) historical_items: Vec<Value>,
}

pub(crate) struct RecoveryEvidence {
    pub(crate) accepted: bool,
    pub(crate) artifact_current: bool,
    pub(crate) drift: Option<Value>,
    pub(crate) inputs_current: Option<bool>,
}

#[derive(Clone, Debug)]
pub(crate) struct Request<'a> {
    pub(crate) source_config: &'a str,
    pub(crate) historical_fixtures: &'a str,
    pub(crate) endpoint_configs: Option<&'a str>,
    pub(crate) request_id: &'a str,
    pub(crate) reason: &'a str,
    pub(crate) apply: bool,
    pub(crate) current_binding: Option<&'a str>,
}

pub(crate) fn config_binding_for_current(loaded: &Loaded) -> Result<ConfigBinding, String> {
    config_binding(loaded)
}

fn named_goal(record: &Value) -> bool {
    super::requirements::named(record)
}

pub(crate) fn context(owner_loaded: &Loaded, record: &Value) -> Result<Option<Context>, String> {
    let Some(state) = validated_state(record)? else {
        return Ok(None);
    };
    let active = state.active;
    if named_goal(record) {
        return Err("native Goal recovery cannot be used by a named-requirement Goal".into());
    }
    if record["goalId"].as_str() != Some(active.goal_id.as_str()) {
        return Err(
            "native Goal recovery belongs to a different Goal; refresh the recovery".into(),
        );
    }
    let native_session = super::native::current()?
        .ok_or("native Goal recovery requires its original host session")?;
    let scope = super::native::scope(owner_loaded, false)?;
    let owner = scope
        .owner
        .as_ref()
        .ok_or("native Goal recovery owner is unavailable")?;
    if !scope.is_session()
        || native_session.host != active.host
        || native_session.key != active.session_key
        || owner.identity_sha256 != active.owner_identity_sha256
        || owner.project_id != active.owner_project_id
    {
        return Err("native Goal recovery is bound to another host session or fixed owner".into());
    }
    let source = crate::config::load(Some(&active.source_config.path)).map_err(|error| {
        format!("saved Source configuration is unavailable; refresh recovery: {error}")
    })?;
    if config_binding(&source)? != active.source_config {
        return Err("saved Source configuration changed; preview recovery again".into());
    }
    let current_members = source_members(&source, record)?;
    let selected_members = active
        .source_members
        .iter()
        .map(|item| item.member.clone())
        .collect::<Vec<_>>();
    if current_members != selected_members {
        return Err("Source Goal membership changed since recovery; preview recovery again".into());
    }
    let journey =
        super::journey::state(record)?.ok_or("native Goal journey membership is unavailable")?;
    let by_work = journey
        .members
        .into_iter()
        .map(|member| (member.work.clone(), member))
        .collect::<BTreeMap<_, _>>();
    let mut historical = BTreeSet::new();
    let mut historical_items = Vec::new();
    for fixture in &active.historical_fixtures {
        let member = by_work
            .get(&fixture.work)
            .ok_or("retained historical member disappeared from Goal history")?;
        if member.start_event_sha256 != fixture.start_event_sha256
            || super::journey::member_identity(member) != fixture.member_sha256
            || current_members
                .iter()
                .any(|source| source.work == fixture.work)
        {
            return Err("retained historical member identity changed or now overlaps Source; refresh recovery".into());
        }
        historical.insert(fixture.work.clone());
        historical_items.push(json!({
            "work":fixture.work,
            "state":"historical",
            "reason":fixture.reason,
            "startEventSha256":fixture.start_event_sha256,
            "memberSha256":fixture.member_sha256,
        }));
    }
    let mut endpoints = BTreeMap::new();
    for binding in &active.source_members {
        let member = by_work
            .get(&binding.member.work)
            .ok_or("recovered Source member disappeared from Goal history")?;
        if member != &binding.member {
            return Err("recovered Source member identity changed; preview recovery again".into());
        }
        let endpoint =
            crate::config::load(Some(&binding.endpoint_config.path)).map_err(|error| {
                format!("selected endpoint configuration is unavailable; refresh recovery: {error}")
            })?;
        if config_binding(&endpoint)? != binding.endpoint_config {
            return Err(format!(
                "endpoint configuration for {} changed; preview recovery again",
                member.work
            ));
        }
        super::journey::validate_recovery_endpoint(&endpoint, member)?;
        endpoints.insert(member.work.clone(), endpoint);
    }
    Ok(Some(Context {
        source,
        active,
        members: current_members,
        endpoints,
        historical,
        historical_items,
    }))
}

/// Resolves only exact accepted endpoint references captured by the recovery,
/// or the exact currently accepted integration that supports that binding.
/// `None` leaves ordinary owner-project lookup unchanged.
pub(crate) fn result_ref_evidence(
    owner_loaded: &Loaded,
    record: &Value,
    reference: &str,
) -> Result<Option<RecoveryEvidence>, String> {
    let Some(context) = context(owner_loaded, record)? else {
        return Ok(None);
    };
    for member in &context.members {
        let endpoint_loaded = context
            .endpoints
            .get(&member.work)
            .ok_or("recovered endpoint configuration is unavailable")?;
        let Some(endpoint) =
            super::journey::accepted_endpoint_for_recovery(endpoint_loaded, member)?
        else {
            continue;
        };
        if endpoint == reference {
            return Ok(
                crate::run::result_ref_evidence(endpoint_loaded, reference)?.map(|evidence| {
                    RecoveryEvidence {
                        accepted: evidence.accepted,
                        artifact_current: evidence.artifact_current,
                        drift: evidence.drift,
                        inputs_current: evidence.inputs_current,
                    }
                }),
            );
        }
    }
    if let Some((integration, integration_config)) =
        super::journey::current_recovery_integration(owner_loaded, record)?
    {
        if integration == reference {
            return Ok(
                crate::run::result_ref_evidence(&integration_config, reference)?.map(|evidence| {
                    RecoveryEvidence {
                        accepted: evidence.accepted,
                        artifact_current: evidence.artifact_current,
                        drift: evidence.drift,
                        inputs_current: evidence.inputs_current,
                    }
                }),
            );
        }
    }
    Ok(None)
}

fn endpoint_overrides(raw: Option<&str>) -> Result<BTreeMap<String, String>, String> {
    let Some(raw) = raw else {
        return Ok(BTreeMap::new());
    };
    let values: UniqueEndpointOverrides = serde_json::from_str(raw)
        .map_err(|_| "--endpoint-configs must be a JSON object mapping Work IDs to paths")?;
    let values = values.0;
    if values
        .iter()
        .any(|(work, path)| !valid_text(work, 256) || !valid_text(path, 4096))
    {
        return Err("--endpoint-configs contains an invalid Work ID or path".into());
    }
    Ok(values)
}

struct UniqueEndpointOverrides(BTreeMap<String, String>);

impl<'de> Deserialize<'de> for UniqueEndpointOverrides {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        struct EndpointMapVisitor;

        impl<'de> Visitor<'de> for EndpointMapVisitor {
            type Value = UniqueEndpointOverrides;

            fn expecting(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                formatter.write_str("an object with unique Work ID keys and string paths")
            }

            fn visit_map<M>(self, mut map: M) -> Result<Self::Value, M::Error>
            where
                M: MapAccess<'de>,
            {
                let mut values = BTreeMap::new();
                while let Some((work, path)) = map.next_entry::<String, String>()? {
                    if values.insert(work, path).is_some() {
                        return Err(serde::de::Error::custom("duplicate Work ID"));
                    }
                }
                Ok(UniqueEndpointOverrides(values))
            }
        }

        deserializer.deserialize_map(EndpointMapVisitor)
    }
}

fn request_digest(request: &Request<'_>) -> Result<String, String> {
    Ok(hash::value(&json!({
        "sourceConfig": request.source_config,
        "historicalFixtures": historical_ids(request.historical_fixtures)?,
        "endpointConfigs": endpoint_overrides(request.endpoint_configs)?,
        "reason": request.reason,
    })))
}

fn historical_ids(raw: &str) -> Result<Vec<String>, String> {
    let mut seen = BTreeSet::new();
    let values = raw
        .split(',')
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_owned)
        .collect::<Vec<_>>();
    if values.len() > MAX_FIXTURES || values.iter().any(|value| !seen.insert(value.clone())) {
        return Err("--historical-fixtures must contain unique exact Work IDs".into());
    }
    Ok(values)
}

fn source_members(source: &Loaded, record: &Value) -> Result<Vec<super::journey::Member>, String> {
    let state = super::journey::state(record)?
        .ok_or("native session Goal has no explicit journey membership protocol")?;
    let source_config = config_binding(source)?;
    let mut members = state
        .members
        .into_iter()
        .filter(|member| {
            member.project_identity_sha256 == source_config.project_identity_sha256
                && member.product_root == source_config.product_root
                && member.state_root == source_config.state_root
        })
        .collect::<Vec<_>>();
    members.sort_by(|left, right| left.work.cmp(&right.work));
    Ok(members)
}

fn preview_decision(
    owner_loaded: &Loaded,
    source: &Loaded,
    record: &Value,
    request: &Request<'_>,
) -> Result<(ActiveDecision, String), String> {
    if named_goal(record) {
        return Err("native Goal recovery does not support named-requirement Goals".into());
    }
    if record["closure"]["closed"] == true {
        return Err(
            "native Goal recovery requires an open Goal; inspect the current Goal before recovery"
                .into(),
        );
    }
    if !valid_text(request.request_id, 128) || !valid_text(request.reason, 1024) {
        return Err("goal recover requires bounded non-empty --request-id and --reason".into());
    }
    let source_config = config_binding(source)?;
    let mut all = super::journey::state(record)?
        .ok_or("native session Goal has no explicit journey membership protocol")?;
    let members = source_members(source, record)?;
    if members.is_empty() {
        return Err("goal recover found no exact Source members for --source-config".into());
    }
    let selected = historical_ids(request.historical_fixtures)?;
    let overrides = endpoint_overrides(request.endpoint_configs)?;
    let by_work = all
        .members
        .drain(..)
        .map(|member| (member.work.clone(), member))
        .collect::<BTreeMap<_, _>>();
    if selected.iter().any(|work| !by_work.contains_key(work)) {
        return Err("--historical-fixtures contains an unknown native Goal member".into());
    }
    if selected
        .iter()
        .any(|work| members.iter().any(|member| &member.work == work))
    {
        return Err("a current Source contributor cannot be selected as historical".into());
    }
    if overrides
        .keys()
        .any(|work| !members.iter().any(|member| &member.work == work))
    {
        return Err("--endpoint-configs must select exact current Source members".into());
    }
    let session = super::native::current()?
        .ok_or("goal recover requires a current native session identity")?;
    let scope = super::native::scope(owner_loaded, false)?;
    scope.require_owner()?;
    let owner = scope
        .owner
        .as_ref()
        .ok_or("goal recover requires the fixed native session owner")?;
    let mut source_members = Vec::with_capacity(members.len());
    for member in members {
        let path = overrides
            .get(&member.work)
            .map(String::as_str)
            .unwrap_or(&member.config_path);
        let endpoint = crate::config::load(Some(path)).map_err(|error| {
            format!(
                "endpoint configuration for {} is unavailable: {error}",
                member.work
            )
        })?;
        let endpoint_config = config_binding(&endpoint)?;
        if endpoint_config.product_root != member.product_root
            || endpoint_config.state_root != member.state_root
            || endpoint_config.project_identity_sha256 != member.project_identity_sha256
            || endpoint_config.local_identity_sha256 != member.local_identity_sha256
            || endpoint.project_id != source.project_id
        {
            return Err(format!(
                "endpoint configuration for {} does not exactly match its member",
                member.work
            ));
        }
        super::journey::validate_recovery_endpoint(&endpoint, &member)?;
        source_members.push(SourceMember {
            member_sha256: super::journey::member_identity(&member),
            member,
            endpoint_config,
        });
    }
    let mut historical_fixtures = validated_state(record)?
        .map(|state| state.active.historical_fixtures)
        .unwrap_or_default();
    if historical_fixtures.iter().any(|fixture| {
        source_members
            .iter()
            .any(|member| member.member.work == fixture.work)
    }) {
        return Err("a previously retained historical member is now part of the Source set; preserve the decision and resolve the membership change".into());
    }
    for work in selected {
        let member = by_work.get(&work).expect("selected member checked above");
        let fixture = HistoricalFixture {
            work: work.clone(),
            start_event_sha256: member.start_event_sha256.clone(),
            member_sha256: super::journey::member_identity(member),
            reason: request.reason.to_owned(),
        };
        if let Some(existing) = historical_fixtures.iter().find(|item| item.work == work) {
            if existing.start_event_sha256 != fixture.start_event_sha256
                || existing.member_sha256 != fixture.member_sha256
            {
                return Err(
                    "historical fixture identity changed; preserve the recorded disposition".into(),
                );
            }
        } else {
            historical_fixtures.push(fixture);
        }
    }
    historical_fixtures.sort_by(|a, b| a.work.cmp(&b.work));
    let mut active = ActiveDecision {
        goal_id: record["goalId"]
            .as_str()
            .ok_or("native Goal identity is missing")?
            .to_owned(),
        applied_from_revision: record["revision"]
            .as_u64()
            .ok_or("native Goal revision is missing")?,
        applied_from_event_sha256: record["eventSha256"]
            .as_str()
            .ok_or("native Goal event identity is missing")?
            .to_owned(),
        host: session.host.to_owned(),
        session_key: session.key,
        owner_identity_sha256: owner.identity_sha256.clone(),
        owner_project_id: owner.project_id.clone(),
        reason: request.reason.to_owned(),
        source_config,
        source_members,
        historical_fixtures,
        decision_sha256: String::new(),
    };
    active.decision_sha256 = decision_digest(&active)?;
    let preview = json!({
        "version": VERSION,
        "requestId": request.request_id,
        "reason": request.reason,
        "endpointConfigSelections": overrides,
        "goalId": active.goal_id,
        "goalRevision": active.applied_from_revision,
        "goalEventSha256": active.applied_from_event_sha256,
        "sourceConfig": active.source_config,
        "sourceMembers": active.source_members,
        "historicalFixtures": active.historical_fixtures,
        "decisionSha256": active.decision_sha256,
    });
    Ok((active, hash::value(&preview)))
}

pub(crate) fn recover(
    root: &Path,
    owner_loaded: &Loaded,
    request: Request<'_>,
) -> Result<Value, String> {
    let scope = super::native::scope(owner_loaded, false)?;
    scope.require_owner()?;
    if !scope.is_session() {
        return Err("goal recover requires the original native Goal owner context".into());
    }
    let request_sha256 = request_digest(&request)?;
    if !request.apply && request.current_binding.is_some() {
        return Err(
            "--current-binding is accepted only with --apply; request a fresh preview".into(),
        );
    }
    if request.apply {
        if let Some(record) = super::with_history(root, |history| {
            if history.last().is_some_and(named_goal) {
                return Err(
                    "native Goal recovery cannot be used by a named-requirement Goal".into(),
                );
            }
            find_replay(
                history,
                request.request_id,
                &request_sha256,
                request.current_binding,
            )
        })? {
            return Ok(
                json!({"kind":"native_goal_recovery_replayed","readOnly":true,
                "event":record,"currentStatus":crate::emitted::goal_status(&owner_loaded.path)}),
            );
        }
    }
    let source = crate::config::load(Some(request.source_config))
        .map_err(|error| format!("--source-config is unavailable: {error}"))?;
    let current = super::read(root)?.ok_or("no native session Goal is incorporated")?;
    let (active, preview_sha256) = preview_decision(owner_loaded, &source, &current, &request)?;
    let owner_config = owner_loaded
        .path
        .to_str()
        .ok_or("owner configuration path is not valid UTF-8")?;
    let apply_command = crate::emitted::Route::new([
        "goal",
        "recover",
        "--source-config",
        request.source_config,
        "--historical-fixtures",
        request.historical_fixtures,
        "--endpoint-configs",
        request.endpoint_configs.unwrap_or("{}"),
        "--request-id",
        request.request_id,
        "--reason",
        request.reason,
        "--current-binding",
        preview_sha256.as_str(),
        "--apply",
        "--json",
    ])
    .config_text(owner_config)
    .within(16 * 1024);
    if !request.apply {
        return Ok(
            json!({"kind":"native_goal_recovery_preview","readOnly":true,
            "currentBinding":preview_sha256,"decision":active,"applyArgv":apply_command.argv_value(),
            "sameConfigRequired":apply_command.needs_config(),
            "sameExecutableRequired":apply_command.needs_executable()}),
        );
    }
    if request.current_binding != Some(preview_sha256.as_str()) {
        return Err("--current-binding does not match the exact current recovery preview".into());
    }
    let record = super::mutate_history(root, |history| {
        if request.apply {
            if let Some(saved) = find_replay(
                history,
                request.request_id,
                &request_sha256,
                request.current_binding,
            )? {
                return Ok(saved);
            }
        }
        let previous = history
            .last()
            .ok_or("no native session Goal is incorporated")?;
        let fresh_owner = crate::config::load(Some(
            owner_loaded
                .path
                .to_str()
                .ok_or("owner configuration path is not valid UTF-8")?,
        ))
            .map_err(|error| format!("owner configuration changed or became unavailable; reload and preview recovery: {error}"))?;
        let owner_scope = super::native::scope(&fresh_owner, false)?;
        owner_scope.require_owner()?;
        let fresh_source = crate::config::load(Some(request.source_config))
            .map_err(|error| format!("Source configuration changed or became unavailable; preview recovery again: {error}"))?;
        let (fresh, fresh_preview) =
            preview_decision(&fresh_owner, &fresh_source, previous, &request)?;
        if fresh_preview != preview_sha256 || fresh != active {
            return Err("native Goal recovery preview is stale; request a new preview".into());
        }
        if named_goal(previous) {
            return Err("native Goal recovery does not support named-requirement Goals".into());
        }
        let mut next = previous.clone();
        next.as_object_mut()
            .expect("validated Goal object")
            .remove("eventSha256");
        let revision = previous["revision"]
            .as_u64()
            .ok_or("native Goal revision is missing")?;
        next["revision"] = json!(revision + 1);
        next["predecessor"] = json!({"goalId":previous["goalId"],"revision":revision});
        next["successorOf"] = Value::Null;
        let mut fixtures = validated_state(previous)?
            .map(|state| state.active.historical_fixtures)
            .unwrap_or_default();
        if fixtures.iter().any(|fixture| {
            active
                .source_members
                .iter()
                .any(|member| member.member.work == fixture.work)
        }) {
            return Err("a previously retained historical member is now part of the Source set; preserve the decision and resolve the membership change".into());
        }
        for fixture in active.historical_fixtures.clone() {
            if let Some(existing) = fixtures.iter().find(|item| item.work == fixture.work) {
                if existing != &fixture {
                    return Err("historical fixture disposition cannot be silently changed".into());
                }
            } else {
                fixtures.push(fixture);
            }
        }
        let mut active = active.clone();
        active.historical_fixtures = fixtures;
        active.decision_sha256 = decision_digest(&active)?;
        let mut requests = validated_state(previous)?
            .map(|state| state.requests)
            .unwrap_or_default();
        if requests.len() >= MAX_REQUESTS {
            return Err("native Goal recovery request bound reached".into());
        }
        requests.push(AppliedRequest {
            request_id: request.request_id.to_owned(),
            request_sha256,
            preview_sha256,
        });
        next[FIELD] = serde_json::to_value(RecoveryState {
            version: VERSION,
            active,
            requests,
        })
        .map_err(|error| error.to_string())?;
        Ok(super::sealed(next, Some(previous)))
    })?;
    Ok(
        json!({"kind":"native_goal_recovery_applied","readOnly":false,
        "event":record,"currentStatus":crate::emitted::goal_status(&owner_loaded.path)}),
    )
}

fn find_replay(
    history: &[Value],
    request_id: &str,
    request_sha256: &str,
    current_binding: Option<&str>,
) -> Result<Option<Value>, String> {
    for record in history {
        let Some(state) = validated_state(record)? else {
            continue;
        };
        if let Some(saved) = state
            .requests
            .iter()
            .find(|saved| saved.request_id == request_id)
        {
            if saved.request_sha256 != request_sha256 {
                return Err(
                    "--request-id was already used with different recovery arguments".into(),
                );
            }
            if current_binding != Some(saved.preview_sha256.as_str()) {
                return Err("replayed --request-id requires its original --current-binding".into());
            }
            return Ok(Some(record.clone()));
        }
    }
    Ok(None)
}

#[cfg(test)]
mod tests;

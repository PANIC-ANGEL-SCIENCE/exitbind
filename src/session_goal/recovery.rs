// Recovers owner-bound native Goals with unresolved journey state.
// Refuses this route for named-requirement Goals; see `src/session_goal.rs`.
//! Explicit, owner-bound recovery for a native Goal whose retained journey
//! contains members from a separately configured Source project.

mod configuration;
mod record;
mod retirement;

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
const GROUP_VERSION: u64 = 2;
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
    #[serde(default, skip_serializing_if = "Option::is_none")]
    mode: Option<String>,
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
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    groups: Vec<ActiveDecision>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    owner_config: Option<ConfigBinding>,
    requests: Vec<AppliedRequest>,
}

impl RecoveryState {
    fn decisions(&self) -> impl Iterator<Item = &ActiveDecision> {
        std::iter::once(&self.active).chain(self.groups.iter())
    }
}

fn same_group(left: &ConfigBinding, right: &ConfigBinding) -> bool {
    left.project_identity_sha256 == right.project_identity_sha256
        && left.local_identity_sha256 == right.local_identity_sha256
        && left.product_root == right.product_root
        && left.state_root == right.state_root
}

fn upsert_group(state: &mut RecoveryState, active: ActiveDecision, owner: ConfigBinding) {
    state.version = GROUP_VERSION;
    state.owner_config = Some(owner);
    if same_group(&state.active.source_config, &active.source_config) {
        state.active = active;
    } else if let Some(group) = state
        .groups
        .iter_mut()
        .find(|group| same_group(&group.source_config, &active.source_config))
    {
        *group = active;
    } else {
        state.groups.push(active);
    }
}

pub(crate) struct Context {
    pub(crate) source: Loaded,
    pub(crate) active: ActiveDecision,
    pub(crate) integration_sha256: String,
    pub(crate) grouped: bool,
    members: Vec<super::journey::Member>,
    pub(crate) endpoints: BTreeMap<String, Loaded>,
    pub(crate) historical: BTreeSet<String>,
    pub(crate) historical_items: Vec<Value>,
    pub(crate) retired_uncredited: BTreeSet<String>,
    pub(crate) retired_items: Vec<Value>,
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
    pub(crate) retirements: Option<&'a str>,
    pub(crate) source_writers_quiesced: bool,
    pub(crate) upsert_group: bool,
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

/// Offer only a read-only preview for one unambiguous foreign Source. The
/// actual preview and apply routes independently revalidate their inputs.
pub(crate) fn preview_form(owner_loaded: &Loaded, record: &Value) -> Option<Value> {
    fn candidate(owner_loaded: &Loaded, record: &Value) -> Result<Option<Value>, String> {
        if named_goal(record) || record["closure"]["closed"] != false || record.get(FIELD).is_some()
        {
            return Ok(None);
        }
        let scope = super::native::scope_readonly(owner_loaded)?;
        if !scope.is_session()
            || !scope.owner_matches
            || super::read_for_recovery(&scope.root)?.as_ref() != Some(record)
        {
            return Ok(None);
        }
        let owner = config_binding(owner_loaded)?;
        let journey = match super::journey::state(record)? {
            Some(journey) => journey,
            None => return Ok(None),
        };
        let foreign = journey
            .members
            .iter()
            .filter(|member| {
                member.project_identity_sha256 != owner.project_identity_sha256
                    || member.local_identity_sha256 != owner.local_identity_sha256
                    || member.product_root != owner.product_root
                    || member.state_root != owner.state_root
            })
            .collect::<Vec<_>>();
        let paths = foreign
            .iter()
            .map(|member| member.config_path.as_str())
            .collect::<BTreeSet<_>>();
        if paths.len() != 1 {
            return Ok(None);
        }
        let source_path = *paths.iter().next().expect("one source path");
        let source = crate::config::load(Some(source_path))?;
        let request = Request {
            source_config: source_path,
            historical_fixtures: "",
            endpoint_configs: None,
            retirements: None,
            source_writers_quiesced: false,
            upsert_group: false,
            request_id: "preview",
            reason: "preview",
            apply: false,
            current_binding: None,
        };
        let (decision, _) = preview_decision(
            owner_loaded,
            &source,
            record,
            &request,
            super::native::Access::ReadExisting,
        )?;
        let selected = decision
            .source_members
            .iter()
            .map(|item| item.member.work.as_str())
            .collect::<BTreeSet<_>>();
        let expected = foreign
            .iter()
            .map(|member| member.work.as_str())
            .collect::<BTreeSet<_>>();
        if selected != expected {
            return Ok(None);
        }
        // A status route is not a frozen preview. Decline to offer it if the
        // facts used to identify the Source changed during this observation.
        let owner_path = owner_loaded
            .path
            .to_str()
            .ok_or("owner configuration path is not valid UTF-8")?;
        let fresh_owner = crate::config::load(Some(owner_path))?;
        let fresh_source = crate::config::load(Some(source_path))?;
        let fresh_scope = super::native::scope_readonly(&fresh_owner)?;
        // Recovery preview has no write effects, but its existing CLI entry
        // point requires writable native storage. This check inspects existing
        // directories and access only; it does not create or change them.
        let preview_scope = super::native::scope(&fresh_owner, false)?;
        if config_binding(&fresh_owner)? != owner
            || config_binding(&fresh_source)? != decision.source_config
            || !fresh_scope.is_session()
            || !fresh_scope.owner_matches
            || fresh_scope.root != scope.root
            || !preview_scope.is_session()
            || !preview_scope.owner_matches
            || preview_scope.root != scope.root
            || super::read_for_recovery(&fresh_scope.root)?.as_ref() != Some(record)
        {
            return Ok(None);
        }
        let request_id = crate::emitted::slot("REQUEST_ID");
        let reason = crate::emitted::slot("REASON");
        let route = crate::emitted::Route::new([
            "goal",
            "recover",
            "--source-config",
            source_path,
            "--request-id",
            request_id.as_str(),
            "--reason",
            reason.as_str(),
            "--json",
        ])
        .config(&owner_loaded.path)
        .within(16 * 1024);
        if route.argv_value().is_null() {
            return Ok(None);
        }
        Ok(Some(json!({
            "kind":"native_goal_recovery_preview",
            "readOnly":true,
            "sourceConfig":source_path,
            "sourceWorks":selected,
            "command":route.argv_value(),
            "sameConfigRequired":route.needs_config(),
            "sameExecutableRequired":route.needs_executable(),
            "placeholders":["REQUEST_ID","REASON"],
            "inputConstraints":{
                "REQUEST_ID":{"minimumBytes":1,"maximumBytes":128,"nonBlank":true,"forbiddenCharacters":["NUL"]},
                "REASON":{"minimumBytes":1,"maximumBytes":1024,"nonBlank":true,"forbiddenCharacters":["NUL"]}
            },
            "integrationRequired":true,
            "meaning":"Preview only; Source acceptance and a checked, reviewed integration remain separate."
        })))
    }
    candidate(owner_loaded, record).ok().flatten()
}

fn context_for_decision(
    owner_loaded: &Loaded,
    record: &Value,
    active: ActiveDecision,
    exact_group: bool,
    owner_binding: Option<&ConfigBinding>,
) -> Result<Context, String> {
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
    if let Some(binding) = owner_binding {
        if binding.local_identity_sha256 != owner.identity_sha256
            || binding.project_id != owner.project_id
        {
            return Err("native Goal recovery owner configuration names another owner".into());
        }
        let current_owner = crate::config::load(Some(&binding.path)).map_err(|error| {
            format!(
                "saved Goal owner configuration is unavailable; preview recovery again: {error}"
            )
        })?;
        if config_binding(&current_owner)? != *binding {
            return Err("saved Goal owner configuration changed; preview recovery again".into());
        }
    }
    let source = crate::config::load(Some(&active.source_config.path)).map_err(|error| {
        format!("saved Source configuration is unavailable; refresh recovery: {error}")
    })?;
    if config_binding(&source)? != active.source_config {
        return Err("saved Source configuration changed; preview recovery again".into());
    }
    let current_members = source_members(&source, record, exact_group)?;
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
    let selection = retirement::selected(record, &active)?;
    if let Some(selection) = &selection {
        let requests = retirement::from_selection(selection);
        retirement::with_locks(&source, &requests, || {
            retirement::revalidate(&source, record, &active, selection)
        })?;
    }
    let mut retired_uncredited = BTreeSet::new();
    let mut retired_items = Vec::new();
    if let Some(selection) = &selection {
        for item in &selection.items {
            retired_uncredited.insert(item.member.work.clone());
            retired_items.push(json!({
                "work":item.member.work.as_str(),
                "state":"retired_uncredited",
                "reason":item.reason.as_str(),
                "startEventSha256":item.blocked.start_event_sha256.as_str(),
                "terminalEventSha256":item.blocked.head_event_sha256.as_str(),
                "governorSpent":item.blocked.governor_spent,
                "replacement":item.replacement.member.work.as_str(),
            }));
        }
    }
    let integration_sha256 = retirement::integration_digest(&active, selection.as_ref());
    Ok(Context {
        source,
        active,
        integration_sha256,
        grouped: exact_group,
        members: current_members,
        endpoints,
        historical,
        historical_items,
        retired_uncredited,
        retired_items,
    })
}

fn shared_goal_inputs(record: &Value) -> String {
    hash::value(&json!({
        "domain":"exitbind.native-goal-recovery-shared-inputs.v2",
        "goalId":record["goalId"],
        "goal":record["goal"],
        "continuation":record.get("continuation"),
        "admissionCoverage":record["journeyMembershipProtocol"].as_u64().map(|protocol| protocol >= 2),
        "journeyMembers":record.get("journeyMembers"),
    }))
}

pub(crate) fn shared_inputs(record: &Value) -> String {
    hash::value(
        &json!({"domain":"exitbind.native-goal-recovery-owner-shared.v2",
        "goalInputsSha256":shared_goal_inputs(record),
        "ownerConfig":record[FIELD].get("ownerConfig")}),
    )
}

/// The integration member names the exact Goal event observed before its
/// admission. Compare shared inputs with that validated event, rather than the
/// collection head, so migrating a still-current v1 integration is safe.
pub(super) fn shared_inputs_current(
    loaded: &Loaded,
    record: &Value,
    integration: &super::journey::Member,
) -> Result<bool, String> {
    let scope = super::native::scope(loaded, false)?;
    let history = super::records(&scope.root)?;
    let Some((index, observed)) = history.iter().enumerate().find(|(_, event)| {
        event["eventSha256"].as_str()
            == Some(integration.parent_observed_goal_event_sha256.as_str())
    }) else {
        return Ok(false);
    };
    let Some(current_index) = history
        .iter()
        .position(|event| event["eventSha256"] == record["eventSha256"])
    else {
        return Ok(false);
    };
    if current_index < index {
        return Ok(false);
    }
    if observed[FIELD]["version"] == VERSION {
        if current_index == index {
            return Ok(false);
        }
        let migration = history[index + 1..=current_index].iter().find(|event| {
            event["goalId"] == record["goalId"] && event[FIELD]["version"] == GROUP_VERSION
        });
        return Ok(migration
            .is_some_and(|event| event[FIELD]["ownerConfig"] == record[FIELD]["ownerConfig"])
            && shared_goal_inputs(observed) == shared_goal_inputs(record));
    }
    Ok(shared_inputs(observed) == shared_inputs(record))
}

/// Resolve only the group that actually owns this exact journey member.
/// Other groups are independently revalidated when they are used.
pub(super) fn context_for_member(
    owner_loaded: &Loaded,
    record: &Value,
    member: &super::journey::Member,
) -> Result<Option<Context>, String> {
    let Some(state) = validated_state(record)? else {
        return Ok(None);
    };
    let active = state.decisions().find(|decision| {
        decision
            .source_members
            .iter()
            .any(|item| item.member.work == member.work)
            || decision
                .historical_fixtures
                .iter()
                .any(|item| item.work == member.work)
    });
    active
        .map(|active| {
            context_for_decision(
                owner_loaded,
                record,
                active.clone(),
                state.version == GROUP_VERSION,
                state.owner_config.as_ref(),
            )
        })
        .transpose()
}

/// Resolve the one recovery group selected by this integration's actual
/// project/state/local identity, never by project ID or a config path alone.
pub(crate) fn context_for_source(
    loaded: &Loaded,
    record: &Value,
) -> Result<Option<Context>, String> {
    let Some(state) = validated_state(record)? else {
        return Ok(None);
    };
    let selected = config_binding(loaded)?;
    let active = state
        .decisions()
        .find(|active| same_group(&active.source_config, &selected));
    active
        .map(|active| {
            context_for_decision(
                loaded,
                record,
                active.clone(),
                state.version == GROUP_VERSION,
                state.owner_config.as_ref(),
            )
        })
        .transpose()
}

fn contexts(owner_loaded: &Loaded, record: &Value) -> Result<Vec<Context>, String> {
    let Some(state) = validated_state(record)? else {
        return Ok(Vec::new());
    };
    state
        .decisions()
        .map(|active| {
            context_for_decision(
                owner_loaded,
                record,
                active.clone(),
                state.version == GROUP_VERSION,
                state.owner_config.as_ref(),
            )
        })
        .collect()
}

/// Hold selected source Run locks before entering a Goal mutation which may
/// consume the retirement selection. A changed selection fails in `context`.
pub(crate) fn with_retirement_locks<T>(
    record: Option<&Value>,
    operation: impl FnOnce() -> Result<T, String>,
) -> Result<T, String> {
    let Some(record) = record else {
        return operation();
    };
    let Some(state) = validated_state(record)? else {
        return operation();
    };
    let Some(selection) = retirement::selected(record, &state.active)? else {
        return operation();
    };
    let source = crate::config::load(Some(&state.active.source_config.path))?;
    if config_binding(&source)? != state.active.source_config {
        return Err("saved Source configuration changed; preview recovery again".into());
    }
    retirement::with_locks(&source, &retirement::from_selection(&selection), operation)
}

pub(crate) fn retirement_binding(record: Option<&Value>) -> Result<Option<String>, String> {
    let Some(record) = record else {
        return Ok(None);
    };
    let Some(state) = validated_state(record)? else {
        return Ok(None);
    };
    Ok(retirement::selected(record, &state.active)?
        .as_ref()
        .map(retirement::digest))
}

/// Resolves only exact accepted endpoint references captured by the recovery,
/// or the exact currently accepted integration that supports that binding.
/// `None` leaves ordinary owner-project lookup unchanged.
pub(crate) fn result_ref_evidence(
    owner_loaded: &Loaded,
    record: &Value,
    reference: &str,
) -> Result<Option<RecoveryEvidence>, String> {
    for context in contexts(owner_loaded, record)? {
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
        if let Some(member) = context.members.first() {
            if let Some((integration, integration_config)) =
                super::journey::current_recovery_integration(owner_loaded, record, member)?
            {
                if integration == reference {
                    return Ok(
                        crate::run::result_ref_evidence(&integration_config, reference)?.map(
                            |evidence| RecoveryEvidence {
                                accepted: evidence.accepted,
                                artifact_current: evidence.artifact_current,
                                drift: evidence.drift,
                                inputs_current: evidence.inputs_current,
                            },
                        ),
                    );
                }
            }
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
    let mut value = json!({
        "sourceConfig": request.source_config,
        "historicalFixtures": historical_ids(request.historical_fixtures)?,
        "endpointConfigs": endpoint_overrides(request.endpoint_configs)?,
        "reason": request.reason,
    });
    if request.upsert_group {
        value["mode"] = json!("group-upsert");
    }
    if let Some(items) = retirement::request(request.retirements, request.source_writers_quiesced)?
    {
        value["retirements"] = json!(items);
        value["sourceWritersQuiesced"] = json!(true);
    }
    Ok(hash::value(&value))
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

fn source_members(
    source: &Loaded,
    record: &Value,
    exact_group: bool,
) -> Result<Vec<super::journey::Member>, String> {
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
                && (!exact_group
                    || member.local_identity_sha256 == source_config.local_identity_sha256)
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
    access: super::native::Access,
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
    let owner_config = request
        .upsert_group
        .then(|| config_binding(owner_loaded))
        .transpose()?;
    let existing = validated_state(record)?;
    if existing
        .as_ref()
        .is_some_and(|state| state.version == GROUP_VERSION)
        && !request.upsert_group
    {
        return Err("native Goal recovery v2 requires explicit --upsert-group".into());
    }
    if request.upsert_group {
        let owner_config = owner_config.as_ref().expect("group owner binding");
        if request.source_config != source_config.path {
            return Err(
                "--upsert-group requires the exact canonical absolute --source-config path".into(),
            );
        }
        if same_group(owner_config, &source_config) {
            return Err("a recovery group must select a foreign project or state identity".into());
        }
        if let Some(state) = &existing {
            if state
                .owner_config
                .as_ref()
                .is_some_and(|prior| prior.path != owner_config.path)
            {
                return Err("a recovery group cannot be selected through a different owner configuration alias".into());
            }
            if let Some(prior) = state
                .decisions()
                .find(|item| same_group(&item.source_config, &source_config))
            {
                if prior.source_config.path != source_config.path {
                    return Err("a recovery group cannot be selected through a different configuration alias".into());
                }
            }
            if request.retirements.is_some()
                && !same_group(&state.active.source_config, &source_config)
            {
                return Err(
                    "retirement selection must remain bound to its existing recovery group".into(),
                );
            }
        }
    }
    let mut all = super::journey::state(record)?
        .ok_or("native session Goal has no explicit journey membership protocol")?;
    let members = source_members(source, record, request.upsert_group)?;
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
    let scope = super::native::scope_with_access(owner_loaded, access)?;
    scope.require_owner()?;
    let session = scope
        .session
        .as_ref()
        .ok_or("goal recover requires a current native session identity")?;
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
        super::journey::validate_recovery_endpoint_with_access(&endpoint, &member, access)?;
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
    if request.upsert_group
        && existing.as_ref().is_some_and(|state| {
            (!state.groups.is_empty() || !same_group(&state.active.source_config, &source_config))
                && state.active.historical_fixtures != historical_fixtures
        })
    {
        return Err("a group upsert cannot change another group's historical dispositions".into());
    }
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
        session_key: session.key.clone(),
        owner_identity_sha256: owner.identity_sha256.clone(),
        owner_project_id: owner.project_id.clone(),
        reason: request.reason.to_owned(),
        source_config,
        source_members,
        historical_fixtures,
        decision_sha256: String::new(),
    };
    active.decision_sha256 = decision_digest(&active)?;
    if request.upsert_group {
        let mut candidate = existing.unwrap_or_else(|| RecoveryState {
            version: GROUP_VERSION,
            active: active.clone(),
            groups: Vec::new(),
            owner_config: None,
            requests: Vec::new(),
        });
        if record.get(retirement::FIELD).is_some()
            && same_group(&candidate.active.source_config, &active.source_config)
            && request.retirements.is_none()
        {
            return Err("an existing retirement selection must be explicitly reselected for the new recovery".into());
        }
        upsert_group(
            &mut candidate,
            active.clone(),
            owner_config.clone().expect("group owner binding"),
        );
        candidate.requests.push(AppliedRequest {
            request_id: request.request_id.to_owned(),
            request_sha256: request_digest(request)?,
            preview_sha256: "0".repeat(64),
            mode: Some("group-upsert".into()),
        });
        record::validate_state(&candidate)?;
    }
    let mut preview = json!({
        "version": if request.upsert_group { GROUP_VERSION } else { VERSION },
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
    if request.upsert_group {
        preview["mode"] = json!("group-upsert");
        preview["ownerConfig"] = json!(owner_config);
    }
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
            let historical_selection = record.get(retirement::FIELD).is_some();
            let mut response = json!({"kind":"native_goal_recovery_replayed","readOnly":true,
                "event":record,"currentStatus":crate::emitted::goal_status(&owner_loaded.path)});
            if historical_selection {
                response["historical"] = json!(true);
                response["meaning"] = json!("The saved event is historical evidence; only the latest Goal record can authorize a current selection.");
            }
            return Ok(response);
        }
    }
    let source = crate::config::load(Some(request.source_config))
        .map_err(|error| format!("--source-config is unavailable: {error}"))?;
    let retired = retirement::request(request.retirements, request.source_writers_quiesced)?;
    if let Some(items) = retired.as_deref() {
        return retirement::with_locks(&source, items, || {
            recover_current(
                root,
                owner_loaded,
                &request,
                &source,
                Some(items),
                &request_sha256,
            )
        });
    }
    recover_current(root, owner_loaded, &request, &source, None, &request_sha256)
}

fn recover_current(
    root: &Path,
    owner_loaded: &Loaded,
    request: &Request<'_>,
    source: &Loaded,
    retired: Option<&[retirement::RequestItem]>,
    request_sha256: &str,
) -> Result<Value, String> {
    let current = if request.apply {
        super::read(root)?
    } else {
        super::read_for_recovery(root)?
    }
    .ok_or("no native session Goal is incorporated")?;
    let (active, legacy_preview_sha256) = preview_decision(
        owner_loaded,
        source,
        &current,
        request,
        super::native::Access::RequireWritable,
    )?;
    let prior_recovery = if retired.is_some() {
        Some(
            validated_state(&current)?
                .ok_or("uncredited retirement requires an existing active recovery decision")?
                .active
                .decision_sha256,
        )
    } else {
        None
    };
    let selection = retired
        .map(|items| {
            retirement::observe(
                source,
                &current,
                &active,
                items,
                prior_recovery
                    .as_deref()
                    .expect("retirement has prior recovery"),
            )
        })
        .transpose()?;
    let preview_sha256 = retirement_preview(&legacy_preview_sha256, selection.as_ref());
    let owner_config = owner_loaded
        .path
        .to_str()
        .ok_or("owner configuration path is not valid UTF-8")?;
    let mut apply_command = crate::emitted::Route::new([
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
    ]);
    if let Some(raw) = request.retirements {
        apply_command = apply_command.words(["--retirements", raw, "--source-writers-quiesced"]);
    }
    if request.upsert_group {
        apply_command = apply_command.words(["--upsert-group"]);
    }
    let apply_route = apply_command
        .words([
            "--current-binding",
            preview_sha256.as_str(),
            "--apply",
            "--json",
        ])
        .config_text(owner_config);
    let apply_command = apply_route.within(16 * 1024);
    if !request.apply {
        let mut response = json!({"kind":"native_goal_recovery_preview","readOnly":true,
            "currentBinding":preview_sha256,"decision":&active,"applyArgv":apply_command.argv_value(),
            "sameConfigRequired":apply_command.needs_config(),
            "sameExecutableRequired":apply_command.needs_executable()});
        if request.upsert_group {
            response["mode"] = json!("group-upsert");
            response["ownerConfig"] = json!(config_binding(owner_loaded)?);
            let preserved = validated_state(&current)?.map_or_else(Vec::new, |state| {
                state
                    .decisions()
                    .filter(|group| !same_group(&group.source_config, &active.source_config))
                    .map(|group| {
                        json!({"decisionSha256":group.decision_sha256,
                        "projectIdentitySha256":group.source_config.project_identity_sha256,
                        "localIdentitySha256":group.source_config.local_identity_sha256})
                    })
                    .collect::<Vec<_>>()
            });
            response["preservedGroups"] = json!(preserved);
        }
        if let Some(selection) = &selection {
            response["retirementSelection"] = json!(selection);
            response["sourceWriterAttestation"] = json!("The owner attests that all source Work writers, including old producers, remain quiescent through each governed use. This does not fence an already-running writer or waive any observable refusal.");
            if apply_command.needs_config() || apply_command.needs_executable() {
                response["applyArgv"] = Value::Null;
                response["applyArgvState"] = json!("unavailable");
                response["meaning"] = json!("A complete apply command is unavailable; shorten the explicit selection or restore the executable and request a fresh preview.");
            }
        }
        return Ok(response);
    }
    if request.current_binding != Some(preview_sha256.as_str()) {
        return Err("--current-binding does not match the exact current recovery preview".into());
    }
    let record = super::mutate_history(root, owner_loaded, |history| {
        if request.apply {
            if let Some(saved) = find_replay(
                history,
                request.request_id,
                request_sha256,
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
        let (fresh, fresh_legacy_preview) = preview_decision(
            &fresh_owner,
            &fresh_source,
            previous,
            request,
            super::native::Access::RequireWritable,
        )?;
        let fresh_prior = if retired.is_some() {
            Some(
                validated_state(previous)?
                    .ok_or("uncredited retirement requires an existing active recovery decision")?
                    .active
                    .decision_sha256,
            )
        } else {
            None
        };
        let fresh_selection = retired
            .map(|items| {
                retirement::observe(
                    &fresh_source,
                    previous,
                    &fresh,
                    items,
                    fresh_prior
                        .as_deref()
                        .expect("retirement has prior recovery"),
                )
            })
            .transpose()?;
        let fresh_preview = retirement_preview(&fresh_legacy_preview, fresh_selection.as_ref());
        if fresh_preview != preview_sha256 || fresh != active || fresh_selection != selection {
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
        if selection.is_some() {
            fixtures.sort_by(|left, right| left.work.cmp(&right.work));
        }
        let mut active = active.clone();
        active.historical_fixtures = fixtures;
        active.decision_sha256 = decision_digest(&active)?;
        let previous_state = validated_state(previous)?;
        let updates_primary = !request.upsert_group
            || previous_state.as_ref().map_or(true, |state| {
                same_group(&state.active.source_config, &active.source_config)
            });
        if let Some(selection) = &selection {
            if !updates_primary {
                return Err(
                    "retirement selection must remain bound to its existing recovery group".into(),
                );
            }
            retirement::validate(selection, &active)?;
            next[retirement::FIELD] =
                serde_json::to_value(selection).map_err(|error| error.to_string())?;
        } else if updates_primary && previous.get(retirement::FIELD).is_some() {
            return Err("an existing retirement selection must be explicitly reselected for the new recovery".into());
        }
        let mut state = previous_state.unwrap_or_else(|| RecoveryState {
            version: if request.upsert_group {
                GROUP_VERSION
            } else {
                VERSION
            },
            active: active.clone(),
            groups: Vec::new(),
            owner_config: None,
            requests: Vec::new(),
        });
        if state.requests.len() >= MAX_REQUESTS {
            return Err("native Goal recovery request bound reached".into());
        }
        if request.upsert_group {
            upsert_group(&mut state, active, config_binding(&fresh_owner)?);
        } else {
            state.active = active;
        }
        state.requests.push(AppliedRequest {
            request_id: request.request_id.to_owned(),
            request_sha256: request_sha256.to_owned(),
            preview_sha256,
            mode: request.upsert_group.then(|| "group-upsert".to_owned()),
        });
        record::validate_state(&state)?;
        next[FIELD] = serde_json::to_value(state).map_err(|error| error.to_string())?;
        Ok(super::sealed(next, Some(previous)))
    })?;
    Ok(
        json!({"kind":"native_goal_recovery_applied","readOnly":false,
        "event":record,"currentStatus":crate::emitted::goal_status(&owner_loaded.path)}),
    )
}

fn retirement_preview(legacy: &str, selection: Option<&retirement::Selection>) -> String {
    selection.map_or_else(
        || legacy.to_owned(),
        |selection| {
            hash::value(
                &json!({"domain":"exitbind.native-goal-retirement-preview.v1",
            "recoveryPreviewSha256":legacy,"selectionSha256":retirement::digest(selection)}),
            )
        },
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

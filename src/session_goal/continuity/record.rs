//! Revision-fenced continuation mutations.

use super::*;
use std::io::Read;

fn next_record(previous: &Value, work_id: &str) -> Result<Value, String> {
    if previous["goalId"] != work_id || previous["continuation"]["work"] != work_id {
        return Err("continuation is not bound to this current work".into());
    }
    if previous["closure"]["closed"] == true {
        return Err("closed canonical goal cannot be mutated".into());
    }
    let prior = previous["revision"]
        .as_u64()
        .ok_or("goal revision missing")?;
    let mut record = previous.clone();
    record
        .as_object_mut()
        .ok_or("goal record malformed")?
        .remove("eventSha256");
    record["revision"] = json!(prior + 1);
    record["predecessor"] = json!({"goalId":work_id,"revision":prior});
    record["successorOf"] = Value::Null;
    record["closure"] = json!({"closed":false,"revision":null,"resultRefs":[]});
    Ok(record)
}

fn require_current(record: &Value, input: &Value) -> Result<(), String> {
    if revision(input, "expectedRevision")? != record["revision"].as_u64().unwrap_or(0) {
        return Err("stale canonical goal revision".into());
    }
    Ok(())
}

fn binding(record: &Value, input: &Value) -> Result<(), String> {
    if revision(input, "bindingRevision")?
        != record["continuation"]["binding"]["revision"]
            .as_u64()
            .ok_or("no current lead binding")?
    {
        return Err("stale lead binding revision".into());
    }
    Ok(())
}

fn apply(loaded: &Loaded, work_id: &str, previous: &Value, input: &Value) -> Result<Value, String> {
    let action = field(input, "action", 32)?;
    let mut record = next_record(previous, work_id)?;
    let new_revision = record["revision"].clone();
    match action {
        "bind" => {
            shape(
                input,
                &[
                    "action",
                    "expectedRevision",
                    "expectedBindingRevision",
                    "host",
                    "session",
                    "hostVersion",
                ],
            )?;
            let host = field(input, "host", 32)?;
            if !matches!(host, "codex" | "claude") {
                return Err("unsupported native host".into());
            }
            let session = field(input, "session", 256)?;
            let host_version = field(input, "hostVersion", 64)?;
            let current = &previous["continuation"]["binding"];
            if current["host"] == host
                && current["session"] == session
                && current["hostVersion"] == host_version
            {
                return Ok(previous.clone());
            }
            require_current(previous, input)?;
            let expected = revision(input, "expectedBindingRevision")?;
            let old = current["revision"].as_u64().unwrap_or(0);
            if expected != old {
                return Err("stale or conflicting lead binding".into());
            }
            record["continuation"]["binding"] = json!({
                "revision":old + 1,"host":host,"session":session,"hostVersion":host_version,
                "sourceClass":"host_reported","previousRevision":old,
            });
        }
        "cover" => {
            shape(
                input,
                &[
                    "action",
                    "expectedRevision",
                    "bindingRevision",
                    "sourceSha256",
                ],
            )?;
            require_current(previous, input)?;
            binding(previous, input)?;
            if input["sourceSha256"] != previous["continuation"]["source"]["sha256"] {
                return Err("source coverage assertion is stale".into());
            }
            record["continuation"]["source"]["coverageConfirmed"] = json!(true);
            record["continuation"]["source"]["coverageClass"] =
                json!("lead_asserted_review_required");
        }
        "refine" => {
            shape(
                input,
                &[
                    "action",
                    "expectedRevision",
                    "bindingRevision",
                    "id",
                    "text",
                    "sourceRef",
                    "expectedRequirementRevision",
                ],
            )?;
            require_current(previous, input)?;
            binding(previous, input)?;
            let id = field(input, "id", 64)?;
            let text = field(input, "text", 1024)?;
            let source_ref = field(input, "sourceRef", 256)?;
            let expected = revision(input, "expectedRequirementRevision")?;
            let requirements = record["continuation"]["requirements"]
                .as_array_mut()
                .ok_or("requirements malformed")?;
            let requirement = requirements
                .iter_mut()
                .find(|x| x["id"] == id)
                .ok_or("requirement is not in source coverage")?;
            let old = requirement["revision"]
                .as_u64()
                .ok_or("requirement revision missing")?;
            if expected != old || requirement["text"] == text {
                return Err("stale or empty requirement refinement".into());
            }
            let previous_text = requirement["text"].clone();
            let previous_source = requirement["sourceRef"].clone();
            if requirement["history"].as_array().is_none() {
                requirement["history"] = json!([]);
            }
            let history = requirement["history"]
                .as_array_mut()
                .ok_or("requirement history malformed")?;
            if history.len() >= MAX_ITEMS {
                return Err("requirement refinement history bound exceeded".into());
            }
            history.push(json!({"revision":old,"text":previous_text,"sourceRef":previous_source}));
            requirement["text"] = json!(text);
            requirement["revision"] = json!(old + 1);
            requirement["sourceRef"] = json!(source_ref);
            requirement["sourceClass"] = json!("operator_asserted_refinement");
            let ledger = verify_work(loaded, work_id)?;
            let (_, events, _) = run::ledger::load(loaded, &ledger)?;
            requirement["checkFloorIndex"] = json!(events.len());
        }
        "correct" => {
            shape(
                input,
                &[
                    "action",
                    "expectedRevision",
                    "bindingRevision",
                    "id",
                    "text",
                    "scope",
                    "sourceRef",
                    "supersedes",
                ],
            )?;
            require_current(previous, input)?;
            binding(previous, input)?;
            let id = field(input, "id", 64)?;
            let text = field(input, "text", 1024)?;
            let scope = field(input, "scope", 128)?;
            let source_ref = field(input, "sourceRef", 256)?;
            let supersedes = input["supersedes"].as_str();
            let corrections = record["continuation"]["corrections"]
                .as_array_mut()
                .ok_or("corrections malformed")?;
            if corrections.len() >= MAX_ITEMS || corrections.iter().any(|x| x["id"] == id) {
                return Err("correction identity or bound exceeded".into());
            }
            if let Some(prior) = supersedes {
                if !corrections
                    .iter()
                    .any(|x| x["id"] == prior && x["scope"] == scope)
                {
                    return Err("correction supersession is not in scope".into());
                }
            }
            corrections.push(json!({"id":id,"text":text,"scope":scope,"sourceRef":source_ref,"supersedes":supersedes,"sourceClass":"operator_asserted","revision":new_revision}));
        }
        "intent" => {
            shape(
                input,
                &[
                    "action",
                    "expectedRevision",
                    "bindingRevision",
                    "id",
                    "parametersSha256",
                    "description",
                ],
            )?;
            let id = field(input, "id", 64)?;
            let params = field(input, "parametersSha256", 64)?;
            if !valid_hash(params) {
                return Err("operation parameters hash is invalid".into());
            }
            let description = field(input, "description", 512)?;
            let operations = previous["continuation"]["operations"]
                .as_array()
                .ok_or("operations malformed")?;
            if let Some(old) = operations.iter().find(|x| x["id"] == id) {
                if old["parametersSha256"] == params && old["description"] == description {
                    return Ok(previous.clone());
                }
                return Err("same request identity has conflicting parameters".into());
            }
            require_current(previous, input)?;
            binding(previous, input)?;
            let operations = record["continuation"]["operations"]
                .as_array_mut()
                .ok_or("operations malformed")?;
            if operations.len() >= MAX_ITEMS {
                return Err("operation bound exceeded".into());
            }
            operations.push(json!({"id":id,"description":description,"parametersSha256":params,
                "origin":previous["continuation"]["binding"],"status":"uncertain","handle":null,"resultEventSha256":null,
                "intentRevision":new_revision}));
        }
        "observe" => {
            shape(
                input,
                &[
                    "action",
                    "expectedRevision",
                    "bindingRevision",
                    "id",
                    "status",
                    "handle",
                    "resultEventSha256",
                ],
            )?;
            require_current(previous, input)?;
            binding(previous, input)?;
            let id = field(input, "id", 64)?;
            let status = field(input, "status", 32)?;
            if !matches!(status, "dispatched" | "completed" | "failed" | "uncertain") {
                return Err("invalid observed operation status".into());
            }
            let operations = record["continuation"]["operations"]
                .as_array_mut()
                .ok_or("operations malformed")?;
            let op = operations
                .iter_mut()
                .find(|x| x["id"] == id)
                .ok_or("operation not found")?;
            if op["origin"]["host"] != previous["continuation"]["binding"]["host"] {
                return Err("origin host must observe its operation".into());
            }
            if op["status"] == "completed" || op["status"] == "failed" {
                return Err("terminal operation cannot be observed again".into());
            }
            if let Some(handle) = input["handle"].as_str() {
                if handle.is_empty() || handle.len() > 256 {
                    return Err("native handle is invalid".into());
                }
                op["handle"] = json!({"host":op["origin"]["host"],"session":op["origin"]["session"],"id":handle});
            }
            if status == "completed" {
                let event = field(input, "resultEventSha256", 64)?;
                if !valid_hash(event) {
                    return Err("result event hash is invalid".into());
                }
                let ledger = verify_work(loaded, work_id)?;
                run::inspect_event(loaded, &ledger, event)?;
                op["resultEventSha256"] = json!(event);
            }
            op["status"] = json!(status);
            op["observedBy"] = previous["continuation"]["binding"].clone();
        }
        "support" => {
            shape(
                input,
                &[
                    "action",
                    "expectedRevision",
                    "bindingRevision",
                    "requirementId",
                    "requirementRevision",
                    "resultEventSha256",
                    "conditionsSha256",
                ],
            )?;
            require_current(previous, input)?;
            binding(previous, input)?;
            let id = field(input, "requirementId", 64)?;
            let wanted = revision(input, "requirementRevision")?;
            let requirement = previous["continuation"]["requirements"]
                .as_array()
                .and_then(|xs| xs.iter().find(|x| x["id"] == id))
                .ok_or("requirement is not in source coverage")?;
            if requirement["revision"] != wanted {
                return Err("support targets stale requirement revision".into());
            }
            let event = field(input, "resultEventSha256", 64)?;
            let conditions = field(input, "conditionsSha256", 64)?;
            if !valid_hash(event) || !valid_hash(conditions) {
                return Err("support hash is invalid".into());
            }
            let ledger = verify_work(loaded, work_id)?;
            let observed = run::inspect_event(loaded, &ledger, event)?;
            let result = &observed["event"];
            if observed["eventIndex"].as_u64().unwrap_or(0)
                < requirement["checkFloorIndex"].as_u64().unwrap_or(0)
            {
                return Err("check predates the current requirement revision".into());
            }
            if result["action"] != "check"
                || result["acquisition"] != "observed"
                || result["result"]["kind"] != "exit"
                || result["result"]["code"] != 0
                || result["requirementId"] != id
            {
                return Err(
                    "support requires a passing check bound to the named requirement".into(),
                );
            }
            if !evidence_verified(&observed) {
                return Err("check evidence is missing or unverified".into());
            }
            let (current_inputs, current_conditions) = current_conditions(loaded)?;
            if result["inputsSha256"] != current_inputs
                || conditions != current_conditions
                || !acquired_config_current(loaded, result)
            {
                return Err("check or support conditions no longer match current inputs".into());
            }
            let supports = record["continuation"]["supports"]
                .as_array_mut()
                .ok_or("supports malformed")?;
            if supports.len() >= MAX_ITEMS {
                return Err("support bound exceeded".into());
            }
            if supports
                .iter()
                .any(|x| x["requirementId"] == id && x["resultEventSha256"] == event)
            {
                return Err("duplicate support".into());
            }
            supports.push(json!({"type":"supports","requirementId":id,"requirementRevision":wanted,
                "sourceSha256":requirement["sourceSha256"],"resultEventSha256":event,"subjectSha256":result["subjectSha256"],
                "inputsSha256":result["inputsSha256"],"conditionsSha256":conditions,
                "acquiredConfigSha256":result["configSha256"],
                "acquisition":result["acquisition"],"producer":result["producer"],"sourceClass":"observed_check"}));
        }
        "diagnose" => {
            shape(
                input,
                &[
                    "action",
                    "expectedRevision",
                    "bindingRevision",
                    "operationId",
                    "class",
                    "text",
                    "evidenceEventSha256",
                    "conditionsSha256",
                    "invalidateWhen",
                ],
            )?;
            require_current(previous, input)?;
            binding(previous, input)?;
            let id = field(input, "operationId", 64)?;
            if !previous["continuation"]["operations"]
                .as_array()
                .is_some_and(|xs| xs.iter().any(|x| x["id"] == id))
            {
                return Err("diagnosis operation is unknown".into());
            }
            let class = field(input, "class", 32)?;
            if !matches!(
                class,
                "hypothesis" | "observed_failure" | "diagnosed_cause" | "verified_repair"
            ) {
                return Err("invalid diagnosis class".into());
            }
            let text = field(input, "text", 1024)?;
            let conditions = field(input, "conditionsSha256", 64)?;
            let invalidation = field(input, "invalidateWhen", 512)?;
            if !valid_hash(conditions) {
                return Err("diagnosis conditions hash is invalid".into());
            }
            let evidence = input["evidenceEventSha256"].as_str();
            if let Some(event) = evidence {
                let ledger = verify_work(loaded, work_id)?;
                run::inspect_event(loaded, &ledger, event)?;
            }
            if matches!(class, "diagnosed_cause" | "verified_repair") && evidence.is_none() {
                return Err("diagnosed or verified cause needs source evidence".into());
            }
            let list = record["continuation"]["diagnoses"]
                .as_array_mut()
                .ok_or("diagnoses malformed")?;
            if list.len() >= MAX_ITEMS {
                return Err("diagnosis bound exceeded".into());
            }
            list.push(json!({"operationId":id,"class":class,"text":text,"evidenceEventSha256":evidence,
                "conditionsSha256":conditions,"invalidateWhen":invalidation,"sourceClass":"lead_reported","revision":new_revision}));
        }
        "child" => {
            shape(
                input,
                &[
                    "action",
                    "expectedRevision",
                    "bindingRevision",
                    "assignment",
                    "nativeChild",
                    "resultSha256",
                    "resultText",
                ],
            )?;
            let assignment = field(input, "assignment", 128)?;
            let child = field(input, "nativeChild", 256)?;
            let result = field(input, "resultText", 8192)?;
            let digest = field(input, "resultSha256", 64)?;
            if hash::text(result) != digest {
                return Err("native child result digest mismatch".into());
            }
            let origin = &previous["continuation"]["binding"];
            let key = child_key(origin, child);
            if let Some(old) = previous["continuation"]["children"]
                .as_array()
                .and_then(|xs| {
                    xs.iter().find(|x| {
                        x["childKey"]
                            .as_str()
                            .map(str::to_owned)
                            .unwrap_or_else(|| {
                                child_key(
                                    &x["origin"],
                                    x["nativeChild"].as_str().unwrap_or_default(),
                                )
                            })
                            == key
                    })
                })
            {
                if old["assignment"] == assignment && old["resultSha256"] == digest {
                    return Ok(previous.clone());
                }
                return Err("native child identity has conflicting output".into());
            }
            require_current(previous, input)?;
            binding(previous, input)?;
            let list = record["continuation"]["children"]
                .as_array_mut()
                .ok_or("children malformed")?;
            if list.len() >= MAX_ITEMS {
                return Err("native child bound exceeded".into());
            }
            list.push(json!({"assignment":assignment,"nativeChild":child,"childKey":key,"origin":origin,
                "resultSha256":digest,"resultText":result,"sourceClass":"host_reported_native_child","revision":new_revision}));
        }
        _ => return Err("unsupported continuation action".into()),
    }
    Ok(sealed(record, Some(previous)))
}

pub(crate) fn continuation_record(loaded: &Loaded, work_id: &str) -> Result<Value, String> {
    verify_work(loaded, work_id)?;
    let mut bytes = Vec::new();
    std::io::stdin()
        .take(MAX_INPUT + 1)
        .read_to_end(&mut bytes)
        .map_err(|e| e.to_string())?;
    if bytes.len() as u64 > MAX_INPUT {
        return Err("continuation input exceeds 16 KiB".into());
    }
    let input: Value =
        serde_json::from_slice(&bytes).map_err(|_| "continuation input is not JSON".to_owned())?;
    let action = field(&input, "action", 32)?;
    let (record, appended) = if action == "init" {
        init(loaded, work_id, &input)?
    } else {
        let mut appended = true;
        let record = mutate(&loaded.state_root, |previous| {
            let previous = previous.ok_or("continuation has not been initialized")?;
            let record = apply(loaded, work_id, previous, &input)?;
            appended = &record != previous;
            Ok(record)
        })?;
        (record, appended)
    };
    crate::work::compact::continuation_mutation(&record, work_id, action, appended, &loaded.path)
}

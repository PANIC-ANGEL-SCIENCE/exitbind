//! Source material and current evidence for the selected actor.
use super::{classified, copy_if_present, nonempty};
use crate::evidence::hash;
use serde_json::{json, Map, Value};

pub(super) fn source_context(
    actor: &mut Map<String, Value>,
    context: &Value,
    evidence: &Value,
    work: &str,
) -> Result<(), String> {
    let sources = context["evidence"]
        .as_array()
        .ok_or("native source metadata is unavailable")?;
    let items = evidence["items"]
        .as_array()
        .ok_or("native evidence items are unavailable")?;
    let mut statuses = Vec::new();
    for source in sources {
        classified(
            source,
            &[
                "kind",
                "evidenceId",
                "artifact",
                "rawEventRef",
                "checkEventRef",
                "checkLogRef",
                "status",
                "targetEventSha256",
                "checkEventSha256",
                "requirementId",
                "requirementText",
                "checker",
                "checkerSha256",
                "origin",
                "acquisition",
                "logStatus",
                "result",
                "freshness",
                "strength",
            ],
            "native source metadata",
        )?;
        if !matches!(
            source["kind"].as_str(),
            Some("submission" | "submit" | "check" | "preservation")
        ) {
            return Err("native source kind is unclassified".into());
        }
        for name in ["rawEventRef", "checkEventRef", "checkLogRef"] {
            if source[name].is_null() {
                continue;
            }
            classified(
                &source[name],
                &[
                    "kind",
                    "work",
                    "sha256",
                    "headEventSha256",
                    "eventCount",
                    "selector",
                    "exact",
                    "id",
                    "checkEventSha256",
                    "stdout",
                    "stderr",
                ],
                "native source reference",
            )?;
            if source[name]["work"] != work {
                return Err("native source reference is not current for this Work".into());
            }
            if source[name]["exact"] != true
                || !source[name]["id"].is_string()
                || (name == "checkLogRef" && source[name]["kind"] != "check_log")
                || (name != "checkLogRef" && source[name]["kind"] != "ledger_event")
            {
                return Err("native source reference is malformed".into());
            }
            for stream in ["stdout", "stderr"] {
                if source[name][stream].is_object() {
                    classified(
                        &source[name][stream],
                        &["kind", "sha256", "bytes", "exact"],
                        "native source stream",
                    )?;
                    let sha = source[name][stream]["sha256"]
                        .as_str()
                        .ok_or("native source stream identity is unavailable")?;
                    if !items.iter().any(|item| {
                        item[stream]["sha256"].as_str() == Some(sha)
                            && item[stream]["bytes"] == source[name][stream]["bytes"]
                    }) {
                        return Err("native source stream original is unavailable".into());
                    }
                }
            }
        }
        if source["artifact"].is_object() {
            classified(
                &source["artifact"],
                &["kind", "sha256", "bytes", "exact"],
                "native source artifact",
            )?;
            let sha = source["artifact"]["sha256"]
                .as_str()
                .ok_or("native source identity is unavailable")?;
            if source["artifact"]["kind"] != "evidence" || source["artifact"]["exact"] != true {
                return Err("native source artifact is unclassified".into());
            }
            if !items.iter().any(|item| {
                item["artifact"]["sha256"].as_str() == Some(sha)
                    && item["artifact"]["bytes"] == source["artifact"]["bytes"]
            }) {
                return Err("native source original is unavailable".into());
            }
        }
        let material_fields = [
            "status",
            "freshness",
            "strength",
            "requirementId",
            "requirementText",
            "checker",
            "origin",
            "acquisition",
            "logStatus",
            "result",
            "checkEventRef",
            "checkLogRef",
        ];
        if source["kind"] == "submission" {
            if !items.iter().any(|item| {
                item["eventSha256"] == source["evidenceId"]
                    && item["reference"] == source["rawEventRef"]["id"]
                    && item["artifact"]["sha256"] == source["artifact"]["sha256"]
                    && item["artifact"]["bytes"] == source["artifact"]["bytes"]
            }) {
                return Err("native submission source identity is unavailable".into());
            }
            if material_fields.iter().all(|name| !nonempty(&source[*name])) {
                continue;
            }
        }
        let mut status = Map::new();
        for name in [
            "kind",
            "status",
            "freshness",
            "strength",
            "requirementId",
            "requirementText",
            "checker",
            "origin",
            "acquisition",
            "logStatus",
            "result",
        ] {
            copy_if_present(&mut status, name, &source[name]);
        }
        for name in ["rawEventRef", "checkEventRef", "checkLogRef"] {
            if let Some(reference) = source[name]["id"].as_str() {
                status.insert(name.into(), json!(reference));
            }
        }
        statuses.push(Value::Object(status));
    }
    if !statuses.is_empty() {
        actor.insert("sourceStatuses".into(), json!(statuses));
    }
    if let Some(routes) = context["expansions"].as_array() {
        for route in routes {
            classified(
                route,
                &[
                    "kind",
                    "work",
                    "sha256",
                    "headEventSha256",
                    "eventCount",
                    "exact",
                    "id",
                ],
                "native history route",
            )?;
            if route["kind"] != "ledger_history" || route["work"] != work || route["exact"] != true
            {
                return Err("native history metadata is unclassified".into());
            }
        }
    } else if nonempty(&context["expansions"]) {
        return Err("native history metadata is unavailable".into());
    }
    Ok(())
}

pub(super) fn actor_evidence(evidence: &Value) -> Result<Value, String> {
    classified(
        evidence,
        &[
            "complete",
            "historical",
            "items",
            "checkEvidence",
            "earlyEvidence",
            "preReturn",
        ],
        "native evidence",
    )?;
    if evidence["complete"] != true || !evidence["historical"].is_boolean() {
        return Err("complete current actor evidence is unavailable".into());
    }
    let items = evidence["items"]
        .as_array()
        .ok_or("native evidence items are unavailable")?;
    let mut selected = Vec::new();
    for item in items {
        classified(
            item,
            &[
                "reference",
                "kind",
                "eventSha256",
                "historical",
                "artifact",
                "stdout",
                "stderr",
                "result",
                "outcome",
            ],
            "native evidence item",
        )?;
        if !item["historical"].is_boolean() || !item["kind"].is_string() {
            return Err("native evidence item status is unavailable".into());
        }
        let mut output = Map::new();
        for name in ["reference", "kind", "outcome", "result"] {
            if !item[name].is_null() {
                output.insert(name.into(), item[name].clone());
            }
        }
        if item["historical"] == true {
            output.insert("historical".into(), json!(true));
        }
        for name in ["artifact", "stdout", "stderr"] {
            if item[name].is_null() {
                continue;
            }
            let material = &item[name];
            classified(
                material,
                &[
                    "sha256", "bytes", "verified", "complete", "exact", "readable", "encoding",
                    "content", "reason", "access",
                ],
                "native evidence material",
            )?;
            let content = material["content"]
                .as_str()
                .ok_or("native evidence bytes are unavailable")?;
            let content_sha256 = hash::text(content);
            if material["verified"] != true
                || material["complete"] != true
                || material["exact"] != true
                || material["readable"] != true
                || material["encoding"] != "utf-8"
                || material["bytes"].as_u64() != Some(content.len() as u64)
                || material["sha256"].as_str() != Some(content_sha256.as_str())
            {
                return Err("native evidence is not verified complete exact UTF-8".into());
            }
            output.insert(name.into(), json!(content));
            let mut status = Map::new();
            copy_if_present(&mut status, "reason", &material["reason"]);
            copy_if_present(&mut status, "access", &material["access"]);
            if !status.is_empty() {
                output.insert(format!("{name}Status"), Value::Object(status));
            }
        }
        selected.push(Value::Object(output));
    }
    let mut result = json!({"complete":true,"historical":evidence["historical"],
        "verification":"included material is verified, complete, exact UTF-8","items":selected});
    copy_if_present(
        result.as_object_mut().unwrap(),
        "checkEvidence",
        &evidence["checkEvidence"],
    );
    if evidence["earlyEvidence"]
        != json!({"authority":"none","selected":false,"state":"not_selected"})
    {
        copy_if_present(
            result.as_object_mut().unwrap(),
            "earlyEvidence",
            &evidence["earlyEvidence"],
        );
    }
    if evidence["preReturn"] != json!({"attempt":null,"enabled":false,"policy":null}) {
        copy_if_present(
            result.as_object_mut().unwrap(),
            "preReturn",
            &evidence["preReturn"],
        );
    }
    Ok(result)
}

pub(super) fn upstream_reasons(
    actor: &mut Map<String, Value>,
    upstream: &Value,
    evidence: &Value,
) -> Result<(), String> {
    let Some(items) = upstream.as_array() else {
        return if upstream.is_null() {
            Ok(())
        } else {
            Err("native upstream sources are unavailable".into())
        };
    };
    let mut reasons = Vec::new();
    for item in items {
        classified(
            item,
            &[
                "agent",
                "attempt",
                "attemptStatus",
                "reason",
                "role",
                "sha256",
                "stage",
            ],
            "native upstream source",
        )?;
        let sha256 = item["sha256"]
            .as_str()
            .ok_or("native upstream source identity is unavailable")?;
        let sources = evidence["items"]
            .as_array()
            .ok_or("native evidence items are unavailable")?
            .iter()
            .filter(|candidate| candidate["artifact"]["sha256"].as_str() == Some(sha256))
            .collect::<Vec<_>>();
        let [source] = sources[..] else {
            return Err("native upstream original is unavailable or ambiguous".into());
        };
        let mut reason = json!({"role":item["role"],"agent":item["agent"],"stage":item["stage"],"attempt":item["attempt"],"attemptStatus":item["attemptStatus"],"reason":item["reason"]});
        if source["reference"].is_string() {
            reason["reference"] = source["reference"].clone();
        }
        reasons.push(reason);
    }
    if !reasons.is_empty() {
        actor.insert("upstream".into(), json!(reasons));
    }
    Ok(())
}

//! Verify that a native child profile belongs to the current pending assignment.
//! A focus pointer and a matching agent name alone are never acquisition proof.

use crate::{config::Loaded, work::focus::Focus};

#[derive(Clone, Debug)]
pub(crate) struct Provenance {
    pub(crate) project: String,
    pub(crate) work: String,
    pub(crate) assignment: String,
    pub(crate) role: String,
    pub(crate) agent: String,
    pub(crate) native: String,
    pub(crate) profile_sha256: String,
    pub(crate) packet_digest: String,
    pub(crate) rules_sha256: String,
    pub(crate) root_scope: String,
}

pub(crate) enum Selection {
    Unbound,
    Bound {
        work: String,
        assignment: String,
        packet_digest: String,
        provenance: Box<Provenance>,
        /// Read routes for the evidence this assignment's packet covers.
        evidence: Vec<String>,
        architecture: Option<serde_json::Value>,
    },
    Mismatch(String),
    Unavailable(String),
}

pub(crate) fn project_identity(loaded: &Loaded) -> Result<String, String> {
    let root = loaded
        .product_root
        .to_str()
        .ok_or("project root is not valid UTF-8")?;
    let root_sha256 = crate::evidence::hash::text(root);
    Ok(loaded.project_id.as_ref().map_or_else(
        || format!("root:{root_sha256}"),
        |id| format!("id:{id};root:{root_sha256}"),
    ))
}

pub(crate) fn selection(loaded: &Loaded, agent_id: &str, profile_sha: &str) -> Selection {
    let work = match crate::work::focus::read(loaded) {
        Ok(Focus::Absent) => return Selection::Unbound,
        Ok(Focus::Work(work)) => work,
        Ok(Focus::Unusable(reason)) => return Selection::Unavailable(reason),
        Err(reason) => return Selection::Unavailable(reason),
    };
    let Ok(view) = crate::work::next(loaded, &work) else {
        return Selection::Unavailable("current action is not a pending spawn".into());
    };
    let next = &view["next"];
    if next["action"] != "spawn" {
        return Selection::Unavailable("assignment packet has no context digest".into());
    }
    let Some(agent) = loaded.agent(agent_id) else {
        return Selection::Mismatch(format!(
            "configured agent '{agent_id}' is not present in the current project"
        ));
    };
    let expected_native = agent.native_name(agent_id);
    let expected = (
        next["packet"]["agent"].as_str().unwrap_or(""),
        next["packet"]["nativeTaskName"].as_str().unwrap_or(""),
        next["packet"]["profileSha256"].as_str().unwrap_or(""),
    );
    if expected != (agent_id, expected_native.as_str(), profile_sha) {
        return Selection::Mismatch(format!(
            "pending assignment differs (agent={}, nativeTaskName={}, profileSha256={})",
            expected.0, expected.1, expected.2
        ));
    }
    let (Some(assignment), Some(packet_digest)) = (
        next["assignment"].as_str(),
        next["packet"]["context"]["digest"].as_str(),
    ) else {
        return Selection::Unavailable("assignment packet is missing identity".into());
    };
    let context = &next["packet"]["context"];
    let project = match project_identity(loaded) {
        Ok(project) => project,
        Err(reason) => return Selection::Unavailable(reason),
    };
    let rules_sha256 =
        crate::evidence::hash::value(&crate::project::context::snapshot(loaded)["rules"]);
    let provenance = Provenance {
        project,
        work: work.clone(),
        assignment: assignment.to_owned(),
        role: next["packet"]["role"]
            .as_str()
            .unwrap_or("unknown")
            .to_owned(),
        agent: agent_id.to_owned(),
        native: expected_native,
        profile_sha256: profile_sha.to_owned(),
        packet_digest: packet_digest.to_owned(),
        rules_sha256,
        root_scope: context["run"]["id"]
            .as_str()
            .or_else(|| context["scope"]["planSha256"].as_str())
            .unwrap_or("unavailable")
            .to_owned(),
    };
    let evidence = super::assignment_evidence::lines(loaded, &work, context);
    let architecture = match crate::project::architecture::delivery(loaded, &next["packet"]) {
        Ok(slice) => slice,
        Err(reason) => return Selection::Unavailable(reason),
    };
    Selection::Bound {
        work,
        assignment: assignment.to_owned(),
        packet_digest: packet_digest.to_owned(),
        provenance: Box::new(provenance),
        evidence,
        architecture,
    }
}

pub(crate) fn verify_away_context(
    loaded: &Loaded,
    packet: &serde_json::Value,
    assignment: &serde_json::Value,
) -> Result<(), String> {
    if !crate::producer::exitbind_surface() {
        return Ok(());
    }
    if let Some(packet_work) = packet["work"].as_str() {
        verify_work_scope(packet_work, &crate::work::focus::read(loaded)?)?;
    }
    let expected = crate::evidence::hash::text(&loaded.source);
    let current_project = project_identity(loaded)?;
    if packet["projectIdentity"].as_str() != Some(current_project.as_str()) {
        return Err(format!(
            "assignment context mismatch: packet project identity is {}, current project is {}",
            packet["projectIdentity"].as_str().unwrap_or("absent"),
            current_project
        ));
    }
    if packet["configSha256"].as_str() != Some(expected.as_str()) {
        return Err(format!(
            "assignment context mismatch: current project configuration is {expected}, packet is {}",
            packet["configSha256"].as_str().unwrap_or("absent")
        ));
    }
    if assignment["profileSha256"].as_str().is_none()
        || assignment["nativeTaskName"].as_str().is_none()
        || assignment["role"].as_str().is_none()
    {
        return Err("assignment context mismatch: packet provenance is incomplete".into());
    }
    let canonical = packet["assignments"]
        .as_array()
        .and_then(|items| {
            items
                .iter()
                .find(|item| item["agent"] == assignment["agent"])
        })
        .ok_or("assignment context mismatch: selected assignment is not in the current packet")?;
    for field in [
        "stage",
        "attempt",
        "agent",
        "nativeTaskName",
        "profileSha256",
        "role",
    ] {
        if canonical[field] != assignment[field] {
            return Err(format!(
                "assignment context mismatch: {field} differs from the current packet"
            ));
        }
    }
    Ok(())
}

fn verify_work_scope(packet_work: &str, focus: &Focus) -> Result<(), String> {
    match focus {
        Focus::Absent => Ok(()),
        Focus::Work(current) if current == packet_work => Ok(()),
        Focus::Work(current) => Err(format!(
            "assignment context mismatch: packet work {packet_work} differs from focused work {current}"
        )),
        Focus::Unusable(reason) => Err(format!(
            "assignment context mismatch: current work scope is unavailable ({reason})"
        )),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use std::collections::BTreeMap;
    use std::path::PathBuf;

    fn loaded(source: &str) -> Loaded {
        Loaded {
            config: json!({}),
            agents: BTreeMap::new(),
            lead: None,
            architecture_contract: None,
            path: PathBuf::from("config.json"),
            control_root: PathBuf::from("fixture-root"),
            product_root: PathBuf::from("fixture-root"),
            state_root: PathBuf::from("fixture-root"),
            mode: crate::project::layout_types::Mode::Portable,
            project_id: None,
            source: source.to_owned(),
        }
    }

    #[test]
    fn away_context_rejects_a_different_project_configuration() {
        let loaded = loaded("current");
        let packet = json!({
            "projectIdentity": project_identity(&loaded).unwrap(),
            "configSha256": crate::evidence::hash::text("other")
        });
        let assignment = json!({
            "profileSha256": "a",
            "nativeTaskName": "worker",
            "role": "worker"
        });
        if !crate::producer::exitbind_surface() {
            assert!(verify_away_context(&loaded, &packet, &assignment).is_ok());
            return;
        }
        let error = verify_away_context(&loaded, &packet, &assignment).unwrap_err();
        assert!(error.contains("assignment context mismatch"));
        assert!(error.contains("current project configuration"));
    }

    #[test]
    fn away_context_rejects_an_incomplete_assignment_provenance() {
        let loaded = loaded("current");
        let packet = json!({
            "projectIdentity": project_identity(&loaded).unwrap(),
            "configSha256": crate::evidence::hash::text("current")
        });
        if !crate::producer::exitbind_surface() {
            assert!(verify_away_context(&loaded, &packet, &json!({})).is_ok());
            return;
        }
        let error = verify_away_context(&loaded, &packet, &json!({})).unwrap_err();
        assert_eq!(
            error,
            "assignment context mismatch: packet provenance is incomplete"
        );
    }

    #[test]
    fn same_project_different_work_and_stale_work_are_refused() {
        let error = verify_work_scope("smw_current", &Focus::Work("smw_other".into())).unwrap_err();
        assert!(error.contains("packet work smw_current differs from focused work smw_other"));
        let stale = verify_work_scope("smw_old", &Focus::Work("smw_current".into())).unwrap_err();
        assert!(stale.contains("packet work smw_old differs from focused work smw_current"));
    }

    #[test]
    fn global_agreements_do_not_replace_a_project_scope() {
        assert!(verify_work_scope("smw_current", &Focus::Absent).is_ok());
        let error =
            verify_work_scope("smw_current", &Focus::Unusable("stale pointer".into())).unwrap_err();
        assert!(error.contains("current work scope is unavailable"));
    }

    #[test]
    fn different_project_identity_and_role_mismatch_are_refused() {
        let loaded = loaded("current");
        let packet = json!({
            "projectIdentity": "id:other;root:other",
            "configSha256": crate::evidence::hash::text("current"),
            "assignments": [{"agent":"reviewer","stage":1,"attempt":1,
                "nativeTaskName":"reviewer","profileSha256":"review-profile","role":"reviewer"}]
        });
        let assignment = json!({"agent":"reviewer","stage":1,"attempt":1,
            "nativeTaskName":"worker","profileSha256":"review-profile","role":"worker"});
        let same_project = json!({
            "projectIdentity": project_identity(&loaded).unwrap(),
            "configSha256": crate::evidence::hash::text("current"),
            "assignments": [{"agent":"reviewer","stage":1,"attempt":1,
                "nativeTaskName":"reviewer","profileSha256":"review-profile","role":"reviewer"}]
        });
        if !crate::producer::exitbind_surface() {
            assert!(verify_away_context(&loaded, &packet, &assignment).is_ok());
            assert!(verify_away_context(&loaded, &same_project, &assignment).is_ok());
            return;
        }
        let error = verify_away_context(&loaded, &packet, &assignment).unwrap_err();
        assert!(error.contains("packet project identity"));

        let error = verify_away_context(&loaded, &same_project, &assignment).unwrap_err();
        assert!(error.contains("nativeTaskName differs"));
    }

    #[cfg(unix)]
    #[test]
    fn non_utf8_project_root_fails_closed() {
        use std::ffi::OsString;
        use std::os::unix::ffi::OsStringExt;
        let mut loaded = loaded("current");
        loaded.product_root = PathBuf::from(OsString::from_vec(b"project-\xff".to_vec()));
        assert_eq!(
            project_identity(&loaded).unwrap_err(),
            "project root is not valid UTF-8"
        );
    }
}

//! Verify that a native child profile belongs to the current pending assignment.
//! A focus pointer and a matching agent name alone are never acquisition proof.

use crate::{config::Loaded, work::focus::Focus};

pub(crate) enum Selection {
    Unbound,
    Bound {
        work: String,
        assignment: String,
        packet_digest: String,
    },
    Mismatch,
    Unavailable,
}

pub(crate) fn selection(loaded: &Loaded, agent_id: &str, profile_sha: &str) -> Selection {
    let work = match crate::work::focus::read(loaded) {
        Ok(Focus::Absent) => return Selection::Unbound,
        Ok(Focus::Work(work)) => work,
        Ok(Focus::Unusable(_)) | Err(_) => return Selection::Unavailable,
    };
    let Ok(view) = crate::work::next(loaded, &work) else {
        return Selection::Unavailable;
    };
    let next = &view["next"];
    if next["action"] != "spawn" {
        return Selection::Unavailable;
    }
    let Some(agent) = loaded.agent(agent_id) else {
        return Selection::Mismatch;
    };
    let matches = next["packet"]["agent"] == agent_id
        && next["packet"]["nativeTaskName"] == agent.native_name(agent_id)
        && next["packet"]["profileSha256"] == profile_sha;
    if !matches {
        return Selection::Mismatch;
    }
    let (Some(assignment), Some(packet_digest)) = (
        next["assignment"].as_str(),
        next["packet"]["context"]["digest"].as_str(),
    ) else {
        return Selection::Unavailable;
    };
    Selection::Bound {
        work,
        assignment: assignment.to_owned(),
        packet_digest: packet_digest.to_owned(),
    }
}

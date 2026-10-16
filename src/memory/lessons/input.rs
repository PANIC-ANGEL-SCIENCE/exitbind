//! One owner for the lesson parser, read-only preparation and displayed bounds.
use super::*;

const ID_BYTES: usize = 64;
const FACT_BYTES: usize = 512;
const PROVENANCE_ITEMS: usize = 4;
const PROVENANCE_BYTES: usize = 160;
const AGENT_ITEMS: usize = 8;
const TERM_ITEMS: usize = 4;
const TERM_BYTES: usize = 64;
const GUARD_ITEMS: usize = 4;
const PATH_BYTES: usize = 256;

pub(crate) fn input_form(loaded: &Loaded) -> Result<Value, String> {
    Ok(json!({"version":1,"scope":SCOPE,"effect":"no-change",
        "bounds":{"sourceMaximumBytes":MAX_ENTRY,"factMaximumBytes":FACT_BYTES,"unit":"UTF-8 bytes"},
        "fieldBounds":{
            "version":{"value":1},
            "id":{"minimumBytes":1,"maximumBytes":ID_BYTES,"characters":"ASCII alphanumeric, dot, underscore or hyphen"},
            "fact":{"minimumBytes":1,"maximumBytes":FACT_BYTES},
            "projectIdentity":{"value":crate::host::assignment_context::project_identity(loaded)?},
            "owner":{"value":loaded.lead()},
            "provenance":{"minimumItems":1,"maximumItems":PROVENANCE_ITEMS,"maximumItemBytes":PROVENANCE_BYTES},
            "createdRevision":{"lengths":[40,64],"characters":"lowercase hexadecimal"},
            "revalidatedRevision":{"lengths":[40,64],"characters":"lowercase hexadecimal"},
            "appliesTo.agents":{"minimumItems":1,"maximumItems":AGENT_ITEMS,"unique":true,"values":loaded.agents.keys().collect::<Vec<_>>()},
            "appliesTo.taskTerms":{"minimumItems":0,"maximumItems":TERM_ITEMS,"maximumItemBytes":TERM_BYTES},
            "guards":{"type":"array","minimumItems":1,"maximumItems":GUARD_ITEMS,
                "element":{"path":{"minimumBytes":1,"maximumBytes":PATH_BYTES,"shape":"confined relative project path containing only normal components"},
                    "sha256":{"bytes":64,"characters":"lowercase hexadecimal"}}},
            "supersedes":{"optional":true,"bytes":64,"characters":"lowercase hexadecimal"}},
        "textRules":"Nonblank strings without control characters; lengths count UTF-8 bytes. Unknown object fields are refused. The complete source has its own byte bound.",
        "template":{"version":1,"id":"stable-lesson-id","fact":"One bounded reviewed project fact.",
            "projectIdentity":crate::host::assignment_context::project_identity(loaded)?,
            "owner":loaded.lead(),"provenance":["Reviewed source and reason"],
            "createdRevision":"<40-or-64-lowercase-hex-original-revision>",
            "revalidatedRevision":"<40-or-64-lowercase-hex-current-reviewed-revision>",
            "appliesTo":{"agents":["<configured-recipient>"],"taskTerms":["<relevant-term>"]},
            "guards":[{"path":"<relative-project-file>","sha256":"<64-lowercase-hex-digest>"}]},
        "correction":{"stableFields":["id","createdRevision"],"reviewedRevision":"revalidatedRevision",
            "supersedes":"exact retired predecessor itemId; propose a new immutable source/ledger; review it; retire predecessor before promotion"},
        "authority":"the template grants no memory rights; existing scoped rights, guards and explicit expires-at still apply"}))
}

fn require(field: &str, valid: bool, rule: &str) -> Result<(), String> {
    if valid {
        Ok(())
    } else {
        Err(format!("project lesson {field}: {rule}"))
    }
}

fn texts(
    field: &str,
    values: &[String],
    min: usize,
    max: usize,
    bytes: usize,
) -> Result<(), String> {
    require(
        field,
        (min..=max).contains(&values.len()),
        &format!("requires {min}..{max} items"),
    )?;
    for (index, value) in values.iter().enumerate() {
        require(
            &format!("{field}[{index}]"),
            text(value, bytes),
            &format!(
                "must be nonblank, without control characters, and at most {bytes} UTF-8 bytes"
            ),
        )?;
    }
    Ok(())
}

pub(crate) fn parse(loaded: &Loaded, bytes: &[u8]) -> Result<Lesson, String> {
    require(
        "source",
        bytes.len() <= MAX_ENTRY,
        &format!("exceeds the {MAX_ENTRY}-byte entry bound (UTF-8 bytes)"),
    )?;
    let lesson: Lesson = serde_json::from_slice(bytes)
        .map_err(|error| format!("invalid project lesson: {error}"))?;
    require("version", lesson.version == 1, "must equal 1")?;
    require(
        "id",
        text(&lesson.id, ID_BYTES)
            && lesson
                .id
                .bytes()
                .all(|c| c.is_ascii_alphanumeric() || b"._-".contains(&c)),
        &format!("requires 1..{ID_BYTES} ASCII alphanumeric, dot, underscore or hyphen bytes"),
    )?;
    require(
        "fact",
        text(&lesson.fact, FACT_BYTES),
        &format!(
            "must be nonblank, without control characters, and at most {FACT_BYTES} UTF-8 bytes"
        ),
    )?;
    require(
        "projectIdentity",
        lesson.project_identity == crate::host::assignment_context::project_identity(loaded)?,
        "must equal the current project identity",
    )?;
    require(
        "owner",
        Some(lesson.owner.as_str()) == loaded.lead(),
        "must equal the configured Lead",
    )?;
    texts(
        "provenance",
        &lesson.provenance,
        1,
        PROVENANCE_ITEMS,
        PROVENANCE_BYTES,
    )?;
    for (field, value) in [
        ("createdRevision", &lesson.created_revision),
        ("revalidatedRevision", &lesson.revalidated_revision),
    ] {
        require(field, revision(value), "requires 40 or 64 lowercase hexadecimal characters; corrections retain the original createdRevision")?;
    }
    let applies = &lesson.applies_to;
    require(
        "appliesTo.agents",
        (1..=AGENT_ITEMS).contains(&applies.agents.len()),
        &format!("requires 1..{AGENT_ITEMS} configured agents"),
    )?;
    for (index, name) in applies.agents.iter().enumerate() {
        require(
            &format!("appliesTo.agents[{index}]"),
            loaded.agent(name).is_some(),
            "must name a configured agent",
        )?;
    }
    require(
        "appliesTo.agents",
        applies.agents.iter().collect::<BTreeSet<_>>().len() == applies.agents.len(),
        "must contain unique configured agents",
    )?;
    texts(
        "appliesTo.taskTerms",
        &applies.task_terms,
        0,
        TERM_ITEMS,
        TERM_BYTES,
    )?;
    require(
        "guards",
        (1..=GUARD_ITEMS).contains(&lesson.guards.len()),
        &format!("requires 1..{GUARD_ITEMS} path/sha256 objects"),
    )?;
    for (index, guard) in lesson.guards.iter().enumerate() {
        require(
            &format!("guards[{index}].sha256"),
            digest(&guard.sha256),
            "requires 64 lowercase hexadecimal characters",
        )?;
        require(&format!("guards[{index}].path"), text(&guard.path, PATH_BYTES)
            && !std::path::Path::new(&guard.path).is_absolute()
            && std::path::Path::new(&guard.path).components().all(|part| matches!(part, std::path::Component::Normal(_))),
            &format!("requires a confined relative project path, without control characters, at most {PATH_BYTES} UTF-8 bytes"))?;
    }
    require(
        "supersedes",
        lesson.supersedes.as_deref().map_or(true, digest),
        "requires the exact predecessor itemId: 64 lowercase hexadecimal characters",
    )?;
    Ok(lesson)
}

/// Exact inactive-source preparation; no lifecycle event, guard refresh or qualification.
fn source_bytes(loaded: &Loaded, source: &str) -> Result<Vec<u8>, String> {
    match path::secure_bytes_observation_bounded(
        &loaded.product_root,
        source,
        "lesson source",
        MAX_ENTRY as u64,
    ) {
        path::SecureBytesResult::Bytes(bytes) => Ok(bytes),
        path::SecureBytesResult::Absent(error)
        | path::SecureBytesResult::Unsafe(error)
        | path::SecureBytesResult::Unreadable(error) => Err(error),
        #[cfg(not(unix))]
        path::SecureBytesResult::Unsupported(error) => Err(error),
    }
}

pub(crate) fn validate_source(loaded: &Loaded, source: &str) -> Result<Value, String> {
    let bytes = source_bytes(loaded, source)?;
    let lesson = parse(loaded, &bytes)?;
    super::super::ensure_config_current(loaded)?;
    let lead = loaded
        .lead()
        .ok_or("lesson preparation requires a configured Lead")?;
    let profile = &loaded
        .agent(lead)
        .ok_or("configured Lead unavailable")?
        .profile;
    let profile_bytes = path::secure_bytes(&loaded.control_root, profile, "lesson owner profile")?;
    if source_bytes(loaded, source)? != bytes {
        return Err("project lesson source changed during preparation".into());
    }
    if path::secure_bytes(&loaded.control_root, profile, "lesson owner profile")? != profile_bytes {
        return Err("project lesson owner profile changed during preparation".into());
    }
    super::super::ensure_config_current(loaded)?;
    Ok(json!({"valid":true,"effect":"no-change","typedSource":true,
        "source":{"path":source,"sha256":hash::bytes(&bytes),"bytes":bytes.len()},
        "bindings":{"owner":lead,"projectIdentity":lesson.project_identity,"configurationSha256":hash::text(&loaded.source),"profileSha256":hash::bytes(&profile_bytes)},
        "guardCount":lesson.guards.len(),"guardCurrentness":"checked at lifecycle and delivery; not refreshed by preparation",
        "qualification":"not_evaluated","lifecycle":"unchanged"}))
}

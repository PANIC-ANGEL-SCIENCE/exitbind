//! A journey member's configuration stamp across a configuration change.
//!
//! A member is stamped with the configuration bytes current when it was
//! recorded, and is never restamped. When the bytes now at its configuration
//! path differ, the member is admitted only on proof by the stamped bytes
//! themselves: the copy kept under the stamp's hash (`memory::stamped_config`)
//! whose lesson-authority projection (`memory::lesson_authority`, with
//! `memoryRead` kept) equals the current one. Its evidence is then read
//! under those stamped bytes. Every member consumer (goal status, goal
//! prepare, work begin --journey-integration, closure) reaches this one rule
//! through `integration::project_loaded`.
use super::*;

/// The configuration a member is read under: the current bytes when they
/// are its stamp, else the kept stamped bytes at the same path when the
/// change moves no authority or read right. `Ok(None)` when no copy of the
/// stamped bytes is kept; any other difference refuses with today's text.
pub(super) fn admitted(current: Loaded, member: &Member) -> Result<Option<Loaded>, String> {
    if member.config_sha256 == hash::text(&current.source) {
        return Ok(Some(current));
    }
    let Some((_, stamped)) = crate::memory::stamped_config::read(&current, &member.config_sha256)
    else {
        return Ok(None);
    };
    let changed = || "journey member belongs to another or changed project/session configuration";
    let previous =
        crate::config::from_source(current.path.clone(), stamped, "stamped configuration copy")
            .map_err(|_| changed())?;
    let authority = |source: &str| {
        crate::memory::lesson_authority::projection(source, false).map_err(|_| changed())
    };
    if authority(&previous.source)? != authority(&current.source)? {
        return Err(changed().into());
    }
    Ok(Some(previous))
}

/// The configuration a member's accepted evidence is read under: the
/// selected one, except where the selected configuration is the member's
/// own file and its bytes are no longer the stamp; the member was then
/// admitted by its kept stamped bytes (`admitted`), and those are read.
pub(super) fn evidence_under<'a>(
    selected: &'a Loaded,
    admitted: &'a Loaded,
    member: &Member,
) -> &'a Loaded {
    if selected.path == admitted.path && hash::text(&selected.source) != member.config_sha256 {
        admitted
    } else {
        selected
    }
}

/// The typed refusal for `works`, stamped with `bound` whose bytes are not
/// kept: one `memory revalidate` route whose previous-configuration slot
/// the owner fills. Run with the owner's copy of those bytes, it keeps them
/// under their hash (and revalidates every lesson ledger stamped with
/// them), which admits every member with that stamp when the change moves
/// no authority.
pub(super) fn unproven(loaded: &Loaded, bound: &str, works: &[&str]) -> String {
    let ledgers = crate::memory::stamped_config::stamped_lesson_ledgers(loaded, bound);
    let refusal = crate::drift::refusal(
        crate::drift::Class::JourneyMemberConfiguration,
        bound,
        Some(&hash::text(&loaded.source)),
        None,
        crate::drift::Route::Revalidate {
            lead: loaded.lead().unwrap_or("lead"),
            ledgers: &ledgers,
            kept_copy: None,
            config: &loaded.path,
        },
    );
    format!("journey members {}: {refusal}", works.join(", "))
}

/// Whether a journey member of this project's owner Goal in the current
/// native session carries `sha256` as its configuration stamp. Read-only;
/// an unavailable session, owner or record reads as no.
pub(crate) fn stamps(loaded: &Loaded, sha256: &str) -> bool {
    let carried = || -> Result<bool, String> {
        let scope = super::super::native::scope(loaded, false)?;
        if scope.session.is_none() || !scope.owner_matches {
            return Ok(false);
        }
        let Some(record) = super::super::read(&scope.root)? else {
            return Ok(false);
        };
        let Some(journey) = state(&record)? else {
            return Ok(false);
        };
        let project = super::super::requirements::identity(loaded)?;
        Ok(journey
            .members
            .iter()
            .chain(journey.integrations.iter().map(|entry| &entry.member))
            .any(|member| {
                member.project_identity_sha256 == project && member.config_sha256 == sha256
            }))
    };
    carried().unwrap_or(false)
}

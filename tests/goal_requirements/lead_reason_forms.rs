//! Shared classification of command forms taking a reason.

/// Commands whose option list takes `--reason` and whose act is the Lead's
/// `submit` on a Work: each must record the text.
pub(super) const LEAD_ACTS: [&str; 4] =
    ["work return", "work act", "work disposition", "run submit"];
/// Commands that take `--reason` but write no `submit` event: not a Lead act
/// on a Work, each keeps its text in its own record.
pub(super) const NOT_LEAD_ACTS: [&str; 15] = [
    "goal correct-owner",
    "goal quarantine",
    "goal recover",
    "memory choose",
    "memory correct-policy",
    "memory revalidate",
    "project advance amend",
    "project advance select",
    "project advance stop",
    "project architecture select",
    "project capabilities select",
    "run review-policy",
    "work quarantine",
    "work quarantine-followup",
    "work replace-check",
];

/// Native successor creation is not a Lead submission. It commits to the
/// reason in the successor identity instead of storing its text; the caller
/// must retain the exact reason for admission recovery.
pub(super) const COMMITTED_REASON_FORMS: [&str; 1] = ["work supersede"];

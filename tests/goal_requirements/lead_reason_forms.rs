//! Shared classification of command forms taking a reason.

/// Commands whose option list takes `--reason` and whose act is the Lead's
/// `submit` on a Work: each must record the text.
pub(super) const LEAD_ACTS: [&str; 4] =
    ["work return", "work act", "work disposition", "run submit"];
/// Commands that take `--reason` but write no `submit` event: not a Lead act
/// on a Work, each keeps its text in its own record.
pub(super) const NOT_LEAD_ACTS: [&str; 14] = [
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

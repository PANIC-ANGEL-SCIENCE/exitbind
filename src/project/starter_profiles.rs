// Supplies neutral starter role-profile assets.
// The bundled set contains three roles; see `src/project/mod.rs`.
//! Neutral starter responsibilities, also distributed as managed role assets.

pub(crate) const PROFILES: [(&str, &str); 3] = [
    (
        "lead",
        include_str!("../../skills/exitbind/references/roles/lead.md"),
    ),
    (
        "worker",
        include_str!("../../skills/exitbind/references/roles/worker.md"),
    ),
    (
        "reviewer",
        include_str!("../../skills/exitbind/references/roles/reviewer.md"),
    ),
];

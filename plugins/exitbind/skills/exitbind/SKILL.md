---
name: exitbind
description: This project is governed by Exitbind: use it for material coding work here - multi-file changes, refactors, migrations, bug fixes that need a check, compatibility-sensitive edits, resumed or interrupted work, and changes that need review or acceptance. Bind each check, review, and acceptance to the exact current result so stale evidence cannot be reused. Keep read-only questions and tiny obvious reversible edits direct.
---

<!-- exitbind-managed-skill:v1 -->

This project is configured for Exitbind. Select it before consequential
repository mutation when the task is material: several files, a refactor or
migration, a bug fix that needs a check, behavior that must be preserved, work
resumed from an earlier session, or a change that should be reviewed before it
is trusted. Read-only questions and tiny obvious reversible edits stay direct,
with no extra reviewers, checks, or ceremony.

Continue existing work with `exitbind work resume`; start new governed work
with `exitbind work begin`. Follow the returned next action instead of asking
the user for work handles, ledger paths, or event hashes. Report exact
progress, evidence, and any refusal. If the user gives only the Exitbind
repository URL, inspect this project and your host capabilities first, and ask
before installation, project writes, permission changes, or another
owner-controlled action.

Exitbind owns the checked-run lifecycle and exact-subject exit semantics. The
host owns models, tools, process execution, permissions, and merge authority.
Never overwrite the host agent's native identity or claim that projected
guidance proves discovery, activation, compliance, or isolation.

For a material task, the lead first classifies the goal, dependencies, useful
context, tools, roles, role boundary, and actual check command. Keep tiny,
clear, reversible work direct. If the task is complex but has no material
semantic-preservation risk, the lead performs only that readiness and keeps
Holytail off. Tool and agent selection remains the lead's responsibility; this
skill does not require a preparation artifact or a standalone Holytail install.
When the governed path is selected, inspect the bridge end to end: source,
projected skill/config, fresh-session discovery, invocation, observed behavior,
and outcome. Run the normal Exitbind lifecycle and keep the ledger/Exit Path as
authority; process survival or an agent's report is not acceptance.

For material work, the routine progress narration is exactly:

[Neuro] Exitbind progress: N%.

Exitbind computes that progress and owns the terminal state; never estimate the
number and never show one when no governed run applies. `work next` and
`work resume` return a `presentation` block: render `neuro` as written, and when
`phrase` is present add that one short line in the user's language. Say nothing
extra when it is absent - repeated reads of an unchanged state stay quiet. When
the run reaches `EXIT READY`, say exactly that and add no flavour line.

Do not add a routine Holytail progress report. When the presentation block
carries a `holytail` line, place it directly above the Neuro line with no blank
line between them; otherwise omit it.

Keep observed, reported, inferred, and proposed facts distinct. Preserve
reported-versus-observed check provenance. Bind every check, review, and lead
decision to the current subject; stale or partial evidence must remain refused
or blocked. Curiosity is non-blocking and cannot expand scope or become proof.

For preservation-risk work, activate Holytail selectively. Route and quality are
separate axes: the route is `INLINE` or `FORMAL`, the assigned quality is `FULL`,
`ECO`, or `MODE-UNBOUND` when no assignment exists, and neither is ever inferred
from the other, from a model name, or from a host effort label. Every `FORMAL`
task is assigned `FULL`. Read
[references/preservation.md](references/preservation.md) before running the
formal path; it ships with Exitbind, so no separate preservation install,
checkout, or second instruction source is needed. Its narrow path is:

1. Clarify the accepted behavior, invariants, allowed changes, ambiguity,
   preservation risks, and evidence needed to demonstrate continuity.
2. Freeze that accepted meaning and bind the obligations to the current
   implementation subject before implementation.
3. Preserve the frozen meaning during implementation; a minimizer may reduce
   mechanism, but it cannot decide whether Holytail is needed or replace the
   preservation check.
4. Read back the same accepted meaning against the exact current subject after
   implementation and observed checks.

Trivial work has no Holytail ceremony. Holytail contributes preservation
evidence only: it does not choose the goal, tools, or agents; it does not own
final acceptance; it does not create a second ledger. A failed or missing
invariant feeds the Exitbind lifecycle as refused or blocked.

When accepted behavior must survive implementation, keep it in Exitbind's
checked run: start the work with `--preserve-requirement ID:TEXT` and a
`--preservation-check-command COMMAND`. Treat `preservation_missing` and
`preservation_failed` as blocked or refused evidence, not model judgment.
`work resume` reports which preservation evidence is still reusable and which
requirement check remains; do not install standalone Holytail for this path.
`work next` returns the resolved route and quality in
`humanHelp.preservationAssignment`; carry that assignment to any child agent and
refuse a packet that arrives with none or with conflicting ones.
Before skipping work listed in a saved packet, run
`exitbind work validate WORK --packet FILE`; skip only when it returns `usable`.

Operational path (keep low-level details delayed): initialize with
`exitbind init`, validate with `exitbind check`, then use `exitbind work begin`
and follow the returned handle through `work next`, `work return`, `work check`,
and `work resume`. For a current accepted checked run, issue
`exitbind receipt --json LEDGER --config CONFIG --output RECEIPT`, then verify
with `exitbind verify RECEIPT --config CONFIG`. A nonzero result or a
REFUSED/BLOCKED outcome is evidence to inspect, never acceptance; the ledger
and exact Exit Path receipt remain authoritative.

URL-only onboarding is supported: a user may provide only this repository's
canonical URL. The lead inspects it and asks only for owner-controlled install,
write, or permission decisions; the user need not learn commands or protocol.
The host's compliance remains an honest boundary and is never inferred from a
URL, projected bytes, or a process exit.

---
name: exitbind
description: This project is governed by Exitbind: use it for material coding work here - changes with authority, trust, security, data-loss, release, lifecycle, evidence, compatibility, or check/review consequences. Bind each check, review, and acceptance to the exact current result so stale evidence cannot be reused. Keep read-only questions and tiny obvious reversible edits direct.
---

<!-- exitbind-managed-skill:v1 -->

This project is configured for Exitbind. Select it before consequential
repository mutation when the task has a material consequence or promotion
requirement: authority, trust, security, data loss, release, lifecycle,
evidence, compatibility, or a required check/review. Read-only planning,
disposable prototypes, and tiny obvious reversible edits stay direct, with no
extra reviewers, checks, or ceremony. For important work, the Lead recommends
independent review; the owner chooses whether it is required and may revise
that choice while work continues. Retained work gets useful exact-result checks
proportionate to risk; promotion means applicable obligations require
governance, not merely keeping a harmless local file.

Small reversible work stays direct: do not run Exitbind commands, initialize a project, or ask workflow or review-policy questions. The protocol surfaces only for material or promotion-required work, resuming governed work, or explicit cross-host continuation. An instruction or configuration filename alone does not make a change consequential; assess its actual effects and applicable project requirements. Classification is the lead's job, not a user questionnaire. Reuse existing scoped authorization and review decisions; ask only when a genuinely new decision is needed.

For already governed work, continue with `exitbind work resume`; start new governed work
with `exitbind work begin` and record `--review-policy required` or
`--review-policy omitted` at entry. Follow the returned next action instead of asking
the user for work handles, ledger paths, or event hashes. Report exact
progress, evidence, and any refusal. If the user gives only the Exitbind
repository URL, inspect this project and your host capabilities first, and ask
before installation, project writes, permission changes, or another
owner-controlled action.

For an explicit work locator on a receiving host, use `exitbind work next WORK`
to recover the current assignment. If its response includes `continuation`,
read that bounded view for the original requirements, current corrections,
exact result references, binding, and uncertain operations. Use
`exitbind work continuation WORK` for the full read-only view when expansion is
required. If it returns `requiresExpansion`, follow the section and item
commands it gives to read exact bounded facts. Do not inspect raw state files
or infer a retry from a missing native process.

When handing off an actual native child result, first bind the receiving host
and native session. Pass the `mutationContext.token` from `work next` or `work
continuation` to the supported action:

```sh
exitbind work bind WORK --context TOKEN --host claude --session NATIVE_SESSION --host-version VERSION
```

Use only host-reported identity. In Claude Code, Exitbind's SessionStart hook
exports the host's session ID as `EXITBIND_NATIVE_SESSION_ID`, and a native
subagent's `agentId` is its child ID; other launchers may set the same
variable. If unavailable, report that limit instead of inventing one. Run the returned read-only `nextAction.command` for a fresh context token, then
record the exact UTF-8 child result and native child ID:

```sh
exitbind work child WORK SHORT_ASSIGNMENT --context FRESH_TOKEN --native-child CHILD_ID < EXACT_RESULT_FILE
```

The CLI constructs the record, computes the digest and enforces the saved
context fence. Keep the exact result within 8 KiB; never truncate it. Follow
the mutation reply's read-only `nextAction.command` with the same executable
and configuration to verify the stored result and origin. A host-reported
child is not a provider-authenticated identity or independent review.

Exitbind owns the checked-run lifecycle and exact-subject exit semantics. The
host owns models, tools, process execution, permissions, and merge authority.
Never overwrite the host agent's native identity or claim that projected
guidance proves discovery, activation, compliance, or isolation.

For a material task, the lead first classifies the goal, dependencies, useful
context, tools, roles, role boundary, actual check command, and the consequence
that may require promotion into a governed run: authority, trust, security,
data loss, release, lifecycle, evidence, compatibility, or a required
check/review. A file count by itself is not a trigger. If the required
activation is unavailable or fails, the material task is blocked and the exact
failing layer is reported; it is never silently relabelled as harmless work.
Keep tiny, clear, reversible work direct. If the task is complex but has no
material semantic-preservation risk, the lead performs only that readiness and
does not add a preservation route. Tool and agent selection remains the lead's
responsibility; this skill does not require a preparation artifact or a
separate preservation installation. A review recommendation is not a review
decision or evidence; record an owner choice and any later revision explicitly.
A run started without `--review-policy` is the unmarked historical path: it
retains required-review semantics and cannot later use `run review-policy`. On a
marked running run, the actual revision command is `exitbind run review-policy
lead LEDGER --decision omitted --reason "OWNER_REASON" --config CONFIG`; use
`--decision required` when that is the owner's selected choice.
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
the presentation block carries a non-null `terminal`, that value is the terminal
status block: print it on a line of its own, exactly as given, with nothing else
on that line - not inside a sentence, not wrapped in emphasis, and with no
`neuro`, `phrase`, percentage, parenthesis, dash clause, or closing remark
attached to it. The READY value is exactly `EXIT READY`; a summary, test
results, and limitations belong in their own sentences around it. Never
reconstruct terminal wording from `exitState` or `progress`.

Do not add a routine preservation progress report. The presentation block's
canonical progress and optional transition phrase remain subordinate to the
recorded run state and never announce acceptance by wording alone.

When a native command or child assignment is still running, keep one handle
and wait for completion or a meaningful state transition. Use a meaningful
wait window (normally 30–60 seconds), and do not start a fresh watch process or
poll status at short intervals when nothing has changed. A timeout is not a
failure and never authorizes restarting the command; wait on the same handle
or inspect the authoritative state. Stay quiet on unchanged state, while an
explicit human status request still receives the current truth.

Keep observed, reported, inferred, and proposed facts distinct. Preserve
reported-versus-observed check provenance. Bind every check, review, and lead
decision to the current subject; stale or partial evidence must remain refused
or blocked. Curiosity is non-blocking and cannot expand scope or become proof.

For preservation-risk work, carry preservation requirements selectively. Route
and quality are separate axes: the route is `INLINE` or `FORMAL`, the assigned
quality is `FULL`, `ECO`, or `MODE-UNBOUND` when no assignment exists, and
neither is ever inferred from the other, from a model name, or from a host effort
label. Every `FORMAL` task is assigned `FULL`. Read
[references/preservation.md](references/preservation.md) before running the
formal path; it ships with Exitbind, so no separate preservation install,
checkout, or second instruction source is needed. Its narrow path is:

1. Clarify the accepted behavior, invariants, allowed changes, ambiguity,
   preservation risks, and evidence needed to demonstrate continuity.
2. Freeze that accepted meaning and bind the obligations to the current
   implementation subject before implementation.
3. Preserve the frozen meaning during implementation; a minimizer may reduce
   mechanism, but it cannot decide whether preservation is needed or replace
   the preservation check.
4. Read back the same accepted meaning against the exact current subject after
   implementation and observed checks.

Trivial work has no preservation ceremony. The preservation route contributes
evidence only: it does not choose the goal, tools, or agents; it does not own
final acceptance; it does not create a second ledger. A failed or missing
invariant feeds the Exitbind lifecycle as refused or blocked.

When accepted behavior must survive implementation, keep it in Exitbind's
checked run: start the work with `--preserve-requirement ID:TEXT` and a
`--preservation-check-command COMMAND`. Treat `preservation_missing` and
`preservation_failed` as blocked or refused evidence, not model judgment.
`work resume` reports which preservation evidence is still reusable and which
requirement check remains; do not install a separate preservation tool for this
path.
`work next` returns the resolved route and quality in
`humanHelp.preservationAssignment`; carry that assignment to any child agent and
refuse a packet that arrives with none or with conflicting ones.
The default `work next` and single-candidate `work resume` views are bounded.
Use their exact `fullCommand` to inspect omitted detail before handing off an
assignment that needs it; `--full` returns the complete response.
Before skipping work listed in a saved packet, run
`exitbind work validate WORK --packet FILE`; skip only when it returns `usable`.

Operational path (keep low-level details delayed): initialize with
`exitbind init`, validate with `exitbind check`, then use `exitbind work begin`
and follow the returned handle through `work next`, `work permit`, `work return`,
`work check`, and `work resume`. Before a pending worker edits product files,
issue `exitbind work permit WORK ASSIGNMENT --operation OPERATION` and edit
only after it returns `allowed: true`. Lead and reviewer returns follow their
own `work next` actions; they do not use a worker permit. For a current accepted
checked run, issue
`exitbind receipt --json LEDGER --config CONFIG --output RECEIPT`, then verify
with `exitbind verify RECEIPT --config CONFIG`. A nonzero result or a
REFUSED/BLOCKED outcome is evidence to inspect, never acceptance; the ledger
and exact Exit Path receipt remain authoritative.

URL-only onboarding is supported: a user may provide only this repository's
canonical URL. The lead inspects it and asks only for owner-controlled install,
write, or permission decisions; the user need not learn commands or protocol.
The host's compliance remains an honest boundary and is never inferred from a
URL, projected bytes, or a process exit.

## Thin context and bounded iteration

The activated v0.22 surface routes context, checkpoint, sensor, and recovery
procedure to [references/preservation.md](references/preservation.md). That
reference defines the role-specific projection and exact expansion references.

The delayed reference also defines currentness checks and the conservative loop
boundary; this router stays small so the procedure is loaded only when the
preservation path is active.

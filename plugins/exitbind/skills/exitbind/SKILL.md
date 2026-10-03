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

Small reversible work stays direct: do not initialize a project or ask workflow or review-policy questions for it. In an already configured project, an explicit request for lightweight direct Codex recording may use `exitbind activity codex < PROMPT_FILE`; its private observation is unjudged, not governed acceptance. The governed protocol surfaces only for material or promotion-required work, resuming governed work, or explicit cross-host continuation. An instruction or configuration filename alone does not make a change consequential; assess its actual effects and applicable project requirements. Classification is the lead's job, not a user questionnaire. Reuse existing scoped authorization and review decisions; ask only when a genuinely new decision is needed.

For already governed work, continue with `exitbind work resume`; start new governed work
with `exitbind work begin` and record `--review-policy required` or
`--review-policy omitted` at entry. Follow the returned next action instead of asking
the user for work handles, ledger paths, or event hashes. `work begin` makes
the new work the project's current-work focus; `work resume` returns that work
and reports older running work only as history (`work resume --history`). The
focus is navigation, never authority. Report exact
progress, evidence, and any refusal. If the user gives only the Exitbind
repository URL, inspect this project and your host capabilities first, and ask
before installation, project writes, permission changes, or another
owner-controlled action.

For a configured Codex worker or reviewer on the selected product-managed
path, use `exitbind work act WORK` for one pending `work next` assignment. It
delivers the current packet and profile, executes Codex, and records the native
result without manual bind, child preparation, or result copying. Use
`--resume` only for a recorded resumable worker turn; an uncertain running
journal is not proof that a retry is safe. Continue with the recorded next
action: a later `work act WORK` or `work check WORK` runs the frozen check,
and the Lead's own `work act WORK --outcome OUTCOME --reason REASON` records
its distinct decision. The commands below govern host-managed child handoffs,
not this product-managed Codex route.

For opted-in durable project lessons, the ordinary recipient context includes
only current applicable reviewed facts, within explicit item/byte bounds and
with an omitted-count inspection route. Consume those facts before selecting
companion edits/checks. They grant no permission or old acceptance. Keep live
tools/resources and transient Work state out of durable memory; only the
configured Lead may change `project-lessons.v1` through the existing reviewed,
expiring memory lifecycle. Follow current returned argv and branch on `effect`,
not process exit zero: held/refused/unknown results cannot advance the workflow.

For a Work already known in this conversation, `exitbind work detail WORK
--json` is the sufficient ordinary read: current recipient profile/rules,
assignment, upstream evidence, task obligations and executable action forms.
Use its worker pre-edit permit and complete return forms; keep review and Lead
acceptance separate. Missing or oversized material remains explicitly incomplete
and uses the emitted expert expansion. Do not repeat profile/rule discovery
when the current complete delivery already contains it.

For an explicit work locator, first run `exitbind work next WORK --json`
to recover the bounded current action. Follow `current.details` exact argv for
complete assignment, evidence and task sections before dependent action or
review; compact status alone is insufficient. Expert `--full` inspection remains
available. Use `exitbind work continuation WORK` for an
initialized same-Work cross-host handoff; follow its `receive` block for binding
and child preparation. If another goal owns the continuation sidecar, preserve
it and continue through `work next`. This read-only continuation view carries the original
requirements, current corrections, exact result references, binding,
uncertain operations, `mutationContext.token`, and a `receive` block with the
bind and native-child commands. If it returns `requiresExpansion`, follow the
section and item commands it gives to read exact bounded facts. Do not inspect
raw state files or infer a retry from a missing native process. `work next`
remains the Lead's assignment view.

When handing off a host-managed native child result, first bind the receiving host
and native session. Pass the `mutationContext.token` from `work next` or `work
continuation` to the supported action:

```sh
exitbind work bind WORK --context TOKEN --host claude --session NATIVE_SESSION --host-version VERSION
```

Use only host-reported identity. In Claude Code, Exitbind's SessionStart hook
exports the host's session ID as `EXITBIND_NATIVE_SESSION_ID`; other launchers
may set the same variable. If unavailable, report that limit instead of
inventing one. Run the returned read-only `nextAction.command` for a fresh
context token, then prepare the child before launching it:

```sh
exitbind work child prepare WORK SHORT_ASSIGNMENT --context FRESH_TOKEN
```

Then launch the native child normally. In Claude Code, Exitbind's managed
subagent hooks capture the next compatible subagent this session starts: they
record its host-reported ID and its final message exactly as the host delivers
it, so do not copy either yourself. Add `--agent-type TYPE` when other
subagents may start first. A subagent started with no prepared intent is never
recorded. Read `exitbind work continuation WORK` afterwards: `preparedChildren`
shows the capture state and `children` the recorded result. If a finished
child stays `prepared` or `claimed` (a host without these hooks), record its
exact final message with `exitbind work child WORK SHORT_ASSIGNMENT --context
FRESH_TOKEN --native-child CHILD_ID < EXACT_RESULT_FILE`, within 8 KiB and
never truncated; if it is `failed` with a retained result, run the `recover`
command it names. A host-reported child is not a provider-authenticated
identity or independent review.

Exitbind owns the checked-run lifecycle and exact-subject exit semantics. The
host owns models, tools, process execution, permissions, and merge authority.
Never overwrite the host agent's native identity or claim that projected
guidance proves discovery, activation, compliance, or isolation.

One assignment can contain several required outcomes. Retain their dependencies,
evidence, failure and return conditions in the existing plan; a completed part
does not close the whole goal. Use useful granularity without requiring an agent
or Work for every part. Treat causal explanations and impact maps as provisional:
investigate plausible wider links within read authority, return basis or repair
boundary contradictions to the Lead, and revise affected assignments and evidence
through this protocol before dependent implementation. Preserve scoped recipient
and visibility restrictions in relevant handoffs. Personal profiles and lenses
augment judgment while Exitbind owns its state and acceptance routes. Adapt
reasoning to the task without a fixed thinking sequence or model choice.

Before first implementation, carry the applicable goal, tasks and evidence,
required outcomes and their failure or return conditions, constraints and
non-goals, current source/result/recipient/operation identity, known decisive
positive and negative cases, and corrections or open questions. Map the relevant
consumer, help and packaged mirror surfaces that the source identifies and check
their focused positive and negative fixtures. Keep that map bounded and
source-supported; it is not a universal dependency graph or a readiness score.
Missing required detail remains explicit and follows the current supported read
route.

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
A reviewer's launch context names the current check records and logs with the
`work expand` commands that read them; hand the reviewer its assignment, not a
summary of the checks.
In marked work, a reviewer `rework` waits for the Lead instead of starting a
worker. A finding is evidence, not a requirement. Decide it with
`exitbind work disposition WORK ASSIGNMENT --decision repair|defer|reject|supersede
--reason TEXT`: `repair` also takes your own `--repair-boundary` and
`--regression`, `supersede` takes an explicit `--successor-basis`, and `defer`
or `reject` keep the finding without new work. A deferred or rejected finding
is not an approval.
For a clear defect inside the accepted task, record `repair` and proceed without
asking the owner again. Ask only when the finding changes accepted scope,
authority, review choice, or an irreversible decision.
When the governed path is selected, inspect the bridge end to end: source,
projected skill/config, fresh-session discovery, invocation, observed behavior,
and outcome. Run the normal Exitbind lifecycle and keep the ledger/Exit Path as
authority; process survival or an agent's report is not acceptance.

Legacy `presentation.neuro` is retained for machine compatibility and describes
current-result prerequisite readiness. It is not overall-goal completion,
elapsed effort or a delivery estimate. The normal human line comes from the
product's English `goalProgress.systemText`; a model must not render the legacy
percentage, invent a replacement or narrate progress each turn. Optional
phrases and terminal state remain product-owned.
When `presentation.goalProgress` is present, the product has already generated
its bounded English `systemText` from the canonical external goal and Lead task
records. Supported native and host paths surface that text automatically; do
not ask a model to calculate percentages, remember a progress command, or
reformat the goal/task states. Its `resultReadiness` remains separate from
overall and task completion, and unavailable canonical decomposition stays
explicitly unavailable. When
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
`exitbind init`, or preview known owner-approved facts with
`exitbind setup --mode portable --root . --scope worker --observe PATHS --write PATHS --commands FACTUAL_COMMAND --check-command CHECK_COMMAND --goal GOAL --review-policy required|omitted`.
Review setup's affected paths and host mapping, then repeat it with `--apply`.
Applied setup returns validated configuration identity and exact next argv;
use that current action instead of rediscovering it with a separate check.
Role-specific `--lead-observe`, `--worker-write`, and corresponding per-role
observe/write/commands flags express asymmetric approved facts in one application;
`none` explicitly supplies an empty list. Shared scope/facts remain compatible
but cannot be mixed with per-role flags. Use `exitbind work begin`
and follow the returned handle. The product-managed Codex route uses `work act`
as described above. The host-managed child route uses `work next`, `work permit`,
`work return`, `work check`, and `work resume`. Before a pending host-managed
worker edits product files,
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

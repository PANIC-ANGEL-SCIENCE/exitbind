# Complete a goal with several requirements

## Native conversation Goal and Work context

When Exitbind runs with one unambiguous host-reported native session identity,
Goal operations use that conversation's canonical Goal journal. Codex
`CODEX_THREAD_ID` or `CODEX_SESSION_ID` and Claude Code
`CLAUDE_CODE_SESSION_ID` are treated as correlation locators. If multiple
selectors are present they must identify one host, and the Codex selectors
must agree; a conflict or malformed selector declines native lookup. Claude
Code's own `CLAUDE_CODE_SESSION_ID` names the current session, so it is used
when an exported `EXITBIND_NATIVE_SESSION_ID` still names an earlier one. These values identify a journal;
they do not authenticate the caller or grant host authority. Session journals
are private user-state data, separate from configured project Work/evidence
state, and are fixed to the project that first incorporates the Goal.

With no recognized native identity, Goal commands keep their existing
configured-project behavior. Under a native identity, use `goal ... --project`
to explicitly select that project's legacy Goal journal. Native lookup never
falls back to the project journal when the session Goal is absent, closed, or
unavailable. A project other than the fixed owner may read a bounded
context-only Goal preview, but cannot edit its requirements, map named
coverage, evaluate owner evidence, or close it.

A new session of the same host (after `/clear` or a restart) has no Goal of
its own. When the project's Work records name exactly one open Goal journal of
the same project, every UNAVAILABLE Goal surface (`goal status`, the
SessionStart hook, `work next`, `work resume`) prints one full
`goal adopt --session-key KEY` route. Running it records the adoption in that
journal (one new revision) and in the new session's own owner record; from
then on both sessions read and write the same journal. A journal of another
project, a closed Goal, a session that already owns a Goal, or two reachable
open journals are refused with nothing written; then no route is printed.

An ordinary `work begin` in a native conversation links the new Work to the
current conversation Goal when one exists, even if the Work belongs to a
different configured project or has a narrower task scope. Before starting the
Work, Exitbind records a bounded pending admission under the Goal's journal
lock. The admission reopens an earlier closure and blocks a new whole-journey
terminal until the exact immutable Work start and context link are verified.
It is an unresolved intention, never evidence that a Work started or finished.
Work checks and evidence remain in the Work's configured project. Without a
usable native identity or a current conversation Goal, the Work starts without
a session link; a Goal created later does not retroactively attach it. Existing
single-Work continuations are not claimed by new ordinary Works. Same-Goal
updates retain admissions; a successor Goal carries the same conversation's
members and unresolved admissions with their original Goal bindings.

Use `work begin ... --standalone` when a Work should not receive conversation
context. `--standalone` cannot be combined with the explicit named-requirement
contract options (`--goal-id`, `--requirement`, `--artifact`, or `--scope`).
Those options retain frozen admission and explicit assignment behavior. By
default, a named contract selects the conversation Goal when a valid native
identity is present. Add `--project-goal` to select a project-local named
contract while keeping the ordinary conversation context link. Named mappings
and their evidence always stay within the selected contract's configured
project boundary.

The owner Goal record stores context links under the versioned
`workAssociationProtocol: 1` extension and journey membership/admissions under
`journeyMembershipProtocol: 2`. Both use bounded exact Work and start identities.
Membership protocol 1 remains historically readable, but it cannot establish
admission coverage or whole-journey readiness. Each linked Work also gets an
immutable private locator bound to its exact start. The locator helps resolve
the owner journal; it grants no authority. An unknown or malformed Goal
extension is refused. Older readers may ignore these fields, and an older
writer may drop them; mixed-version mutation is unsupported. Work start records
remain unchanged, so older readers can still read the Work but will not display
the new session context.

Native journey membership is a closure dependency: each admitted task must have
an accepted, current result or be covered by a separately selected, final-
current journey integration. A context-only link does not satisfy a named
requirement, create a mapping, or import another project's acceptance. Named
requirement coverage remains explicit and project-bound.

Each journey member keeps the configuration sha256 it was recorded under and
is never restamped. When the configuration changes later, the member stays
integrable only on proof by those stamped bytes: the copy kept under their
hash beside the lesson ledgers (`MEMORY_ROOT/stamped-config-*.json`, which
needs a memory policy), whose agent authority and read rights equal the
current ones, so only `profile`, `nativeName`, `displayName` or
`phaseGuidance` changed. The member's acceptance is then read under the
stamped bytes. The configuration writer (`setup --apply` and the other
product writers) keeps that copy when a journey member of the owner's Goal
carries the hash. After a hand edit the product never saw, `goal prepare`
omits `--journey-integration`, `work begin --journey-integration` refuses and
`goal status` marks the members unavailable, each with one route
`memory revalidate LEAD --from-config <PREVIOUS_CONFIG> --reason <REASON>
--apply` naming the members; run with the owner's copy of the earlier bytes,
it keeps them under their hash (with no lesson ledger listed, keeping is all
it does). A file whose hash is no stamp refuses and keeps nothing. Any other
difference (another project, session, owner or root, or an authority or
read-right change) refuses as before.

The `exitbind.session-journey-integration.*` basis constraints are reserved for
the explicit `--journey-integration` path. Do not copy or author these markers
in `--basis`; Exitbind rejects caller-supplied reserved markers before starting
the Work or recording its admission. Pending admissions reserve slots within
the existing 32-member journey bound, so a Work is not started when no member
slot remains.

If a begin reply reports an unresolved native admission, inspect Goal status and
the exact Work detail. Run `work link-session WORK` only after the actual Work
start exists. This recovery verifies the current host-reported session, fixed
owner, configured project and immutable start, then writes only the missing
context link; repeated exact recovery is idempotent. It does not repeat begin,
check, accept, grant a permit, or contact a provider. An absent or invalid Work
start remains unresolved and blocks whole-journey closure. This release has no
automatic cancellation or withdrawal for a no-start admission.

Give your lead the goal in your usual conversation. The lead keeps the agreed
source and each accepted meaning, then assigns useful work. One Work can cover
several requirements, and several Works can contribute to the same requirement.
A mapping records an assignment; it becomes current support only after actual
checks, the selected review and Lead acceptance. The pre-existing project-local
named-requirement path is included in the stable v0.31.23 installation; the
native conversation linkage described above is also included in the stable v0.31.23 installation.

## Inspect and continue

Use `exitbind goal status --json` in the selected Goal journal. Its `requirements`
view shows the exact goal revision, source references, requirement IDs/revisions,
active mappings, supporting subject, check/event identities, observed acquisition,
acceptance and the next available action. States distinguish `unmapped`, `pending`,
`failed`, `stale`, `unknown` and `current`. Historical closure and current readiness
are separate. A fresh CLI process can inspect this same saved state; it does not
renew a grant or change a Work's review choice.

The Goal's short text (`goalProgress.systemText`, also printed by Work
commands, the `setup --apply` response as `goalText`, and the SessionStart
hook) ends with two facts. For a native conversation Goal it states the
journey: its member count, how it closes (each member accepted and current,
or one final `--journey-integration` Work; a stale accepted member needs the
integration Work), and the read route for members not yet current. Once the
Goal is closed it states only the member count and that the journey closed
with the Goal, and the SessionStart hook prints one line saying the Goal is
closed with nothing to resume. It always
states the project-lesson state: `Lessons: none configured` or
`Lessons: N deliverable`.

`goal status` also returns the current disposition for each of the five Goal
categories, the valid category states, and the valid item dispositions in a
bounded list with exact Lead-decision routes for unresolved categories and
items. Empty categories offer their own `none-applicable` choice; status never
marks every category on the Lead's behalf. Before Work freezes a mapping,
`requirements.preFreezePreparation` and `goal prepare` show the named IDs,
checker identity and separate begin route. Preparation creates no Work and
does not change prior coverage. Without `--artifact`, `goal prepare` and the named
`work begin` route use the tested project file the check command names,
directly or through one outside script it runs (`sh /abs/check.sh` whose
text runs `./scripts/ci-local.sh`); zero or several candidates is an error
that asks for `--artifact`. The emitted begin route carries the checker it
found. A close route appears only when the same
journey, coverage, integration, evidence and category predicates used by
`goal close` currently pass; otherwise status keeps repair or integration
actions visible without claiming readiness.

Status also identifies stale or unavailable evidence attached to an additional
Goal category. If that evidence alone blocks an otherwise current close, its
route names the exact category item and disposition for an explicit Lead
re-evaluation. A direct item records a new Lead observation against current
project inputs; a result-backed item requires the Lead to select an accepted
Work that supports that item. Neither route starts another integration Work,
refreshes evidence automatically, or changes an earlier accepted Work.

For a project-local requirement contract, retain the agreed request in a
regular project file and explicitly select the project journal with
`--project`. For example:

```sh
exitbind goal incorporate --project --goal-id import --goal 'Import names safely and show a count' \
  --none-applicable findings,blockers,decisions,externalActions
exitbind goal require --project --goal-id import --requirement R1 \
  --obligation 'Reject invalid input before persistence.' --artifact request.txt
exitbind goal require --project --goal-id import --requirement R2 \
  --obligation 'Persist each valid name only once.' --artifact request.txt
exitbind goal require --project --goal-id import --requirement R3 \
  --obligation 'Show the number of distinct imported names.' --artifact request.txt
exitbind goal cover --project --goal-id import
exitbind goal status --project --json
```

The three texts must be exact excerpts of `request.txt`. `goal cover` records the
Lead's judgment that the finite set covers the agreed scope. An exact excerpt
or digest does not establish that no natural-language meaning was omitted.
Keep any additional findings, blockers, decisions or external actions in the
existing goal categories; they remain closure conditions.

A configured project checker must exercise the declared behavior. This example
assumes `check.sh` checks R1 and R2:

```sh
exitbind work begin change --goal 'Validate input and duplicate-safe persistence' \
  --goal-id import --requirement R1,R2 --artifact check.sh \
  --check-command 'sh check.sh R1 && sh check.sh R2' \
  --review-policy required --project-goal --detail
```

Follow the returned current context and action forms for scope, implementation,
actual checks, review and Lead acceptance. Coverage is frozen before execution. `--scope integration` declares that the checker tests the final composition; listing all IDs alone does not make that declaration.
`goal assign` cannot convert an unrelated successful Work into support. A lost
begin reply is recovered with `work resume` or the known `work detail` route;
do not repeat begin. `goal require`, `goal cover` and repeated identical mappings
have no duplicate effect. A new Work is a new assignment, not an idempotent retry
of begin.

## Validate the final integration

Passing the separate parts does not establish correct composition. Create a
legitimate validation Work on the current integrated inputs, covering all agreed
IDs and exercising their interaction:

```sh
exitbind work begin change --goal 'Validate the complete import behavior' \
  --goal-id import --requirement R1,R2,R3 --artifact check.sh \
  --check-command 'sh check.sh all' --scope integration --review-policy required --project-goal --detail
```

After its actual checks, selected review and Lead acceptance, use the returned
Work identifier as `NEW_ACCEPTED_WORK` below. When it replaces earlier validation
contributors, make that decision explicit:

```sh
exitbind goal assign --project --goal-id import --requirement R1,R2,R3 \
  --result-ref NEW_ACCEPTED_WORK --disposition replace
exitbind goal close --project --goal-id import --result-ref NEW_ACCEPTED_WORK
exitbind goal status --project --json
```

Replacement retires the selected items' earlier active mappings and retains
history. It never removes requirements or reopens accepted Work. Without
replacement, every active contributor remains required. Correct a meaning with
`goal require` using the same ID and its new agreed source excerpt: the revision
increases, old meaning remains in history, coverage needs confirmation, and old
support cannot satisfy the new revision.

Exitbind automatically supplies the completion block in `presentation.terminal`
on existing goal and bound Work responses only after current whole-goal checks,
selected review, final integration and explicit Lead closure. Finishing a part
does not emit it. Ordinary `goal close` and `goal status` print only the block
when this named goal is currently complete; `--json` retains structured evidence
and the same product-owned block. No extra display command or model call is
needed. Stale or unknown evidence suppresses it without erasing historical
closure.

Currentness conservatively covers tracked and non-ignored project files, or all
walked files outside Git, excluding Exitbind state. A shared input or checker
change invalidates prior support even when its result artifact stays unchanged.
Ignored/external files, environment, remote services and time are not covered.
The declared checker must be a regular non-ignored project file named by the
check command. The freeze covers that project file and the bytes of the one
outside script the command names; the script's environment and the files it
sources are not frozen (a sourced tracked project file is still covered as a
product input), so a per-Work input such as the comparison base belongs
there or in a ref resolved at run time, never as a literal in the script.
This relationship and checked evidence are observable; whether
that checker adequately tests the requirement remains a review judgment.
Project identity includes the canonical root path: recovery in the same project
is supported, while moving this goal to another path requires new validated
work. This is not a cross-path portability claim.

A native Goal may follow an exact, recorded same-Goal Work successor created
under a changed configuration when the saved member configuration and the
explicitly selected endpoint configuration both validate the original project,
state root, and session owner. Exitbind checks every recorded supersession
claim and only recognizes configurations whose identities are available from
that saved member or the selected endpoint; it does not search for missing
intermediate configurations. If any link or identity cannot be verified, the
member remains unresolved and cannot support current closure.

## Reproduce the process scenario

From this source checkout, run:

```sh
cargo test --locked --features repository-checks --test goal_basis_requirements \
  goal_requirements::three_requirements_multiple_works_interruption_drift_composition_and_closure -- --exact --nocapture
```

The packaged CLI creates an isolated import example with a shared format file
and a retained names file. Repeated imports check the saved bytes for duplicates;
rejected input must leave those bytes unchanged.
It shows partial observed checks with one unmapped item, recovery in new CLI
processes, shared-format drift, passing individual checks with failing
composition, a new integrated validation Work, explicit closure and stale
readiness after a later change. Normal CI runs this target. Its role decisions
are deterministic fixtures: the scenario proves process and evidence behavior,
not an independent semantic review, live-model handoff, human adoption or a
quality/token advantage. No private helper or raw goal-ledger editing is needed.

This bounded path allows at most 32 requirements, 128 mappings and 32 corrections
per requirement. The agreed source is at most 32 KiB; an exact meaning is at most
1024 bytes. These limits keep the local representation finite. Existing ordinary
multi-obligation goals and older single-Work continuation records retain their
supported behavior.

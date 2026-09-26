# Continue one work item across Codex and Claude Code

Small reversible work stays direct; the protocol surfaces only for material or
promotion-required work, resuming governed work, or explicit cross-host
continuation. Exitbind keeps the portable work identity in the existing run
and canonical session-goal histories. Native sessions and children remain in
their own hosts.
This local-workspace path assumes the same authorized operating-system
principal on both hosts; a host name supplied to the CLI is an observation, not
an authentication credential.

Start from an explicit `smw_…` work locator. `exitbind work next WORK` includes a
bounded `continuation` view when this work has been initialized. If that field
says `requiresExpansion`, run `exitbind work continuation WORK` before acting.
The latter is read-only and includes the source requirements, current lead
binding, corrections, result references, operations, diagnoses, children, and
remaining requirements. It never launches or retries an operation. `work
resume` includes the same view only when discovery finds exactly one current
work item. An ambiguous resume still requires an explicit locator.

The coordinating lead initializes or attaches the continuation once by piping
a JSON object to `exitbind work record WORK`. For an existing open canonical
goal with the same work identity, include its current `expectedRevision`; an
identical retry leaves history unchanged. A conflicting source or a closed goal
is refused. For a successor work, explicitly incorporate that work as the
current goal before attaching, preserving the predecessor's history.
`sourceText` retains the original request; every initial requirement must be a
unique, exact excerpt of it.
Unrepresented source clauses remain a coverage question until a lead records
an explicit `cover` assertion against the source hash. That assertion is
`lead_asserted_review_required`, not semantic proof by itself.

```json
{
  "action": "init",
  "sourceRef": "requirements.md",
  "sourceText": "First requirement. Second requirement.",
  "requirements": [
    {"id": "first", "text": "First requirement."},
    {"id": "second", "text": "Second requirement."}
  ]
}
```

`work bind WORK --context TOKEN --host HOST --session SESSION --host-version
VERSION` records the receiving host and native session. Take `TOKEN` from the
read-only view's `mutationContext.token`; the CLI carries its goal and binding
revisions. Repeating the identical bind has no effect; a stale or conflicting
bind is refused without fetching a newer revision. The expert `work record`
surface remains available for other continuation mutations. A `refine`
increments only the named requirement revision and retains its prior text.
Existing support remains recorded but no longer applies to the new revision.

`work record` returns a bounded mutation outcome with its revision, append
effect and exact record identity. Follow its read-only `nextAction.command` to
recover the bounded view; the command carries the originating executable and
configuration when they fit. If either cannot fit, its corresponding
`sameExecutableRequired` or `sameConfigRequired` flag requires that exact
invocation context. Section, item and history routes use the same rule.

`support` accepts only a passing observed check event from this exact work
whose recorded `requirementId` matches the named requirement. It also binds
the requirement revision, source hash, tested inputs, acquired configuration and
conditions hash. A check without acquired configuration provenance cannot be
attached as current support; a later caller's conditions hash cannot supply it.
A refinement records the current check-event floor, so an
older event cannot be attached to the new requirement revision. This floor
orders events in the selected single-workspace path; it is not an atomic
transaction across concurrent goal and run writers. `work continuation`
rechecks the event read-only and reports
integrity separately from current applicability. Its `exactRead` locator uses
`run inspect LEDGER --event EVENT`; reading it does not execute the check or
resubmit a held result. A passing check for another requirement or a check
with missing artifact bytes cannot close this one; whole-goal readiness still
requires explicit lead closure.

`correct` records an operator-asserted scoped correction and an optional
same-scope supersession. The CLI cannot independently prove the caller's human
authority. `intent` records an operation before dispatch with a stable request
ID, parameter digest and origin-qualified binding. Its initial status is
`uncertain`; duplicate identical intent is harmless, while conflicting
parameters are refused. `observe` records a status or native handle from the
origin host. No missing process-list entry authorizes another dispatch.
`diagnose` records the observation class, conditions and invalidation rule.

`work child WORK SHORT_ASSIGNMENT --context FRESH_TOKEN --native-child CHILD_ID`
accepts exact UTF-8 text from standard input or `--result-file`; use the new
token returned after `work bind`. The CLI constructs the existing record and
SHA-256 digest. The child is scoped to the current receiving host and session
and marked `host_reported_native_child`: transporting bytes does not make it
an independent review or Lead acceptance. The result limit remains 8 KiB;
larger outputs need a separately assessed artifact path and cannot be silently
shortened. The view is bounded at 64 KiB. An unavailable native session or
child ID must be reported rather than invented.
When a complete view exceeds that bound, it returns section commands; an
oversized section returns indexed item commands. For example,
`work continuation WORK --section children --index 0` reads one exact result
without opening an internal ledger.
An oversized requirement item returns a `historyRead` route; use
`work continuation WORK --section requirements --index 0 --history-index 0`
to read one prior revision. At most 32 prior revisions are retained per
requirement, and a further refinement is refused rather than making the
current requirement unreadable.
Only the relevant local task state should cross hosts; transcripts, hidden
reasoning, credentials and unrelated personal memory do not belong in these
records.

This path does not migrate a live process between hosts, authenticate native
session claims, control external shell effects, or replace each host's own
permission decisions. When the destination cannot perform an action, keep the
obligation and report the limitation instead of broadening permissions.

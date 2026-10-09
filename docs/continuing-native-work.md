# Current context, managed file edits and the quiet hall

## Product-owned goal and result status

When a Lead has registered the external user's goal with `goal incorporate`
and divided it with one or more `--obligation` entries, supported Work and run
responses include a bounded `goalProgress` projection. Its `systemText` names
the overall goal state and the Lead's completed-task count and task states. The
projection is generated from the canonical goal record; a model does not need
to remember a progress command or calculate a percentage.

`goalProgress.resultReadiness` is separate from goal completion. It reports the
current Work result's prerequisite state and reason. A task whose recorded
result is historical or whose tested inputs drifted remains visible as
performed but stale, and is not counted as current completion. If no canonical
goal or task decomposition exists, the projection says `unavailable` rather
than inferring one from a worker packet. `goal status --json` and the normal
human `run status` route surface the same product-owned text.

Each task keeps a bounded human `label`/`id` and a stable `taskId` hash of the
canonical obligation identity, so long task names cannot collide in compact
output. The Work view supplies a scoped tasks route for every task, including tasks omitted from the compact projection.

Use `goal status --json` to inspect the current projection. Existing full
responses and exact result, review, acceptance, and receipt semantics remain
unchanged; this is a derived read-only view.

These additions extend the existing Work path. For a known Work or explicit
locator, start directly with `work detail WORK --json`. Unknown project work
uses `work resume --json` and its `current.details.grouped` route for one
product-owned readable assignment/evidence/tasks delivery, or use the three
section routes when an expert needs legacy paging, and follow the returned assignment
with its configured `actionForms.productManaged.command` for Codex or the
host-managed permit/return forms. For the optional enforced Linux file cell, see
[controlled file effects](controlled-file-effects.md); ordinary workspace-write
remains mediated/advisory. Checks, independent review and explicit Lead acceptance
still have their own results. Normal `work act`, `work check`, and `work
return` executions emit the same product-owned goal line on bounded stderr;
`--json` keeps stdout machine-only while retaining that bounded human channel
on stderr.

## Current binding and complete detail

The normal 8 KiB response exposes `current` version 1 across worker, check,
reviewer, Lead, repair and terminal states. It names the Work and goal identity,
current result availability and subject, phase/attempt, recipient and action,
missing prerequisites, and any recorded rework event. A result that does not
exist is distinct from an unavailable identity. Historical acceptance remains
historical when current inputs have changed.

Use the exact argv in `current.details.grouped` for ordinary readable
consumption. `work detail WORK --json` is the equivalent current read when a
caller has the Work handle. Use the argv arrays in
`current.details.assignment`, `.evidence` and `.tasks` for the legacy expert
routes.
They pin this executable and configuration even from another directory or a
hostile PATH. A route marked `sameExecutableRequired` or `sameConfigRequired`
requires that exact invoking value before execution. These read-only routes
create no grant, launch no provider and change no managed-edit baseline.

`work expand WORK REFERENCE --json` returns exact JSON section bytes encoded
as hex, with their hash, total byte count and offset. Each page carries at most
24 KiB of section bytes; the complete response stays within 64 KiB. Follow
`next.command` until null, concatenate the pages in offset order, verify the
section hash and parse the decoded JSON. `pageComplete` applies to one page;
`complete` only means the whole section fits in that response. All pages must
name the same binding and section hash. Refresh on any mismatch or refusal;
do not combine old and new sections.

The versioned grouped route (`ref:work-detail:v1:<binding>`) returns
`kind: work_detail`, `version: 1`, the current recipient and binding, and
`sections.assignment`, `.evidence`, and `.tasks`. Verified UTF-8 artifact bytes
appear as `content` with `encoding: utf-8`, their original `bytes` and `sha256`,
and `readable: true`; the product performs decoding and currentness checks. The
grouped response is bounded at 256 KiB and has no model-authored paging.
Non-UTF-8 or bytes beyond the verified preview bound remain exact metadata with
`readable: false`, a reason, and an instruction to use the current scoped
evidence metadata route; oversized evidence has no complete text fallback. They are never replaced by lossy text. `complete` is false
whenever a required readable artifact is unavailable.

The assignment section groups the complete assignment and residual rules;
the evidence section supplies the actual verified submission and check-log
bytes; the tasks section contains every Lead-divided task state. Legacy
section expansion refuses missing or oversized verified evidence rather than
becoming a summary; grouped expansion reports an explicit unreadable limitation
with its current fallback route. References
expire on changes to the project, configuration, Work, current recipient,
assignment, ledger, host execution conditions or relevant task conditions. A reference grants no access
to another Work or private role. `--full` remains available for expert inspection.
Native delivery uses the same validated grouped consumer before launch, then
supplies the canonical assignment, all mandatory rules and required reviewer
artifacts. The prompt carries the exact current grouped argv for any follow-up
read; a native consumer does not implement hex decoding, page loops or checksum
validation. Automatic progress presentation stays on the host; its display
payload is omitted from the provider assignment.

The current view also exposes a versioned `actionForm` for Lead states. A
failed check offers assignment-bound `work return WORK ASSIGNMENT --outcome
rework|blocked --reason <REASON>` descriptors. A pending review finding offers
only the decisions present in its current disposition cycle, with the required
repair or successor fields. Lead acceptance exposes bound accept, rework and
blocked choices. Placeholders are explicit Lead inputs. Forms include `--current-binding` so a
changed goal or ledger also refuses an old form even if its assignment ID is unchanged.
Legacy calls retain their existing assignment fence.

## What a native assignment receives

Before a provider starts, Exitbind delivers the current configured role,
project identity and location, Work and assignment identities, requested
native binding, complete current `AGENTS.md` and `CLAUDE.md` bytes when present,
and complete sources for eligible, accepted, opted-in role memory. A new Work
gets a fresh projection; its predecessor's approval and mutation grant do not
carry over. Work identity, role identity and provider session are separate.

Memory remains disabled unless configured. Revoked, expired, scope-ineligible,
changed or unsafe sources are excluded or refuse launch through the existing
memory checks. Ambient transcripts are never collected. The provider receives
the selected bytes; this observation does not prove that a model followed them.
Memory is context, and cannot expand the assignment's authority.

For ordinary host-managed consumption, pass the complete current recipient
context and its emitted actions from `work detail WORK --json` (or the
equivalent native start response). Use the selected role, profile, rules and
declared boundary as delivered; do not add parallel permission prose or attach
extra sources to widen them. An empty `observe` list stays empty, and task
intent does not grant a read. If the task needs more information, use evidence
the authorized recipient may already read or ask the Lead to make a
prospective, explicit scope change before acting.

When the emitted pre-edit action is used, require its exact `allowed: true`
result before any dependent action. The permit appends to the Work ledger and
changes the current detail binding. Read current detail again, confirm the same
Work, assignment, role, profile, rules and declared boundary, and then use only
the actions bound to that fresh response. A denied, missing, malformed, stale
or interrupted acquisition dispatches no dependent action. Correct an invalid
operation from the current form; reread after stale currentness; inspect an
uncertain result. Do not blindly replay an uncertain action.

These declarations and native action paths guide cooperative hosts. Supported
managed file effects enforce their own current assignment, observation and
write checks; other host tools keep the permissions granted by the host. This
does not turn role text into an operating-system sandbox.

Delivery refuses oversized content rather than silently shortening it. Each
project rule is at most 32 KiB and all current project rules together are at
most 48 KiB; the JSON-encoded rules in ordinary detail are also limited to
48 KiB to leave room within its 64 KiB response bound. The native current
context remains bounded at 64 KiB, and the Codex prompt at 256 KiB. Memory
sources remain at most 16 KiB, at most eight memory items are included, and
each memory reference is at most 8 KiB. Narrow an explicit selection or split
an oversized source before trying again. A configured non-Codex runtime cannot
be launched through the Codex executor.

## Read and edit one UTF-8 file

The supported native worker launch supplies a private file tool bound to that
assignment. The Lead issues the current worker's mutation permit through the
existing Work path. Use the supplied tool's exact path as `EDIT` below:

```sh
"$EDIT" read src/example.txt
"$EDIT" edit src/example.txt < replacement.txt
"$EDIT" inspect src/example.txt
```

`read` returns JSON containing the captured UTF-8 content and an `exists` flag;
explicit absence supports creation. Prepare the replacement from those bytes.
Repeated reads retain that baseline rather than silently rebasing an edit onto
newer disk content. The worker supplies the path and complete replacement,
without computing hashes, selecting a Work or inventing operation IDs.
The tool pins its invoking executable and exact configuration, so another
working directory or `PATH` cannot select another project or binary.

Retry the same submitted replacement using the same command and content.
A completed replay returns `effect: no-change` without another replacement,
only while the recorded result remains applicable. Changed parameters refuse.
For an intentional next edit, `"$EDIT" refresh src/example.txt` captures a new
baseline and returns its content. A refused stale-read edit preserves the newer
file; inspect it and refresh before preparing a different replacement.

An unresolved admitted effect, missing effect record or corrupt/missing read
binding refuses replay and refresh. `inspect` reports the observed file state
and available request/effect status; unresolved effects require Lead inspection
and have no automatic reset here. Returning to an old assignment does not
restore its authority. The managed route enforces permitted observation as well
as the existing write boundary. A successful edit is separate from worker
submission, checks, review and acceptance.

An immutable preparation record beside the canonical Work ledger is synced
before session initialization. Losing the session directory, or interrupting
its first initialization, refuses preparation and native launch rather than
recreating read history. A missing, corrupt or mismatched preparation record
also refuses an intact session. Simultaneous loss of both that record and the
session cannot be distinguished from first use; preserve private StateRoot
integrity rather than deleting selected task records to recover an edit.

Files and replacements are limited to 256 KiB. Managed product observations
require a single-link regular file: hard links,
including ordinary source aliases and aliases of private evidence, refuse
read, refresh and inspect without returning or storing their content. The
actual no-follow descriptor is checked before reading and before returning.
The existing expert command keeps its contract.

Private read storage is bounded to 64 paths and 2 MiB per assignment, including
serialized content. Records live
under the existing private state namespace, outside tested product inputs;
they retain file baselines until that task's state is removed. Direct small
work does not require this tool. Reads performed through unrelated native tools
are not captured baselines, and other host tools retain their permissions.

## Existing expert replacement command

Expert callers can replace a file within the current worker's declared write paths after
receiving a mutation permit. Use the exact current assignment from `work next`:

```sh
exitbind work permit WORK ASSIGNMENT --operation source-update
exitbind work write WORK ASSIGNMENT src/example.txt \
  --operation write-example-v1 --expected-sha256 absent < replacement.txt
```

For an existing file, replace `absent` with the SHA-256 of its current bytes.
Standard input is the complete UTF-8 replacement, at most 256 KiB. The command
refuses a different file state, a stale or completed assignment, a missing
current grant, an out-of-scope path, symlinks, and protected configuration,
profile, evidence and host-control paths, including nested `AGENTS.md`,
`CLAUDE.md` and native control directories. It replaces the name atomically and
preserves ordinary Unix permission bits of an existing file, excluding set-id
bits. A task ledger lock fences assignment transitions during the replacement.

Keep the operation ID for this exact request. An identical completed retry
returns `replay: true` and `effect: no-change` only while the recorded file
bytes still match. A changed request under the same ID is refused. An admitted
operation whose completion is unknown is also refused: inspect the actual file
and resolve the uncertainty before choosing a new operation. A missing native
process does not authorize another effect.

This mediator controls this file-replacement class. Native tools outside it
retain their host permissions. It does not intercept arbitrary shell writes,
network requests or remote APIs, authenticate the local operating-system
principal, or guarantee exactly-once external effects. A local principal able
to modify control state is trusted; this is not an OS sandbox.

## Read the quiet hall

```sh
exitbind work world WORK
exitbind work world WORK --plain --reduced-motion
exitbind work world WORK --export --json > world.json
```

The static terminal renderer uses six motifs. Each active marker comes from
the corresponding current record, without changing the records that decide
checks or acceptance. `--plain` has the same status and markers; all rendering
is static, so reduced motion requires no alternate animation. The local text
view includes the canonical Work marker to distinguish tasks. Export includes
renderer status and motif state only, excluding local Work/session markers,
paths, source text and logs.

| Motif | What activates it |
| --- | --- |
| The same door | A confirmed receiving child after a binding change, or an actual saved native return/replay from the same Work |
| A passing black cat | The current `work act` response replayed a recorded native result without another provider execution |
| A distant payphone | A current bound child was claimed or its native result was recorded; binding alone is insufficient |
| A low-resolution poster | A canonical blocked/refused state, unresolved failed/uncertain operation, or required owner disposition |
| An old poster becomes a signpost | An explicitly applied finding has later current observed passing evidence |
| The exit sign | The named whole goal has explicit Lead closure and its required evidence is still current |

Add `--themed` to `work next` or `work act` to include the same world projection
in that JSON response. The black cat belongs to the actual replay response;
polling `work world` later does not invent another replay. A slice's passing
check or accepted run does not light the exit sign for an unresolved whole
goal. Missing or stale support suppresses the corresponding marker. Ordinary
unadorned Work responses retain their existing shape.

The world renderer writes no authority or evidence. The surrounding Work
status path may refresh its existing replaceable presentation memo; that memo
is outside tested inputs. No provider call or artwork cache is needed.

## Record actual finding application

Reading a diagnosis does not establish reuse. The bound Lead may record a
`reuse` through `work record WORK`, using the current goal and binding revisions
from `work continuation WORK`:

```json
{
  "action": "reuse",
  "expectedRevision": 4,
  "bindingRevision": 1,
  "operationId": "repair",
  "diagnosisRevision": 4,
  "resultEventSha256": "LATER_OBSERVED_CHECK_EVENT_SHA256",
  "conditionsSha256": "CURRENT_CONDITIONS_SHA256"
}
```

The diagnosis must already exist with retained evidence and an observation,
cause or verified-repair class; a hypothesis is insufficient. Its result must
be a later passing observed check in the same Work, with verified artifact
bytes and current acquired configuration, tested inputs and conditions.
Reusing the original check, a synthetic result, missing evidence or stale
conditions is refused. `work continuation WORK --section reuses` separates the
retained relation from its current applicability. Changed inputs, conditions
or unavailable evidence suppress the signpost.

Application is explicitly `lead_reported_application_with_observed_check`:
the later check supports the application claim, but does not independently
prove the semantic cause. These features provide local behavioral evidence;
they do not establish comparative superiority or external-user adoption.

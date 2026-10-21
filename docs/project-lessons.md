# Scoped project lessons

A reviewed project fact can follow a new Work without repeating its discovery.
Opt-in role memory remains the owner: no conversation archive or second store
is created. With an applicable lesson, ordinary `work detail WORK --json`
includes `recipientContext.projectLessons`, and `work detail WORK --view action`
prints the lessons selected into the recipient guidance (text and `--json`
`guidance`) with their read route; native assignments also receive the
current selected source. No separate memory lookup is required to start.

Use a lesson for a durable, non-obvious project coupling or convention. Resolve
executables and resources live. Keep current assignments and result evidence in
Work. A lesson grants no permission and carries no old check or acceptance.

## Author and retire through the existing lifecycle

The owner opts in by including `project-lessons.v1` in `memory.protocolScopes`
and granting the configured Lead the corresponding `memoryWrite`,
`memoryReview`, `memoryPromote` and `memoryRevoke` rights. Consumers need
`memoryRead` and compatible `crossContext`. Empty/default memory stays off.
Before reporting memory as active, verify intended selection with
`memory resolve AGENT --task ACTUAL_GOAL --json` and actual current recipient
delivery. Its `selection` diagnostic explains effective scopes without exposing
inaccessible items. `crossContext: none` permits intentional storage without
recall. Check the selected Lead's retirement rights before authoring a lesson.
When activation was requested, an empty required selection is unfinished even
when resolve succeeds. Agents carry out an already authorized correction and
the delivery checks; they do not leave that selected work as a new owner task.
Only the configured Lead can change a durable lesson, even if another role has
generic memory mutation rights. The Lead still has to hold each configured right.

Write an immutable JSON source inside the project, using the example shape
below. Obtain `projectIdentity` from `project context --json` at
`project.memoryIdentity`; it is derived from the current configured project and
location, without storing a machine path in the lesson. The guard hash is the
SHA-256 of the current guarded file. Revision fields accept exact Git SHA or
SHA-256 identifiers. Provenance identifies why the fact was reviewed.

```json
{
  "version": 1,
  "id": "recipient-companion",
  "fact": "Inspect native context delivery when changing the recipient contract.",
  "projectIdentity": "CURRENT_PROJECT_MEMORY_IDENTITY",
  "owner": "lead",
  "provenance": ["Reviewed coupling with regression evidence"],
  "createdRevision": "EXACT_SOURCE_REVISION",
  "revalidatedRevision": "EXACT_REVALIDATED_REVISION",
  "appliesTo": {"agents": ["worker"], "taskTerms": ["recipient"]},
  "guards": [{"path": "src/recipient.rs", "sha256": "CURRENT_FILE_SHA256"}]
}
```

Then use the existing commands with your current configuration:

```sh
exitbind memory propose lead LESSON.json --scope project-lessons.v1 \
  --expires-at FUTURE_RFC3339_TIMESTAMP --ledger MEMORY_ROOT/lesson.jsonl
exitbind memory review lead MEMORY_ROOT/lesson.jsonl
exitbind memory promote lead MEMORY_ROOT/lesson.jsonl
exitbind memory inspect MEMORY_ROOT/lesson.jsonl --json
exitbind memory revoke lead MEMORY_ROOT/lesson.jsonl
```

Lesson mutations add an exact current `nextAction` to their CLI response:
proposal leads to review, review to promotion, and promotion/retirement to
inspection. This response projection is not part of the immutable ledger event;
`memory inspect` retains the exact hashed event. Follow the emitted argv rather
than reconstructing command syntax.

Correction uses a new immutable source and ledger with the same stable `id`.
Revoke the accepted predecessor before promoting its correction. The optional
`supersedes` field names the predecessor's retired memory item ID. An accepted
duplicate is refused. History remains attributable instead of being overwritten.
An expiry is mandatory. Changed/missing guarded files exclude the lesson until
the Lead revalidates it; unsafe paths refuse. A foreign project identity refuses.

## Carry lessons into a new project

A new round set up as a new project starts without the previous round's
lessons. `exitbind setup ... --lessons-from PREVIOUS_CONFIG --apply` reads the
previous project and records each accepted, unexpired lesson in the new
project through the same propose, review and promote lifecycle as its
configured Lead. The new immutable source lives under `.exitbind/lessons/`,
keeps `id`, `createdRevision`, expiry and the other lesson fields, is bound to
the new `projectIdentity`, and names the previous project, item ID and source
SHA-256 in its provenance. Guards follow the usual rule: a guarded file
present with the recorded hash is kept, one absent here but unchanged in the
previous project is copied byte-identical, and one that differs here excludes
the lesson until the Lead revalidates it. Setup adds the `memory` section, the
Lead's lesson rights and `memoryRead` for the recipients. Expired, revoked and
excluded lessons are listed with their reason; without `--apply` the same
lists are previewed and nothing changes. The previous project is not modified.

## Review compatible configuration evolution

`setup --lessons-from` keeps a byte copy of the configuration it stamped the
carried events with, `MEMORY_ROOT/stamped-config-<first 16 hex of its sha256>.json`
beside the lesson ledgers. When the configuration changes afterwards, the
refusal `configuration changed since memory event` names that copy and the
exact `memory revalidate … --from-config COPY --reason REASON --apply`
command, with the bound and current sha256, the same in text and `--json`.
Otherwise save the original authorized configuration before changing
recipient rights. An old lesson remains pinned to its recorded configuration
until the current Lead explicitly reviews and applies compatible lineage:

```sh
exitbind memory revalidate lead MEMORY_ROOT/lesson.jsonl \
  --from-config ORIGINAL_CONFIG.json --reason "Reviewed additive recipient access"
exitbind memory revalidate lead MEMORY_ROOT/lesson.jsonl \
  --from-config ORIGINAL_CONFIG.json --reason "Reviewed additive recipient access" --apply
```

The first call previews exact configuration hashes and recipient-right changes
without appending an event. Compatibility permits additive `memoryRead` rights,
an agent's read-only `phaseGuidance` (recorded as a difference), schema URL
metadata, and the narrowly reviewed contextual opt-in described below. Ownership, write/runtime/retention semantics,
project roots and existing rights must remain unchanged. The current Lead must
hold the lesson write, review, promote and revoke rights. The prior configuration,
owner profile and immutable source provenance must still be provable; incompatible
changes refuse without an append.

Apply appends a memory-event v2 revalidation to the existing chain and preserves
its lesson identity, lifecycle state, expiry and earlier events. Follow the
returned exact `nextAction.command` to inspect or retire it, then use a new
immutable correction with `supersedes`. Revalidation never refreshes a stale
guard or transfers Work state, checks, review approval, acceptance or task grants.
Older binaries can read unchanged v1 ledgers, but refuse a revalidated v2 ledger;
keep v2 ledgers intact during binary rollback and use the current reader for them.

## Correct an owner-selected recall policy

If an accepted lesson cannot be recalled because its original configuration
disabled selection or omitted the Lead's revoke right, preserve that original
configuration and prepare the owner's intended corrected configuration at the
same project, control and state roots. Leave existing Work pins intact.

```sh
exitbind memory correct-policy MEMORY_ROOT/lesson.jsonl \
  --from-config ORIGINAL_CONFIG.json --config CORRECTED_CONFIG.json \
  --reason "Owner selected this lesson's recall and retirement"
```

This read-only preview returns the exact old/new configuration hashes, item,
predecessor head and policy differences. Save its `ownerDecision` only after the
project owner selected those differences, record `approved: true`, then follow
the returned command with that decision file in `OWNER_DECISION`. No new owner
confirmation is needed when the existing decision already covers this scope.
The host controls who may author configuration and approval files; this is an
explicit audited administrative transition, not human authentication or
compatible revalidation by the old ineligible agent.

Use the corrected configuration for subsequent tasks and their emitted routes;
an invocation that still selects the original store-only configuration retains
its original policy. Preserve that original file for historical Work readers.
After apply, verify the original item in each intended recipient's relevant
context and its complete source, then verify unrelated-task exclusion. The
response's activation status remains `requires_recipient_delivery_check` until
those operational checks establish the requested result. Recording an event
alone does not establish delivery.

Correction permits only additive project-lesson read access, selection from
`none` to `protocol-only` for readers limited to that scope, and the same Lead's
lesson revoke right. Unrelated policy changes, changed roots/ownership, stale
configuration or predecessor head, changed source/profile and unapproved
decisions refuse before append. Source, identity, expiry and all predecessor
events remain. Changed guards stay excluded. An exact repeated apply at the
same current head returns `existing_verified` without another event; after any
later transition it refuses, so an old decision cannot revive a retired item.

The new correction event uses memory format v3. Older readers refuse it without
changing the ledger. Keep a compatible reader for corrected memory when rolling
back an executable. Unchanged v1 and compatible v2 memory remain readable.
This transition transfers no Work, grant, check, review or acceptance.

## Selection and bounds

Without `memory.contextual`, agent names are explicit and selection retains its
literal rule: an empty `taskTerms` list applies to all tasks for those agents;
otherwise at least one case-insensitive literal substring must match the current
Work goal. This is a declared applicability rule, not semantic inference.
Session context without a task excludes task-specific lessons.

Each record is at most 2,048 bytes. Normal readable delivery projects the complete
fact, stable ID and owner with its current source reference; authoring metadata
stays in the immutable source reached through that reference. Delivery includes
at most four facts and 6,144 bytes including metadata and the inspection route. An
explicit `omittedCount` and exact read-only command expose the remaining eligible
references; no omitted fact is silently treated as read. The existing overall
memory policy budget still applies. `memory resolve AGENT --task CURRENT_GOAL
--json` offers task-specific inspection.
The normal project entry supports `project context --task CURRENT_GOAL --json`
and `project context memory ITEM_ID --task CURRENT_GOAL --json` for the Lead's
eligible references and complete current source. Without a task, task-specific
lessons remain excluded instead of being injected into an unrelated session.

Adding, correcting, revoking or invalidating selected memory changes the current
Work's inputs. Existing running evidence may become stale and must be
reacquired through the supported Work path. New Work does not inherit an earlier
semantic acceptance. These controls validate stored data and delivery, not the
truth of a model's inferred lesson or universal host compliance.

Guard observation hashes two descriptor reads with a fixed 32 KiB buffer and
checks descriptor metadata and the reopened path identity. Missing or changed
guards exclude or refuse; symlink and nonregular paths refuse. Guard heap storage
does not grow with file size, while I/O still scales with the guarded bytes.
These observations detect the exercised drift; they do not lock a file against
changes after observation.

## Prepare a bounded source and correction

The input-preparation command below is included in the 0.31.14 release.
Existing proposal and correction rights remain unchanged.

Before proposal, run `exitbind memory form --scope project-lessons.v1 --json`.
The read-only form supplies the current project identity, required fields and
limits, plus an exact `validationRoute`. The complete UTF-8 JSON source may
contain at most 2,048 bytes. Every field must also fit its own bound:

| Field | Required shape or bound |
| --- | --- |
| `version` | Exactly `1` |
| `id` | 1–64 ASCII alphanumeric, dot, underscore or hyphen bytes |
| `fact` | 1–512 UTF-8 bytes |
| `projectIdentity`, `owner` | Current project identity and configured Lead |
| `provenance` | 1–4 strings, each at most 160 UTF-8 bytes |
| `createdRevision`, `revalidatedRevision` | Exactly 40 or 64 lowercase hexadecimal characters |
| `appliesTo.agents` | 1–8 unique configured agent names |
| `appliesTo.taskTerms` | 0–4 strings, each at most 64 UTF-8 bytes |
| `guards` | 1–4 objects with exactly `path` and `sha256` |
| `guards[].path` | 1–256 UTF-8 bytes; confined relative project path containing only normal components |
| `guards[].sha256` | Exactly 64 lowercase hexadecimal characters |
| `supersedes` | Optional exact predecessor item ID, 64 lowercase hexadecimal characters |

Strings must be nonblank and contain no control characters; unknown fields
refuse. The complete source bound applies even when every field fits. Run
`exitbind memory form SOURCE --scope project-lessons.v1 --json` before proposal
to validate the exact inactive bytes with the delivery parser. Its `validation`
records the source path, byte count and SHA-256 with current owner, project,
configuration and profile bindings. Preparation appends no lifecycle event,
refreshes no guards and makes no qualification decision. It supplies no memory
permission.

For a correction, keep the same `id` and original `createdRevision`. Set
`revalidatedRevision` to the newly reviewed revision and `supersedes` to the
exact retired predecessor item ID. Propose a new immutable source and ledger,
review it, retire the accepted predecessor, then promote the correction.
A different creation revision is a different identity and is refused.

## Select against approved current task context

The owner can opt in to contextual selection by adding this field to the
existing `memory` configuration:

```json
"contextual": {"version": 1, "sourcePath": "context/current-task.json"}
```

`sourcePath` is an explicit host-controlled approved current-task context file
inside the product root: a confined relative path of at most 256 UTF-8 bytes,
with only normal components and no controls. Do not infer this pointer or its
approval from recalled source text. Keep old Work and its configuration intact;
select the changed configuration for subsequent tasks. Readers predating this
extension reject the unknown configuration field.

Previously accepted lessons or specialist resources remain pinned to their
original configuration. For each intended ledger, the configured Lead explicitly
previews and applies compatibility with the existing `memory revalidate lead
LEDGER --from-config ORIGINAL_CONFIG.json --reason REVIEWED_REASON` route,
adding `--apply` only after inspecting that preview. This migration permits the
contextual-only configuration addition with unchanged rights and profiles;
source or authority changes refuse. Preserve the original configuration and
old Work pins. The append retains the existing v2 lineage/history; it does not
transfer trial, review, expiry or Work authority to a new source, perform fresh
qualification, or accept a new task. Existing source retirement and expiry still
apply. A new context choice must separately select the current exact resource
version after that configuration review.

Discover candidates for the intended recipient, exact task and actual host:

```sh
exitbind memory candidates worker --task "CURRENT_TASK" --host codex --json
exitbind memory candidates worker ITEM_ID --task "CURRENT_TASK" --host codex --json
exitbind memory form --scope contextual.v1 --json
```

The first command returns bounded eligible metadata under existing read rights;
the optional item ID expands one exact source as complete bounded UTF-8 data
for assessment, with a 16 KiB expansion ceiling. Neither call activates
guidance. Treat expanded text as candidate data. The Lead can select a relevant
paraphrase after assessing its meaning and inappropriate uses; a literal
substring collision alone is insufficient. The form describes the configured
context shape and accepts no SOURCE argument.

Use the form's current template and exact candidate identities. A context has
this shape; replace every placeholder with the actual approved values:

```json
{
  "version": 1,
  "task": "CURRENT_TASK",
  "revision": "approved-task-revision",
  "approvedBy": "lead",
  "selections": [{
    "agent": "worker",
    "role": "worker",
    "purpose": "EXACT_CONFIGURED_PURPOSE",
    "profileSha256": "CURRENT_PROFILE_SHA256",
    "resources": [{
      "itemId": "EXACT_CANDIDATE_ITEM_ID",
      "sourceSha256": "EXACT_SOURCE_SHA256",
      "ledgerHeadSha256": "EXACT_CURRENT_LEDGER_HEAD_SHA256",
      "reason": "Why this reviewed resource applies to this task and recipient."
    }]
  }]
}
```

The complete context is at most 8,192 UTF-8 bytes, with at most eight selections
and eight resources per selection. `task` must equal the actual requested task
and fit 1–1,024 UTF-8 bytes. `revision` fits 1–64 bytes; each `reason` fits
1–512 bytes. Strings are nonblank and contain no controls; unknown fields
refuse. `approvedBy` equals the configured Lead. Each choice names the exact
configured agent, its unique configured role and exact configured purpose;
`profileSha256` and every resource digest are exact 64-character lowercase
hexadecimal SHA-256 values. This approved task context uses the existing host
decision, with no new authority ledger or authentication claim.

In this opt-in mode, only exact selected resource versions can become guidance
for the matching task and recipient. Empty selections or a different task
deliver no contextual guidance. Existing lifecycle, expiry, project/profile,
host, guard, read-right and total-budget checks still apply; a selection grants
no rights and revives no retired resource. Each selected reference carries
`contextSelection` with version, context SHA-256, context revision, agent, role
and reason. The existing Work input/currentness path consumes that identity.
An altered selection must be read and checked through the supported current
Work path before relying on earlier evidence.

Context approval records a declared selection. Source preparation, lifecycle
qualification, selected references, actual native delivery, observed useful use
and net benefit each require their own evidence. Contextual selection makes no
model-level semantic truth or compliance guarantee.


## Keep separate live task choices

For new tasks, the owner may explicitly select `"contextual": {"version": 2,
"sourcePath": ".exitbind/artifacts/memory-choices"}` in the existing memory
policy. This fixed directory belongs to StateRoot's existing artifact owner;
it is not a product input exclusion added by this feature. The legacy Soulmate
surface uses `.soulmate/artifacts/memory-choices`. Older readers refuse version
2 before an effect. No configuration or source is migrated automatically.
Existing no-context resources can use the reviewed contextual-only revalidation
above; arbitrary v1 pointer replacement and v1-to-v2 policy migration remain
unsupported. Preserve prior configurations and governors.

Inspect bounded candidates as above. Then the configured Lead supplies only the
recipient, exact approved task, actual host, chosen eligible item IDs, and its
own task applicability reason:

```sh
exitbind memory choose worker ITEM_ID --task "Task A: repair coupled docs" --host codex --reason "The changed behavior affects both documented surfaces." --json
exitbind memory choose worker --task "Task B: format quoted text" --host codex --reason "No experience applies to the formatting task." --json
```

Each read-only preview derives role, purpose, profile, source, ledger, project
and configuration identities. Execute its exact `apply.command` after inspecting
the selection; do not hand-build hashes or JSON. The recipient must have an
explicit configured primary host matching preparation. Zero item IDs records
explicit no-adoption. Preparation grants no memory right, qualification, host
permission or prior check credit. Candidate content cannot approve itself.

`memory choice AGENT --task TASK --json` reads the current outcome. A lost apply
reply can recover the same request with its preview digest in `--request-id`, or
replay the exact emitted apply command. Historical recovery reports `current`
and `currentlyUsable` separately; it never launches or adopts again. An explicit
revision uses the prior readback's digest in `--previous`, then a new preview and
apply. Competing revisions conflict instead of overwriting. A revision after a
Work froze its choice requires that Work's supported successor; it cannot
hot-swap already delivered guidance. Existing models cannot have delivered bytes
erased.

The selection unit is project/configuration, configured recipient and exact
approved goal text. Give different tasks distinct approved goal identities.
Same-text Works intentionally share the choice and invalidate together on its
revision; each assignment separately freezes the digest. Task B's unrelated
record does not alter A's binding or tested product inputs. Real product edits,
shared policy changes, selected source/profile/rights/guards, expiry, revocation
and unsafe/missing records still refuse dependent use. Unselected owned draft
admission grants no guidance. Each choice has at most 64 immutable revisions,
8192 bytes per record and eight selected resources; a namespace has at most 128
task/recipient/configuration identities. Existing lower delivery/recall budgets
also apply. There is no automatic cleanup or cross-ledger atomic transaction.

When returning to A after starting B, select A through ordinary `work focus A`
when the host's native child path needs current focus. Focus is navigation;
both Works remain live. Read A's current complete detail and prepare the fresh
child normally. Route, Work detail, native delivery and checks retain A's exact
choice. An incompatible actual child host refuses before presentation instead
of silently dropping selected guidance. Separate specialists retain separate
rights; private first-pass conclusions are not shared through these records.

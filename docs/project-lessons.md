# Scoped project lessons

A reviewed project fact can follow a new Work without repeating its discovery.
Opt-in role memory remains the owner: no conversation archive or second store
is created. With an applicable lesson, ordinary `work detail WORK --json`
includes `recipientContext.projectLessons`; native assignments also receive
the current selected source. No separate memory lookup is required to start.

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

## Review compatible configuration evolution

Save the original authorized configuration before changing recipient rights.
An old lesson remains pinned to its recorded configuration until the current
Lead explicitly reviews and applies compatible lineage:

```sh
exitbind memory revalidate lead MEMORY_ROOT/lesson.jsonl \
  --from-config ORIGINAL_CONFIG.json --reason "Reviewed additive recipient access"
exitbind memory revalidate lead MEMORY_ROOT/lesson.jsonl \
  --from-config ORIGINAL_CONFIG.json --reason "Reviewed additive recipient access" --apply
```

The first call previews exact configuration hashes and recipient-right changes
without appending an event. Compatibility permits additive `memoryRead` rights
and schema URL metadata only. Ownership, write/runtime/retention semantics,
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

Agent names are explicit. An empty `taskTerms` list applies to all tasks for
those agents; otherwise at least one case-insensitive literal substring must
match the current Work goal. This is a declared applicability rule, not semantic
inference. Session context without a task excludes task-specific lessons.

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

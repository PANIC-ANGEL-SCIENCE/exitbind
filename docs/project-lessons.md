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

Correction uses a new immutable source and ledger with the same stable `id`.
Revoke the accepted predecessor before promoting its correction. The optional
`supersedes` field names the predecessor's retired memory item ID. An accepted
duplicate is refused. History remains attributable instead of being overwritten.
An expiry is mandatory. Changed/missing guarded files exclude the lesson until
the Lead revalidates it; unsafe paths refuse. A foreign project identity refuses.

## Selection and bounds

Agent names are explicit. An empty `taskTerms` list applies to all tasks for
those agents; otherwise at least one case-insensitive literal substring must
match the current Work goal. This is a declared applicability rule, not semantic
inference. Session context without a task excludes task-specific lessons.

Each record is at most 2,048 bytes. Normal readable delivery includes at most
four records and 6,144 bytes including metadata and the inspection route. An
explicit `omittedCount` and exact read-only command expose the remaining eligible
references; no omitted fact is silently treated as read. The existing overall
memory policy budget still applies. `memory resolve AGENT --task CURRENT_GOAL
--json` offers task-specific inspection.

Adding, correcting, revoking or invalidating selected memory changes the current
Work's inputs. Existing running evidence may become stale and must be
reacquired through the supported Work path. New Work does not inherit an earlier
semantic acceptance. These controls validate stored data and delivery, not the
truth of a model's inferred lesson or universal host compliance.

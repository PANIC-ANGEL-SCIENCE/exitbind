# Consume the current command, then recover its result

## Native result text and inspection

Native `summary` and `reason` are single-line text without control characters.
Each is bounded to 8192 UTF-8 bytes; the summary cannot be empty. Keep multiline
evidence in its original artifact, using the required evidence references.
Never rewrite retained evidence to make a result valid.

`work act WORK --inspect --json` separates process exit and observed turn from
the retained result and safe next action. A provisional result has a bounded
`result.validation` diagnosis (`valid`, `invalid`, or `unavailable`) without
revealing its text. Other result states have null validation; null does not
mean valid. A valid result alone grants no retry: follow `nextAction` and the
current assignment. Live or uncertain effects, an advanced assignment, reviewer
ownership, and an already used bounded retry retain their existing refusals.
Preserve original invalid results and use supported readback for a lost reply.

Use the executable, configuration and project returned by the current action.
The configured coordinator owns the task's meaning and final user report. A
pending recipient identifies a particular role and assignment. It does not
change the parent session's identity. `displayName` is an owner-chosen human
label; the agent key, role, native name and rights retain their own meanings.

Capture a command's process status, complete stdout and stderr separately before
decoding. Keep private captures outside the project's tested inputs. A nonzero
status is a failed command. A decoding error after a successful mutation is an
inspection problem: read back the saved Work or result before another action.
Never repeat a successful begin, return, check or acceptance to repair a summary.
If a permit reply is lost, recover it with the exact same request ID.

| Operation | Successful stdout | Options and failure handling |
| --- | --- | --- |
| `version --json` | Flat JSON with version, commit and executable digest | It accepts no configuration option. Compare the digest with trusted executable evidence. |
| `init` | Human-readable setup report | Use `setup --json` for a structured setup preview. Do not parse init prose as JSON or repeat a successful initialization. |
| `work detail WORK --json` | Current recipient, context and action forms | `next` belongs to the next/resume response; detail carries `actionForms`. Use its declared fields. |
| `work permit` | JSON, including `allowed` | Execute the exact emitted argv. This operation does not accept an added `--json`. |
| Memory transitions | JSON event and current `nextAction` | Follow that command; inspection's `--json` option is distinct from transition output. |
| A refused operation | Nonzero status; some operations return a structured refusal on stdout, while argument/load errors use stderr | Preserve both channels and status, then inspect that operation's contract. Inspect effects when the caller cannot determine whether an operation completed. |

JSON values retain their declared types: a boolean is a boolean; an omitted or
null object cannot be iterated. Parse only the response for the specific
operation. There is no universal option suffix or shared next-action shape.

## Read complete instructions once, display only what is needed

A complete current detail supplies the current role profile, rules, task,
evidence and exact actions. Consume all required instructions. Preserve the
complete response through the host's normal protected capture or native
artifact when that capability exists. Display selected fields for a later
inspection instead of printing the same context, escaped evidence and action
forms again. Bound both each displayed item and their combined batch to the
host's output budget. A batch of individually valid responses can still exceed
that budget; serialize those reads or retain them before projecting a summary.

When details are incomplete or the host cannot retain a full response, follow
the exact public section/expansion routes. Section responses state the byte
count, encoding, hash, offset, completeness and next page. Decode and verify all
pages before treating that section as complete. Missing mandatory instructions
stay missing; an output limit never grants permission. Refresh a stale binding
through the supported current route. Do not invent a private launcher or edit
Exitbind source to finish an ordinary project.

If the first response has already been cut and cannot be parsed, use
`work next WORK --json` with the same executable/configuration when the Work
is known, or `work resume --json` when its handle was lost. These are read-only
recovery; do not repeat the original mutation. Follow `current.details` for
assignment, evidence, tasks and **recipient** independently. The recipient
section contains the current role profile, project rules and selected lessons.
Current v2 section routes page 4 KiB of exact content, including profiles too
large for inline detail. Verify every page's binding, offset and final hash.
Historical section routes retain their 24 KiB paging. The host's output limit
must still fit one page's encoded response; Exitbind cannot recover text a host
discarded or change that host's limit.

## Routine Lead decisions

After acquiring the complete current assignment and required instructions, use
`work detail WORK --view action --json` for routine state and action inspection.
Its decision metadata and exact section routes avoid repeating artifacts. It
explicitly marks instructions incomplete: this is navigation, never review or
acceptance evidence. On a changed recipient, binding or instruction/evidence
source, acquire and consume the applicable complete current bytes before the
next dependent effect. At substantive review or acceptance, inspect complete
relevant artifacts and negative cases; earlier acquisition and an approval
label do not substitute for current evidence. Retain full responses through
protected host capture, then display the needed fields and one next command.
Without complete capture, use exact expansion pages and verify every required
byte. Start with one complete `--detail` response, then use the action view for
routine navigation and expand changed required sections as needed. Never treat
a truncated model-visible projection as complete delivery.

For a compact completion response, close the whole conversation Goal with
`goal close --direct --goal-id ID --view event --json`. The response carries the
completion event and `presentation.terminal`; direct completion has no `EXIT
READY`. If the close reply is lost, read `goal status --view event --json` once
to recover the same event ID and terminal text without changing the journal.
On a repeated close, the event view is idempotent and omits the terminal.

## Report the requested outcome

Start the human report with the useful result and the checks actually completed,
in the user's language. If a permitted recovery finished the work, explain its
meaningful tool incident briefly after that result. An optional export or
display failure does not reopen completed project work. Preserve that failure
in the technical record. Required review, permission, inputs or uncertain
effects remain concrete unfinished user outcomes. Do not fabricate acceptance
or a whole-goal terminal; forward only the current producer's actual terminal.

## Contributor checks

Discover an unknown path once, then reuse the actual repository and task roots.
Inspect the package's actual targets. List test names before choosing an exact
filter, and use `scripts/assert-test-ran.sh` for a one-test coverage claim.
Exit zero with zero tests is insufficient. Format, type-check and exercise the
changed boundary before the required complete native pipeline.

Run `scripts/ci-local.sh --preflight` with the intended target, fixture and log
roots, and launch its dependent pipeline only when preflight succeeds. A shell
caller can use `preflight-command && dependent-command`; two unrelated tool
calls must likewise inspect the first status before launching the second.
Keep one heavy local pipeline. A held target lock, foreign cache or failed
preflight preserves existing files and stops admission.

Inspect the owned native handle, process group, saved status and boot/process
identity before any retry or lock cleanup. An absent old PID after a restart is
insufficient. Remove only an owned lock whose operation is known to have ended;
retain unknown outcomes and use the Work's supported recovery. A returned
binary alone is not a complete-suite pass. A timeout is neither termination nor
permission to replay. These controls cannot prevent every future caller error
or a host reboot.

## Assistant shipper for an accepted result

Use an already authorized assistant in a fresh context with the same executable,
configuration and StateRoot custody. The assistant receives a shipping ID and the
explicit local target choice; it does not reconstruct the implementation dialogue.
The existing current Work receipt remains the source of acceptance.

```text
exitbind ship prepare WORK --target task-output --apply --config CONFIG
exitbind ship consume ID --target task-output --apply --config CONFIG
exitbind ship status ID --config CONFIG
```

`prepare` derives the packet, operation identity, exact accepted artifact,
check/review/Lead references and local destination automatically. The default
artifact is the first accepted submission; `--artifact ACCEPTED_PATH` selects a
specific accepted submission. No per-candidate JSON or digest wrapper is needed.
Only artifacts up to 256 KiB are supported by this bounded route. The consumer
copies exact bytes into private StateRoot shipping storage. It does not execute
provider commands. The target is a logical local staging name, not a filesystem
path, installation selection or remote grant. A packet alone grants no effect;
consumption requires the matching explicit target and `--apply`.

Assistant procedure: consume the fixed ID once, inspect `ship status` after a lost
reply, and return its safe mechanical report plus your causal assessment. Read
private attempt/result records only when needed. When the destination is absent,
a retry may attempt the local stage at most three times when retained results
show no prior or uncertain effect. An existing matching
artifact is read back without copying again. Changed, unsafe, partial or missing
previously completed output holds rather than overwriting. Complete staging with
missing capture is reported separately. A surviving successful result remains a
delivery witness even if its completion marker or attempt record disappears.
Do not turn missing capture into another delivery.

Use `ship observe ID --apply` to retain a bounded explicit provider or causal
observation on stdin. Fields are `id`, `classification` (`provider`,
`infrastructure`, `product-defect`), `sourceSha256` (the report's source identity),
`provider`, `runId`, `status`, `conclusion` (nullable), `evidence`, and `reason`.
These are labeled imported/caller-reported and never mint acceptance. Only a
completed run can carry a conclusion; success of a read or parser is not a passed
provider run. A `product-defect` observation first retains a source-wide hold
and separate repair request with evidence. Every destination and accepted-artifact
variant of that source remains held, including packets prepared earlier. Initial
packet-only development holds are also read back; history readback fails closed
above 256 identities. It never reopens the accepted Work or
authorizes the repair. No supported command clears that hold.

The status projection omits private arguments, errors and causal text. Private
raw observations and immutable facts stay in StateRoot and are not uploaded.
Local bytes do not establish live shipment, installed-platform verification or
completion of the user's overall delivery. Current source custody is required;
transporting just JSON to an unrelated host does not authenticate acceptance.
Repository-specific CI/release observations remain repository administration.

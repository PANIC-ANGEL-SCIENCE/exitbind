# Consume the current command, then recover its result

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
| A refused operation | Nonzero status and error text on stderr | Preserve stdout, stderr and status. Inspect effects when the caller cannot determine whether an operation completed. |

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

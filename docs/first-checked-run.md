# Complete your first checked run

It verifies what you asked an agent to do and what came back, in the same record.

Follow a change from a completion claim through its check, the applicable
owner-selected review policy, and lead acceptance. When the check fails, keep
the earlier result and continue with a fresh attempt. The
[authority boundary](../REFERENCE.md#authority-boundary)
defines who owns each action; [SECURITY.md](../SECURITY.md) defines what the
local evidence can establish.

In `v0.25.0-rc.5`, new checked runs use run-event format 8. Exitbind binds each submission, check,
review, and acceptance result to the run's exact Accepted Subject and records
the tested-input identity of the product root; a stale subject or changed
tested inputs are refused. Current v8 basis/review-policy records are also bound
to the successor subject. Historical v1–v7 ledgers remain readable under their
original semantics.

## Try the complete example

Use the pinned binary from the README and the matching source checkout
for the script. Running it needs only the binary and standard POSIX shell tools,
with no jq, Python, model, or account:

```sh
EXITBIND_BIN=exitbind ./scripts/demo-checked-work.sh
```

Alternatively, build this checkout with Rust/Cargo and set
`EXITBIND_BIN=target/debug/exitbind`. The older stable 0.12.0 binary lacks the
`--event-id` and `--text` options used by this example; use its JSON workflow
for host-managed tasks or upgrade the binary before trying this script.

The [executable example](../scripts/demo-checked-work.sh) creates its own
temporary project and always removes it on exit. It writes `message.txt` with
`unfinished`, and the frozen shell check requires `ready`. The first actual
check exits nonzero. A scripted reviewer approves anyway; attempted lead
acceptance is refused. The lead requests rework, and another CLI process reads
the assignment and earlier artifact references. The script repairs the input,
submits a fresh worker document, runs the check again, records a separate review
and lead acceptance, and inspects the final ledger.

Look for these lines, with the resumed assignment printed between attempts:

```text
Attempt 1: host check failed; acceptance refused.
Attempt 2: host check passed; separately reviewed and accepted.
Earlier worker artifact retained; new process reconstructed the assignment.
Scripted fixture complete. No model calls or user-success measurements.
```

This is a scripted fixture, not an agent evaluation or external-user study. Its
shell observes actual check exits; the role documents are simulated. It proves
the exercised transition sequence, not that an arbitrary caller reported a
check honestly or that a host conversation survived. The example is gated in
[CI](../.github/workflows/ci.yml).

For the separate installed-binary self-test and an exportable synthetic evidence
bundle, use `exitbind benchmark` and read the
[proof methodology](value-proof-methodology.md).

## Use your own project

Use the installed stable binary for the commands below. If you built from
source instead, add its absolute binary directory to PATH before changing directory:

```sh
exitbind version
printf 'Project directory: '
IFS= read -r project_dir
cd "$project_dir"
```

Choose a small authorized change and the real command your project already uses
to check it. You or your host will execute that command; review it before
entering it. Git must be available when working inside a Git checkout. Use your
normal version-control checkpoint or backup for product changes. Exitbind
records results; it does not undo product edits.

The following uses a new portable project configuration and the starter
`change` workflow (`lead`, `worker`, `reviewer`, then `lead`), which exercises a
review-required path. If already configured, keep that configuration and follow
its assignments and owner-selected review policy instead. For
control/state outside the checkout, follow [local mode](onboarding.md#choose-the-storage-mode).

Raw goals, command strings, result documents, and ledgers are private operational
data. Portable initialization creates `.exitbind/` with a Git ignore rule;
review what you share, since an ignore rule is not access control. Before any
real agent work, review `exitbind.json` and the generated profiles. The starter
has empty observe/write/command declarations: set the exact authorized limits
and the host's own permissions before freezing the run. A goal does not grant
permission. Follow [review the starter setup once](onboarding.md#review-the-starter-setup-once)
for the fields and host mapping needed here; no protocol reference is required
to prepare this first task.

```sh
exitbind init --mode portable
```

After reviewing the configuration, enter the actual goal and check. Continue in
this same shell; `set -e` stops on failed commands, and `set -C` prevents accidental
replacement of the result snapshots written below. Use a fresh ledger name if
`run.jsonl` already exists.

```sh
set -euC
umask 077
printf 'Bounded goal: '
IFS= read -r goal
printf 'Exact project check command: '
IFS= read -r check_command
test -n "$goal"
test -n "$check_command"
ledger=.exitbind/runs/run.jsonl
attempt=1
exitbind brief worker --task "$goal" --config exitbind.json
exitbind run start change --goal "$goal" --check-command "$check_command" \
  --review-policy required --ledger "$ledger" --config exitbind.json
exitbind check --config exitbind.json
exitbind run next "$ledger" --text --config exitbind.json
```

`run start` prints a checked-run notice on stderr while keeping JSON stdout.
This example explicitly selects the review-required policy at entry. An owner
who has selected omission can use `--review-policy omitted` instead and may
revise that choice later while the marked run is running. A `run start` that
omits the option is the historical unmarked path: it retains required-review
semantics and cannot later use `run review-policy`. `check` validates
configuration; the project check runs later. An unchecked run is still
supported by omitting `--check-command`, but it has no check-result requirement
for acceptance.

### 1. Record the lead's scope

The lead reads the pending assignment and writes a real scope document naming
the intended change, allowed work, acceptance criteria, and check. Enter the
path to that completed document. These commands preserve a private snapshot;
they do not invent the document or its decision.

```sh
printf 'Path to the lead scope document: '
IFS= read -r scope_source
cat < "$scope_source" > .exitbind/artifacts/first-scope.md
exitbind run submit lead "$ledger" --outcome scoped \
  --artifact .exitbind/artifacts/first-scope.md --artifact-root state \
  --config exitbind.json
exitbind run next "$ledger" --text --config exitbind.json
```

Have the native host perform the worker's bounded assignment. It must read the
selected profile and relevant upstream evidence and produce a real result
document describing what changed, evidence, and unresolved issues. See
[host handoffs](onboarding.md#keep-the-host-you-already-use); a printed assignment
alone does not execute work.

### 2. Submit the worker result, then run its check

Run this block only when the worker actually reports `completed`. For a blocker,
submit the permitted `blocked` outcome with its real evidence instead and
inspect the run. Keep recorded files unchanged; product source can be repaired
in a later attempt while its earlier result snapshots stay intact.

```sh
printf 'Path to the completed worker result document: '
IFS= read -r worker_source
worker_artifact=".exitbind/artifacts/first-worker-$attempt.md"
cat < "$worker_source" > "$worker_artifact"
worker_event=$(exitbind run submit worker "$ledger" --outcome completed \
  --artifact "$worker_artifact" --artifact-root state --event-id \
  --config exitbind.json)
check_exit=0
sh -c "$check_command" || check_exit=$?
exitbind run record-check "$ledger" --target "$worker_event" \
  --check-command "$check_command" --exit-code "$check_exit" \
  --config exitbind.json
exitbind run status "$ledger" --config exitbind.json
exitbind run next "$ledger" --text --config exitbind.json
```

This historical v3 procedure captures the successful submission's event ID **before** executing
the frozen command. A failed submission stops this sequence. The command text
and `--target` are explicit; Exitbind does not select the latest submission or
execute the command. Every current worker needs its own qualifying report.
Never supply zero when the check failed or could not run.

### 3. Record the review and lead decision

Have the reviewer inspect the assignment, actual change, worker result, and
check evidence. Enter the path to its real review and its actual outcome
(`approved`, `rework`, or `blocked` for this role):

```sh
printf 'Path to the reviewer document: '
IFS= read -r review_source
printf 'Reviewer outcome: '
IFS= read -r review_outcome
review_artifact=".exitbind/artifacts/first-review-$attempt.md"
cat < "$review_source" > "$review_artifact"
exitbind run submit reviewer "$ledger" --outcome "$review_outcome" \
  --artifact "$review_artifact" --artifact-root state --config exitbind.json
exitbind run next "$ledger" --text --config exitbind.json
```

Follow the returned assignment. Reviewer rework returns to a fresh worker
attempt; increment `attempt` and repeat step 2 after the authorized repair.
Reviewer approval moves to the lead. A failed or missing check still prevents
acceptance even if the reviewer approved. Only proceed with the following block
when the lead is pending and has written its actual decision document. Enter
`accepted`, `rework`, `rejected`, or `blocked` as that decision requires:

```sh
printf 'Path to the lead decision document: '
IFS= read -r decision_source
printf 'Lead outcome: '
IFS= read -r lead_outcome
decision_artifact=".exitbind/artifacts/first-decision-$attempt.md"
cat < "$decision_source" > "$decision_artifact"
if exitbind run submit lead "$ledger" --outcome "$lead_outcome" \
  --artifact "$decision_artifact" --artifact-root state --config exitbind.json
then
  exitbind run status "$ledger" --config exitbind.json
else
  printf 'Decision refused; inspect the run and resolve the recorded cause.\n' >&2
  exitbind run status "$ledger" --config exitbind.json
fi
```

A refusal is not acceptance. If the lead remains pending after a failed-check
refusal, it can write a fresh rework document and explicitly request another
attempt:

```sh
printf 'Path to the lead rework document: '
IFS= read -r rework_source
rework_artifact=".exitbind/artifacts/first-rework-$attempt.md"
cat < "$rework_source" > "$rework_artifact"
exitbind run submit lead "$ledger" --outcome rework \
  --artifact "$rework_artifact" --artifact-root state --config exitbind.json
attempt=$((attempt + 1))
exitbind run next "$ledger" --text --config exitbind.json
```

Repeat step 2 after the worker performs the repair, then obtain a fresh review
and lead decision in step 3. The new submission needs a new captured event ID
and new check. Do not reuse a prior attempt's check or overwrite its documents.
If the reviewer or lead already submitted `rework` in step 3, skip the
lead-rework block and run `attempt=$((attempt + 1))` before continuing with the
pending worker. Do not increment twice.

### 4. Inspect or resume from another shell

```sh
exitbind run status .exitbind/runs/run.jsonl --config exitbind.json
exitbind run next .exitbind/runs/run.jsonl --text --config exitbind.json
exitbind run inspect .exitbind/runs/run.jsonl --config exitbind.json
exitbind run report .exitbind/runs/run.jsonl --config exitbind.json
```

Run these with the same compatible binary in the project directory. `next
--text` reconstructs the recorded goal, pending role, stage/attempt, check
policy, result location, and earlier/current artifact references. Read the
referenced files for their contents. For a terminal run it reports the actual
state and no pending assignment. It does not restore shell variables, spawn an
agent, or recover a host conversation. In a new shell, recover the exact frozen
check and current attempt from this view before continuing; never guess them.
Hosts needing the structured packet keep using default `run next` or `--json`.

Success is an `accepted` run with the current worker's qualifying check and
separate review/lead documents; the earlier attempt remains inspectable.
`inspect` validates recorded history and does not establish current disk parity.
Artifact drift prevents further transitions, so preserve the exact legitimate
recorded bytes. Intentional governing-input changes need explicit
[supersession](../REFERENCE.md#run-and-recovery) where permitted; accepted and
rejected predecessors remain final.

For the `v0.25.0-rc.5` release candidate, `run observe-check` can execute only the frozen
command locally while v3 ledgers remain report-only. After updating the binary, refresh owned project skills with `exitbind init
--refresh-skills --root .` and reload the host. Keep a binary compatible with
run-event format 3 for checked-ledger rollback. Remove optional hooks before
removing the binary; [removal](../REFERENCE.md#removal) leaves local records and
product changes under your control.

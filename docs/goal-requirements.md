# Complete a goal with several requirements

Give your lead the goal in your usual conversation. The lead keeps the agreed
source and each accepted meaning, then assigns useful work. One Work can cover
several requirements, and several Works can contribute to the same requirement.
A mapping records an assignment; it becomes current support only after actual
checks, the selected review and Lead acceptance. This path is included in the
stable v0.30.1 installation.

## Inspect and continue

Use `exitbind goal status --json` in the configured project. Its `requirements`
view shows the exact goal revision, source references, requirement IDs/revisions,
active mappings, supporting subject, check/event identities, observed acquisition,
acceptance and the next available action. States distinguish `unmapped`, `pending`,
`failed`, `stale`, `unknown` and `current`. Historical closure and current readiness
are separate. A fresh CLI process can inspect this same saved state; it does not
renew a grant or change a Work's review choice.

For direct operation, retain an agreed request in a regular project file and
incorporate its goal through the existing entry. For example:

```sh
exitbind goal incorporate --goal-id import --goal 'Import names safely and show a count' \
  --none-applicable findings,blockers,decisions,externalActions
exitbind goal require --goal-id import --requirement R1 \
  --obligation 'Reject invalid input before persistence.' --artifact request.txt
exitbind goal require --goal-id import --requirement R2 \
  --obligation 'Persist each valid name only once.' --artifact request.txt
exitbind goal require --goal-id import --requirement R3 \
  --obligation 'Show the number of distinct imported names.' --artifact request.txt
exitbind goal cover --goal-id import
exitbind goal status --json
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
  --review-policy required --detail
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
  --check-command 'sh check.sh all' --scope integration --review-policy required --detail
```

After its actual checks, selected review and Lead acceptance, use the returned
Work identifier as `NEW_ACCEPTED_WORK` below. When it replaces earlier validation
contributors, make that decision explicit:

```sh
exitbind goal assign --goal-id import --requirement R1,R2,R3 \
  --result-ref NEW_ACCEPTED_WORK --disposition replace
exitbind goal close --goal-id import --result-ref NEW_ACCEPTED_WORK
exitbind goal status --json
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
check command. This relationship and checked evidence are observable; whether
that checker adequately tests the requirement remains a review judgment.
Project identity includes the canonical root path: recovery in the same project
is supported, while moving this goal to another path requires new validated
work. This is not a cross-path portability claim.

## Reproduce the process scenario

From this source checkout, run:

```sh
cargo test --locked --test goal_requirements \
  three_requirements_multiple_works_interruption_drift_composition_and_closure -- --exact --nocapture
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

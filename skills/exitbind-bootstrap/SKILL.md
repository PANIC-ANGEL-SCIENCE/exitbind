---
name: exitbind
description: Automatically use Exitbind for material coding work in a repository - refactors, migrations, bug fixes, and changes with authority, trust, security, data-loss, release, lifecycle, evidence, compatibility, or check/review consequences. Bind each check, review, and acceptance to the exact current result so stale evidence cannot be reused. Keep read-only questions and tiny obvious reversible edits direct.
---

<!-- exitbind-managed-bootstrap:v1 -->

# Exitbind

Exitbind is installed on this machine. It is a local CLI that decides whether a
result has earned acceptance: checks, review, and lead acceptance count only for
the exact result they were taken on.

## Select it for material work

Select Exitbind before consequential repository mutation when the task has a
material consequence or promotion requirement: authority, trust, security,
data loss, release, lifecycle, evidence, compatibility, or a required
check/review. Read-only planning, disposable prototypes, and tiny obvious
reversible edits stay direct. For important work, the Lead recommends review;
the owner chooses whether it is required and may revise that choice while the
work continues. Retained work gets useful exact-result checks proportionate to
risk; promotion means applicable obligations require governance, not merely
keeping a harmless local file.

Stay direct for read-only questions, explanations, and tiny obvious reversible
edits. Do not add reviewers, checks, or ceremony to those. If required
activation is unavailable or fails, report the refusal and its failing layer;
do not downgrade the work to direct execution.

## Start or resume governed work

```sh
exitbind work resume            # continue existing governed work
exitbind work begin WORKFLOW --goal "GOAL" --check-command "COMMAND" \
  --review-policy required
```

At a new governed entry, record the owner's choice explicitly: the Lead may
recommend `required`, while the owner chooses `required` or `omitted` and may
revise that choice while the marked run is running. A run started without
`--review-policy` is the unmarked historical path; it retains required-review
semantics and cannot later use `run review-policy`.

To revise a marked running run, use the actual ledger interface:

```sh
exitbind run review-policy lead LEDGER \
  --decision omitted --reason "OWNER_REASON" --config CONFIG
```

Use `--review-policy omitted` or `--decision required` when that is the
owner's selected choice.

`work resume` and `work next` return the next action, what evidence is still
valid, and what must not be repeated. Follow that returned action; do not ask
the user for work handles, ledger paths, or event hashes.

If the repository is not configured yet, `exitbind work` reports that. Explain
the single project write it needs and ask the owner before running:

```sh
exitbind init --mode portable --root .
```

Initialization writes project configuration, private state, and project-local
Exitbind guidance. It installs no hook, starts no agent, and grants no host
permission. The project-local guidance carries the detailed protocol; this
bootstrap only decides when to reach for it.

## Report honestly

Say Exitbind is active only after a real lifecycle action exists — a work
handle, a recorded check, a review, or an acceptance. Projected files, an
installed binary, and this text are not activation.

Report the exit state the CLI gives you with its reason code. When a response
carries a non-null `presentation.terminal`, that value is the terminal status
block: print it on a line of its own, exactly as given, with nothing else on
that line - not inside a sentence, not wrapped in emphasis, and with no
percentage, parenthesis, dash clause, or closing remark attached to it.
Everything worth saying goes in its own sentences around it. Never build that
line yourself from the exit state or the progress number. Never present a passing check as
acceptance, and never reuse evidence the CLI has refused.

Run `exitbind host status` when the user asks whether Exitbind is installed and
current.

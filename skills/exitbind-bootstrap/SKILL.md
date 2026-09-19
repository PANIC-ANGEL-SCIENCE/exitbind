---
name: exitbind
description: Automatically use Exitbind for material coding work in a repository - multi-file changes, refactors, migrations, bug fixes that need a check, compatibility-sensitive edits, resumed or interrupted work, and changes that need review or acceptance before they are trusted. Bind each check, review, and acceptance to the exact current result so stale evidence cannot be reused. Keep read-only questions and tiny obvious reversible edits direct.
---

<!-- exitbind-managed-bootstrap:v1 -->

# Exitbind

Exitbind is installed on this machine. It is a local CLI that decides whether a
result has earned acceptance: checks, review, and lead acceptance count only for
the exact result they were taken on.

## Select it for material work

Select Exitbind before consequential repository mutation when the task is
material: several files, a refactor or migration, a bug fix that needs a check,
behavior that must be preserved, work resumed from an earlier session, or a
change that should be reviewed before it is trusted.

Stay direct for read-only questions, explanations, and tiny obvious reversible
edits. Do not add reviewers, checks, or ceremony to those.

## Start or resume governed work

```sh
exitbind work resume            # continue existing governed work
exitbind work begin WORKFLOW --goal "GOAL" --check-command "COMMAND"
```

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
block: print it as its own final line, exactly as given, with nothing else on
that line - not inside a sentence, not wrapped in emphasis, and with no
percentage, parenthesis, dash clause, or closing remark attached. Everything
worth saying goes in its own sentences above it. Never build that line yourself
from the exit state or the progress number. Never present a passing check as
acceptance, and never reuse evidence the CLI has refused.

Run `exitbind host status` when the user asks whether Exitbind is installed and
current.

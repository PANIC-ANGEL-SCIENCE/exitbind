# Exitbind

**A reported “done” is not an accepted result.**

The code can be current. The tests can have passed. The review can have
approved. All three can still belong to different results.

Exitbind helps a coding agent keep checks, review, and Lead acceptance attached
to the result they actually evaluated. Earlier evidence stays historical when
the result changes. It runs locally in your existing project and agent host.

**No result exits unbound.**

```text
Without Exitbind
  Agent: "Done. Tests pass."
  You merge.
  Later: the tests ran on the agent's previous result.

With Exitbind
  Agent: "Done. Tests pass."
  Exitbind: EXIT BLOCKED (check_missing): no passing check belongs to the current result.
  Only a check, required review, and lead acceptance bound to the current result reach EXIT READY.
```

This source describes `v0.25.1`. Check
[published releases](https://github.com/envy404/exitbind/releases)
for the available version and assets. The local benchmark needs no model;
the optional `activity codex` and `work act` paths launch your installed Codex
CLI. Exitbind runs no daemon or cloud service of its own.

[![Exitbind / Exit](https://github.com/envy404/exitbind/actions/workflows/ci.yml/badge.svg)](https://github.com/envy404/exitbind/actions/workflows/ci.yml)
[![Stable release](https://img.shields.io/github/v/release/envy404/exitbind)](https://github.com/envy404/exitbind/releases/latest)
[![License: MIT](https://img.shields.io/badge/license-MIT-blue.svg)](LICENSE)

## Who it helps and when

Use Exitbind when a coding task needs a recoverable handoff and a decision about
the **current** result. Your existing Codex or Claude lead handles its work
handle and evidence. The host still owns models, permissions, tools, and merges.

| Task | Route | What you get |
| --- | --- | --- |
| Small reversible edit | Keep your usual agent and Git workflow. In an already configured project, `activity codex` can optionally run and privately record one direct Codex task. | The direct record is **unjudged**; it is not governed acceptance. |
| One-off check in your host or CI | Run it there. | Its own exit status. That check creates no Exitbind activity record unless you separately choose an Exitbind command. |
| Material change needing a checked result | Let the lead begin governed Work. For a configured Codex worker or reviewer, `work act` runs the pending assignment. | A result tied to its check, applicable review, and explicit Lead decision. |

For repeated material work, optional project memory can carry accepted rules;
each new Work still needs its own evidence.

## Give your lead one link

Paste this into the Codex or Claude conversation you already use, then describe
your task in ordinary language:

```text
https://github.com/envy404/exitbind
```

Your lead can inspect the project and classify the task. On a URL-only first
contact, it asks before installation, project writes, or permission changes.
You do not need to pass work handles, hashes, or ledger paths between turns.
A pasted link is guidance, not proof that a host followed it. See
[onboarding](docs/onboarding.md) for setup and recovery.

For a model-free look at the exact-result rule, run this **after installing**:

```sh
exitbind benchmark
```

It creates a disposable Git project, shows a failed exact check refused, and
lets only a fresh checked result reach acceptance. It does not run an agent or
touch your project. `exitbind benchmark --output NEW_DIRECTORY` keeps
inspectable records; see the [proof method](docs/value-proof-methodology.md).

## Two Codex paths in v0.25.1

Both paths require an initialized Exitbind project and an available Codex CLI
with its own model access. The lead handles setup and the current assignment.

- **Direct small task:** `exitbind activity codex < TASK_FILE` runs one Codex
  task and privately records command outcomes and a bounded changed-file
  reference. Its result remains `unjudged`; there is no Work review or Lead
  acceptance. The default Codex sandbox for this command is `workspace-write`.
- **Governed Codex task:** `exitbind work act WORK` runs **one current** worker
  or reviewer assignment, delivers the verified packet and profile, and
  records its native result. On later calls, `work act` can run the separate
  frozen check step or record a Lead decision with an explicit outcome and
  reason; `work check` is also available for the check. None is inferred from
  worker completion or reviewer approval.
  Worker rework can resume its native session when the recorded state allows it.

`work act` is the product-managed Codex route. Manual child binding and result
returns remain available for host-managed handoffs; see
[onboarding](docs/onboarding.md) and the [command reference](REFERENCE.md#run-and-recovery).

## Check, review, and acceptance stay separate

```text
current worker result -> declared check -> applicable reviewer judgment
                      -> explicit Lead acceptance -> EXIT READY
```

In checked runs, Exitbind refuses acceptance when the configured check result is missing or reports failure for the current worker artifact.

A worker's completion is not a passing check. A passing check is not reviewer
approval, and reviewer approval is not Lead acceptance. A reviewer finding
needs a recorded Lead decision before it can change the task. Exitbind refuses
older evidence when a new worker result replaces it or covered project files
change. CI and branch protection keep their own roles.

Exitbind records the frozen check command, its real exit code or signal and
hashed output logs, a fingerprint including non-ignored uncommitted files, and
separate worker, review, and Lead events. A host-reported check is labelled
`reported`; a check Exitbind ran is `observed`. An omitted review stays an
omission. An accepted checked run can produce a verifiable receipt, which
covers recorded artifacts rather than proving the code correct. See
[receipt limits](REFERENCE.md#exit-path-receipt-and-verification).

## Install and set up

This page describes `v0.25.1` for Linux x86_64
and macOS on Apple Silicon or Intel. The pinned installer places the executable
under `$HOME/.local/bin` and verifies the archive checksum; release archives
also carry GitHub build attestations. Review the command and destination before
approving installation. Before using this command, confirm that the
[`v0.25.1` release](https://github.com/envy404/exitbind/releases/tag/v0.25.1)
has its tag and assets.

```sh
curl -fsSL https://raw.githubusercontent.com/envy404/exitbind/v0.25.1/install.sh | sh
export PATH="$HOME/.local/bin:$PATH"
```

That command installs the binary and, for each detected coding host, a small
bootstrap skill and session hook. It reports what it installed and never
overwrites files it does not manage. `exitbind host status` checks the managed
host setup. Discovery alone does not prove that governed work started.

### Configure a project when needed

After approving project writes, initialize a portable project from its root:

```sh
exitbind init --mode portable --root .
```

This creates `exitbind.json`, private ignored state under `.exitbind/`,
reviewable role profiles, and project-local guidance for Codex and Claude. Add
`--skip-skills` if your host already manages project skills. Setup installs no
hook, launches no agent, and grants no host or OS permission. Codex and Claude
are exercised setup paths; OpenCode's compatible path is experimental. An
optional host plugin carries the skill only—see [setup options](docs/onboarding.md).
Review the generated boundaries and native worker/reviewer mapping before
project work; empty starter boundaries are valid configuration, not permission.
The Lead records the owner's review choice for important work at governed
entry. Missing project access or model credentials remain host problems to
resolve before executing a Codex task.

## Trust, data, and compatibility

Your host owns models, subagents, tools, processes, and permissions; your
repository host owns merges. Exitbind owns its local lifecycle and exit rules.
The [authority boundary](REFERENCE.md#authority-boundary) defines this once.

Exitbind does not authenticate a model's self-report or withstand an attacker who
can rewrite every local file: its ledger is tamper-evident, not tamper-proof.
Outside Git, the input fingerprint covers project files except Git metadata and
Exitbind state. Neither mode covers files outside the project, the environment,
or remote or time-dependent conditions; Git mode also excludes ignored files.
Raw ledgers and artifacts can contain
goals, commands, paths, and task results; keep them private and read
[SECURITY.md](SECURITY.md) before real work.

Historical run records are read under their original producer and schema
meaning, and old evidence is never relabelled as new. Existing projects retain
their historical paths. See the
[public format map](CHANGELOG.md#public-tags-and-format-readers) before a
rollback, and [legacy compatibility](docs/legacy-compatibility.md) for an old
project.

## Update, recover, or leave

Ask the same lead to update Exitbind, resume interrupted work, or remove it.
Direct operators can use:

```sh
exitbind update
exitbind work resume
exitbind version --json
```

`version --json` reports the running executable's SHA-256 and, for release
builds, the embedded build commit, for comparison with release evidence.

After updating from an earlier release, run `exitbind host install` once so the
host guidance matches the new binary. From this release on, `exitbind update`
uses the new binary to refresh that guidance. Before removing the binary,
remove optional hooks and review which project records to retain.
`rm "$HOME/.local/bin/exitbind"` does not delete project ledgers, receipts,
configuration, or projected skills; see [update and removal](REFERENCE.md#removal)
and [run recovery](docs/repair-a-run.md).

## Read next

- [Ask Codex or Claude to set it up](docs/onboarding.md)
- [First checked run](docs/first-checked-run.md)
- [Work mutation results](docs/work-mutation-results.md)
- [Repair or resume a run](docs/repair-a-run.md)
- [Continue one work item across hosts](docs/cross-host-continuation.md)
- [Command reference](REFERENCE.md)
- [Terminology](docs/glossary.md)
- [Optional memory, hooks, and receipts](docs/optional-surfaces.md)
- [Legacy compatibility](docs/legacy-compatibility.md)
- [Contributing](CONTRIBUTING.md) · [Security](SECURITY.md) · [License](LICENSE)

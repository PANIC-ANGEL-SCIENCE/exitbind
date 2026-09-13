# Exitbind

**A reported “done” is not an accepted result.**

**Exitbind — No result exits unbound.** Bind the evidence to the exact result
before exit.

Exitbind is a local CLI plus project-local instructions for the Codex or Claude
lead you already use. For material work it binds the current result, a
deterministic check, independent review, lead acceptance, and a verifiable
receipt to one exact subject. Missing, failed, or stale evidence keeps that
result from exiting as accepted. That bind records acceptance evidence; it does
not make Exitbind a correctness oracle.

Current stable release: `v0.17.0`. Exitbind is not a model, agent runtime,
daemon, cloud service, or OS sandbox.

[![Exitbind / Exit](https://github.com/envy404/exitbind/actions/workflows/ci.yml/badge.svg)](https://github.com/envy404/exitbind/actions/workflows/ci.yml)
[![Stable release](https://img.shields.io/github/v/release/envy404/exitbind)](https://github.com/envy404/exitbind/releases/latest)
[![License: MIT](https://img.shields.io/badge/license-MIT-blue.svg)](LICENSE)

## Give your lead one link

Paste only this into your existing Codex or Claude conversation:

```text
https://github.com/envy404/exitbind
```

That URL-only path is the primary onboarding path. You do not need to learn
Exitbind commands, roles, handles, ledgers, or receipt mechanics.

If you are the lead receiving that URL: inspect this repository, decide whether
the governed path fits the user's work, and explain the concrete effect before
asking for any installation, project write, or permission change. After the
user authorizes those actions, set up and operate Exitbind inside the existing
conversation. Use it selectively for material work; keep tiny reversible work
direct. Do not make the user translate their request into Exitbind vocabulary.
A pasted URL is host-facing guidance, not proof that every agent will inspect
or follow it.

## See a false “done” refused

The URL path above does not require installing the CLI. If you want a local,
model-free proof of the exit rule, `exitbind benchmark` creates a disposable Git
project, shows a failed check refusing acceptance, preserves that failed
evidence, and then shows a fresh checked result reaching acceptance.

This page describes `v0.17.0` for Linux x86_64 and macOS on Apple Silicon
or Intel. The pinned installer places the executable under `$HOME/.local/bin`
and verifies the archive checksum. Review the command and destination before
approving installation.

```sh
curl -fsSL https://raw.githubusercontent.com/envy404/exitbind/v0.17.0/install.sh | sh
export PATH="$HOME/.local/bin:$PATH"
```

Run the setup-free, model-free proof from any directory:

```sh
exitbind benchmark
```

The benchmark proves only the observed synthetic local refusal-and-recovery
path; it is not a claim about every agent, project, or quality outcome.

In checked runs, Exitbind refuses acceptance when the configured check result is missing or reports failure for the current worker artifact.

## How the exit path works

```text
normal request
  -> the lead classifies the task once
  -> small and reversible: work directly
  -> material: bind the requirements and exact result
       -> implementation complete
       -> exact-result check passed
       -> independent review complete
       -> lead accepted
       -> EXIT READY, EXIT REFUSED, or EXIT BLOCKED
```

The states remain deliberately separate. Worker completion is not a passing
check. A passing check is not reviewer approval. Reviewer approval is not lead
acceptance. Evidence for one subject cannot silently authorize a changed
subject.

An accepted v5 checked run can then emit a verifiable receipt that snapshots
the accepted subject. Receipt verification detects covered drift; it does not
prove that the work is correct or replace the exit states above.

For material work, the optional default narration stays small:

```text
Neuro
Exitbind progress: N%.
```

Exitbind—not the character—computes weighted progress toward `EXIT READY`.
Neuro never replaces the host agent's native identity or authority. Character
presentation is removable without changing system semantics.

## What Exitbind refuses

The setup-free benchmark demonstrates a failed exact check refusing acceptance.
In checked runs, missing, failed, or wrong-result evidence also keeps the result
from reaching `EXIT READY`.

A refusal names the exact technical reason. Character language never replaces
the subject, evidence, transition, or mismatch details.

## Manual project setup

After approving project writes, initialize a portable project from its root:

```sh
exitbind init --mode portable --root .
```

New projects receive `exitbind.json`, private ignored state under `.exitbind/`,
reviewable role profiles, and project-local guidance for Codex and Claude.
Setup installs no hook, launches no agent, and grants no host or OS permission.
Review the generated task boundaries and native worker/reviewer mapping before
project-scoped work; empty starter boundaries are valid configuration, not
permission.

`exitbind work begin` creates a checked run and returns the next validated
action. `work return`, `work check`, and `work resume` keep routine artifact and
target bookkeeping out of the conversation while preserving it in the local
append-only record. Low-level `run` commands remain available for
interoperability and inspection.

Codex and Claude are exercised setup paths. OpenCode uses the compatible
`.agents/skills/exitbind/` projection path, but host execution is experimental
until it is tested directly. Other hosts retain their own discovery, consent,
and permission contracts.

## Optional repository plugin

If you prefer an explicit host plugin, add this GitHub repository as a
marketplace and install `exitbind`. This is a GitHub-repository catalog:

```sh
# Codex
codex plugin marketplace add envy404/exitbind --ref v0.17.0
codex plugin add exitbind@exitbind-plugins

# Claude Code
claude plugin marketplace add envy404/exitbind
claude plugin install exitbind@exitbind-plugins
```

The Codex command pins `v0.17.0`; Claude Code follows the repository's current
default branch. The plugin contains the Exitbind skill only. It installs no
CLI, hook, MCP server, app, or model; CLI installation and project writes remain
separate consent decisions.

## Trust and authority boundaries

Your existing host owns model execution, native subagents, tools, processes,
permissions, and any remote mutation. Exitbind owns its local lifecycle and
exact-subject exit rules. GitHub or another repository host remains merge
authority.

Exitbind records requested configuration, selected profile bytes, artifacts,
transitions, and declared or observed check evidence. It does not authenticate
a model's self-report, inspect hidden reasoning, provide an OS sandbox, or
withstand an attacker who can rewrite every local file. Raw ledgers and
artifacts can contain goals, commands, paths, and task results; keep them
private and read [SECURITY.md](SECURITY.md) before real work.

## Compatibility with Soulmate projects

Exitbind continues the existing version and repository history; the rename does
not reset the project to `0.0.1`. `v0.17.0` reads historical Soulmate v1–v4
run records under their original producer and schema meaning. Existing
`soulmate.json`, `.soulmate/` state, project skill paths, environment controls,
and the `soulmate` command remain bounded compatibility surfaces.

New records use the Exitbind identity and current format. Compatibility never
relabels old evidence as new evidence. Consult the
[public format map](CHANGELOG.md#public-tags-and-format-readers) before choosing
a rollback or release channel.

## Update, recover, or leave

You can ask the same lead to update Exitbind, resume interrupted work, or remove
it; the lead should explain and request any needed write before acting. Direct
operators can use:

```sh
exitbind update
exitbind work resume
```

Removing the default binary with `rm "$HOME/.local/bin/exitbind"` does not
delete project ledgers, receipts, configuration, or projected skills. Remove
optional hooks first and review the exact project paths you want to retain; see
[update and removal](REFERENCE.md#removal) and
[run recovery](docs/repair-a-run.md).

## Read next

- [Ask Codex or Claude to set it up](docs/onboarding.md)
- [First checked run](docs/first-checked-run.md)
- [Repair or resume a run](docs/repair-a-run.md)
- [Command reference](REFERENCE.md)
- [Terminology](docs/glossary.md)
- [Optional memory, hooks, and receipts](docs/optional-surfaces.md)
- [Proof methodology](docs/value-proof-methodology.md)
- [Contributing](CONTRIBUTING.md) · [Security](SECURITY.md) · [License](LICENSE)

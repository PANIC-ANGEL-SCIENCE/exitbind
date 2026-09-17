# Exitbind

**A reported “done” is not an accepted result.**

In agent-assisted coding, finishing work is easy to claim and dangerous to trust.
A test passes before the final edit lands; a review approves a stale file; an agent moves on.
In that liminal space, work exists, but the door is a **false exit**.

Exitbind gives the coding agent you already use a bounded, resumable acceptance
path. Small reversible work stays direct. Material work can earn a verifiable
exit only when required evidence belongs to the exact current result:

```text
one link -> classify -> exact-result check -> independent review -> lead acceptance -> verified receipt
```

## Give your lead one link

Paste this into the Codex or Claude conversation you already use, then describe
the work in ordinary language:

```text
https://github.com/envy404/exitbind
```

This URL-only path uses a local CLI plus project-local guidance:
- **Small and reversible work**: stays direct with zero wrapper overhead or ceremony.
- **Material work**: binds the evidence to the exact result before it can exit as accepted.
- **Resumed work**: carries forward evidence that still belongs to the same accepted subject, so a passed check or approved review is reused instead of re-run. If the subject changes, Exitbind asks for fresh evidence instead of recycling the old win.

Your lead should inspect and explain the effect first, then ask before
installation, project writes, or permission changes. A pasted URL is guidance,
not proof that a host followed it. [See the complete onboarding
path.](docs/onboarding.md)

Current stable release: `v0.18.0`.

[![Exitbind / Exit](https://github.com/envy404/exitbind/actions/workflows/ci.yml/badge.svg)](https://github.com/envy404/exitbind/actions/workflows/ci.yml)
[![Stable release](https://img.shields.io/github/v/release/envy404/exitbind)](https://github.com/envy404/exitbind/releases/latest)
[![License: MIT](https://img.shields.io/badge/license-MIT-blue.svg)](LICENSE)

## See a false done refused

The first observable success is the crossing: a failed exact check cannot be
accepted, while a fresh checked result can proceed after review and lead
acceptance. When changes occur after a check, Exitbind detects a **wrong door**—stale
evidence cannot unlock a modified result.

For resumed work, `work resume` projects a deterministic residual packet from
recorded state instead of asking the next agent to trust a chat summary.

The URL-only path above does not require installing the CLI. For a local,
model-free demonstration of refusal and recovery, `exitbind benchmark` creates a
disposable Git project, preserves the failed attempt, and shows the fresh
checked result reaching acceptance.

This page describes `v0.18.0` for Linux x86_64 and macOS on Apple Silicon or
Intel. The pinned installer places the executable under `$HOME/.local/bin` and
verifies the archive checksum. Review the command and destination before
approving installation.

```sh
curl -fsSL https://raw.githubusercontent.com/envy404/exitbind/v0.18.0/install.sh | sh
export PATH="$HOME/.local/bin:$PATH"
```

Then run the local demonstration:

```sh
exitbind benchmark
```

The benchmark is an explicitly local synthetic demonstration of refusal and
recovery. It is not a claim about every agent, project, or quality outcome. Use
`exitbind benchmark --output NEW_DIRECTORY` and the
[proof methodology](docs/value-proof-methodology.md) when you need inspectable
records.

## How the exit path works

For material work, Exitbind binds the current result, deterministic check,
independent review, lead acceptance, and (for a checked v5 run) verifiable
receipt to one exact subject:

```text
normal request
  -> small and reversible: work directly
  -> material: bind the requirements and exact result
       -> implementation complete
       -> exact-result check passed
       -> independent review complete
       -> lead accepted
       -> EXIT READY, EXIT REFUSED, or EXIT BLOCKED
```

These states stay strictly separate:
- Worker completion is not a passing check.
- A passing check is not reviewer approval.
- Reviewer approval is not lead acceptance.

In checked runs, Exitbind refuses acceptance when the configured check result is missing or reports failure for the current worker artifact. If code changes after an approval, the previous evidence is rejected as a wrong door.

Core invariant: **recorded artifact bytes equal disk bytes, or no new run event is written.** A receipt detects covered drift; it does not prove that the work is correct or replace the exit states.

`work next` and `work resume` include a residual packet for the current work:
what is already established, which evidence is still valid, what remains, the
next action, and which valid checks or reviews should not be repeated. A session
restart alone does not erase current evidence; a changed subject requires fresh
evidence.

For material work, the optional default narration stays small:

```text
Neuro
Exitbind progress: N%.
```

Exitbind—not the character—computes weighted progress. Neuro never replaces the
host agent's native identity or authority, and the presentation is removable
without changing system semantics.

## Set up a project when needed

After approving project writes, initialize a portable project from its root:

```sh
exitbind init --mode portable --root .
```

This creates `exitbind.json`, private ignored state under `.exitbind/`,
reviewable role profiles, and project-local guidance for Codex and Claude. Setup
installs no hook, launches no agent, and grants no host or OS permission. Review
the generated boundaries and native worker/reviewer mapping before project work;
empty starter boundaries are valid configuration, not permission. See
[onboarding by environment](docs/onboarding.md) for local mode, host mapping,
and recovery details.

Codex and Claude are exercised setup paths. OpenCode uses the compatible
`.agents/skills/exitbind/` projection path, but host execution is experimental
until tested directly. Other hosts retain their own discovery, consent, and
permission contracts.

## Optional repository plugin

An explicit host plugin is optional and separate from CLI installation and
project writes:

```sh
# Codex
codex plugin marketplace add envy404/exitbind --ref v0.18.0
codex plugin add exitbind@exitbind-plugins

# Claude Code
claude plugin marketplace add envy404/exitbind
claude plugin install exitbind@exitbind-plugins
```

The Codex command pins `v0.18.0`; Claude Code follows the repository's current
default branch. The plugin contains the Exitbind skill only: it installs no CLI,
hook, MCP server, app, or model.

## Trust, data, and compatibility

Your existing host owns model execution, native subagents, tools, processes,
permissions, and remote mutation. Exitbind owns its local lifecycle and
exact-subject exit rules. GitHub or another repository host remains merge
authority. Exitbind is not a model, agent runtime, daemon, cloud service, or OS
sandbox.

Exitbind records requested configuration, selected profile bytes, artifacts,
transitions, and declared or observed check evidence. It does not authenticate
a model's self-report, inspect hidden reasoning, provide an OS sandbox, or
withstand an attacker who can rewrite every local file. Raw ledgers and
artifacts can contain goals, commands, paths, and task results; keep them
private and read [SECURITY.md](SECURITY.md) before real work.

`v0.18.0` continues the existing version and repository history; the rename does
not reset the project to `0.0.1`. It reads historical Soulmate v1–v4 run records
under their original producer and schema meaning. Existing `soulmate.json`,
`.soulmate/` state, project skill paths, environment controls, and the
`soulmate` command remain bounded compatibility surfaces. New records use the
Exitbind identity and current format; compatibility never relabels old evidence
as new evidence. See the [public format map](CHANGELOG.md#public-tags-and-format-readers)
before choosing a rollback or release channel.

## Update, recover, or leave

Ask the same lead to update Exitbind, resume interrupted work, or remove it; the
lead should explain and request any needed write before acting. Direct operators
can use:

```sh
exitbind update
exitbind work resume
```

Removing the default binary with `rm "$HOME/.local/bin/exitbind"` does not delete
project ledgers, receipts, configuration, or projected skills. Remove optional
hooks first and review the exact project paths you want to retain; see
[update and removal](REFERENCE.md#removal) and
[run recovery](docs/repair-a-run.md).

## Read next

- [Ask Codex or Claude to set it up](docs/onboarding.md)
- [First checked run](docs/first-checked-run.md)
- [Repair or resume a run](docs/repair-a-run.md)
- [Command reference](REFERENCE.md)
- [Terminology](docs/glossary.md)
- [Optional memory, hooks, and receipts](docs/optional-surfaces.md)
- [Contributing](CONTRIBUTING.md) · [Security](SECURITY.md) · [License](LICENSE)

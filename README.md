# Exitbind

**A reported “done” is not an accepted result.**

You already merge changes from Codex or Claude. The agent says the work is
finished and the tests pass—and the pass may be real but belong to the agent's
previous result, or the review may have approved a result that was replaced.
That evidence opens a different door.

Exitbind is a local CLI for the agent you already use. A check, a review, and
lead acceptance count only for the exact result they were taken on. **No result
exits unbound.**

```text
Without Exitbind
  Agent: "Done. Tests pass."
  You merge.
  Later: the tests ran on the agent's previous result.

With Exitbind
  Agent: "Done. Tests pass."
  Exitbind: EXIT BLOCKED (check_missing): no passing check belongs to the current result.
  Only a check, review, and lead acceptance bound to the current result reach EXIT READY.
```

Current stable release: `v0.18.0`. One local binary; it calls no model and runs
no daemon or cloud service.

[![Exitbind / Exit](https://github.com/envy404/exitbind/actions/workflows/ci.yml/badge.svg)](https://github.com/envy404/exitbind/actions/workflows/ci.yml)
[![Stable release](https://img.shields.io/github/v/release/envy404/exitbind)](https://github.com/envy404/exitbind/releases/latest)
[![License: MIT](https://img.shields.io/badge/license-MIT-blue.svg)](LICENSE)

## See a wrong door refused

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

It creates a disposable Git project, shows a failed exact check refused, keeps
that failed attempt on record, and lets only a fresh checked result reach
acceptance. No model runs and your project is untouched. It is a synthetic
demonstration of the mechanism, not a claim about every agent, project, or
quality outcome; use `exitbind benchmark --output NEW_DIRECTORY` and the
[proof methodology](docs/value-proof-methodology.md) for inspectable records.

## The wrong door

Material agent work passes through doors before it can leave—implemented,
checked, reviewed, accepted—and each door opens only for the result standing in
front of it. Verify the passage, not just the endpoints: "code exists" and "tests
passed" can both be true while the pass belongs to another version.

A **wrong door** is evidence taken on a different result: a check from before the
last worker result, or a review of a result that was replaced. Evidence is not a
master key, so Exitbind will not reuse it. The CLI does not print the words
"wrong door"; it reports one of three exit states with a precise reason code:

- `EXIT READY`: the current result earned every required step.
- `EXIT REFUSED`: evidence shows it did not, for example `check_failed`.
- `EXIT BLOCKED`: required evidence is missing or unresolved, for example
  `check_missing`, so Exitbind will not guess.

## Give your lead one link

You do not have to operate the protocol. Paste this into the Codex or Claude
conversation you already use, then describe the work in ordinary language:

```text
https://github.com/envy404/exitbind
```

On this URL-only path your lead inspects the project, explains the effect, and
asks before installation, project writes, or permission changes. After you
approve, it uses the local CLI plus project-local guidance:

- **Small and reversible work** stays direct; Exitbind is used selectively.
- **Material work** binds its evidence to the exact result before it can exit.
- **Resumed work** keeps evidence that still belongs to the same result, so a
  new session does not redo a valid check or review. A changed result needs fresh
  evidence.

A pasted URL is guidance, not proof that a host followed it.
[See the complete onboarding path.](docs/onboarding.md)

## How material work exits

```text
one link -> classify -> exact-result check -> independent review -> lead acceptance -> verified receipt
```

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

These doors stay separate:

- Worker completion is not a passing check.
- A passing check is not reviewer approval.
- Reviewer approval is not lead acceptance.

In checked runs, Exitbind refuses acceptance when the configured check result is missing or reports failure for the current worker artifact. It rejects older evidence when a new worker result replaces it. For an accepted checked run it can emit a
verifiable receipt.

Core invariant: **recorded artifact bytes equal disk bytes, or no new run event is written.** A receipt detects covered drift; it does not prove that the work is correct or replace the exit states. It covers recorded artifacts such as worker results, not every project file; see the `v0.18.0` limit under trust below.

**Resume without restarting.** `work next` and `work resume` project a residual
packet from recorded state, not from a chat summary: what is established, which
evidence still opens its door, what remains, the next action, and which checks or
reviews not to repeat. A session restart alone does not erase valid evidence.

For material work, the optional narration stays small:

```text
Neuro
Exitbind progress: N%.
```

Exitbind—not the character—computes weighted progress, and 100% means only
`EXIT READY`. Neuro never replaces the host agent's native identity or
authority, and removing it changes no semantics.

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
exact-result exit rules. GitHub or another repository host remains merge
authority. Exitbind is not a model, agent runtime, daemon, cloud service, or OS
sandbox.

Exitbind records requested configuration, selected profile bytes, artifacts,
transitions, and declared or observed check evidence. It does not authenticate a
model's self-report, inspect hidden reasoning, or withstand an attacker who can
rewrite every local file. In `v0.18.0`, the result is identified by worker
submissions, so an edit to project files made without a new worker submission is
not detected. Raw ledgers and artifacts can contain goals, commands, paths, and
task results; keep them private and read [SECURITY.md](SECURITY.md) before real
work.

Exitbind is the renamed continuation of Soulmate: `v0.18.0` keeps the existing
version and repository history rather than resetting to `0.0.1`. It reads
historical Soulmate v1–v4 run records under their original producer and schema
meaning. The `soulmate` command is the same binary under the previous name, and
existing `soulmate.json`, `.soulmate/` state, project skill paths, and
environment controls remain bounded compatibility surfaces. New records use the
Exitbind identity; compatibility never relabels old evidence as new evidence. See
the [public format map](CHANGELOG.md#public-tags-and-format-readers) before
choosing a rollback or release channel.

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

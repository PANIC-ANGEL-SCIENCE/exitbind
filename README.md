# Exitbind

**Help your coding agent start with the right task, keep useful context, and finish with evidence you can trust.**

Exitbind works inside your existing Codex or Claude conversation. For material
work, it brings the assigned goal, constraints and applicable evidence together,
keeps results and recovery attached to that work, and makes checks, review and
the lead's completion decision traceable to the result they evaluated.

Your lead decides what needs doing. Exitbind handles local records, verified
context delivery and supported state transitions. Your host owns models, tools,
permissions and execution; you still decide what may be installed or merged.
Small reversible edits can keep their usual agent and Git workflow.

[![Exitbind / Exit](https://github.com/envy404/exitbind/actions/workflows/ci.yml/badge.svg)](https://github.com/envy404/exitbind/actions/workflows/ci.yml)
[![Stable release](https://img.shields.io/github/v/release/envy404/exitbind)](https://github.com/envy404/exitbind/releases/latest)
[![License: MIT](https://img.shields.io/badge/license-MIT-blue.svg)](LICENSE)

## Give your lead one link

Paste this into the Codex or Claude conversation you already use, then describe
your task in ordinary language:

```text
https://github.com/envy404/exitbind

Fix the interrupted-import bug, preserve existing imports, and show me the
checked result. Continue the same task if the session is interrupted.
```

On a URL-only first contact, the lead inspects the project and asks before
installation, project writes or permission changes. After authorized setup, it
handles the work records and supported next actions. You do not need to carry
work handles, hashes or ledger paths between turns. A pasted link is guidance,
not proof that a host followed it. See [onboarding](docs/onboarding.md).

A material task follows this path:

```text
Your goal and approved boundaries
  -> current assignment, related outcomes and verified evidence
  -> implementation, saved results and supported return or repair
  -> check on the current result, applicable review, lead decision
  -> accepted result and an inspectable receipt
```

The lead remains responsible for the task's meaning and affected consumers.
Exitbind supplies readable scoped detail and state-specific action forms; it
cannot infer missing requirements or guarantee that the first implementation
is correct. Goal/task status and current-result readiness remain distinct.

## Install and see a first result

This source targets `v0.27.0` for Linux x86_64 and macOS on Apple Silicon or
Intel. Check [published releases](https://github.com/envy404/exitbind/releases)
for the available tag and assets before installing. The pinned installer
verifies the archive checksum; release archives also carry GitHub build
attestations. Review the command and destination before approving installation.

```sh
curl -fsSL https://raw.githubusercontent.com/envy404/exitbind/v0.27.0/install.sh | sh
export PATH="$HOME/.local/bin:$PATH"
```

After installing, run the model-free benchmark:

```sh
exitbind benchmark
```

It creates a disposable Git project and demonstrates a
checked result reaching acceptance. It runs no agent and does not touch your
project. `exitbind benchmark --output NEW_DIRECTORY` keeps inspectable records;
see the [proof method](docs/value-proof-methodology.md).

The installer places the executable under `$HOME/.local/bin` and installs a
small managed bootstrap and session hook for each detected coding host.
`exitbind host status` shows that setup. It never overwrites host files it does
not manage. Discovery does not prove governed work started.

After approving project writes, initialize from the project root:

```sh
exitbind init --mode portable --root .
```

Setup creates `exitbind.json`, private ignored state, reviewable role profiles
and project-local guidance. It launches no agent and grants no host permission.
Review the generated boundaries and native worker/reviewer mapping once;
empty starter boundaries are valid configuration, not access. Use
`--skip-skills` if your host already manages project guidance. Codex and Claude
are exercised setup paths; OpenCode compatibility is experimental.

When the owner already knows the first task's boundaries, assemble those facts
directly without a model-authored JSON join:

```sh
exitbind setup --mode portable --root . --scope worker \
  --observe README.md,src --write src --commands "cargo fmt --check" \
  --check-command "YOUR_TEST_COMMAND" --review-policy required \
  --goal "Describe the bounded change"
```

This previews the affected paths and detected Codex/Claude executables. Add
`--apply` only after reviewing the preview. Setup updates the selected role's
declared facts, verifies compatible managed native projections, and reports an
unchanged repeat without rewriting custom profiles, skills, hooks, or
permissions. It never launches a model or reloads an existing session. Use
`--hosts codex` or `--hosts claude` to select the supported native mapping;
unsupported hosts are refused.

To inspect the exact native role mapping before a governed task, use
`exitbind project agents --json --config exitbind.json`. After explicit project
consent, `--apply` materializes only Exitbind-managed projections; a current
projection is reported as unchanged and custom or unsafe files are left alone.
This route does not fill task boundaries, choose a check, launch a model, or
change host permissions. Keep those owner decisions in the project
configuration and the governed Work start.

## Work and return in the same conversation

For a configured Codex worker or reviewer, the lead can use `work act` to run
one pending assignment with its verified profile and context. Later calls run
the separately declared check or record an explicit lead decision. Saved
native results can return through the same work; replay does not start another
provider when the result is already recorded. An uncertain execution stays
unresolved until its supported inspection/recovery path establishes what
happened. [Continue native work](docs/continuing-native-work.md) explains the
supported route; [cross-host continuation](docs/cross-host-continuation.md)
covers host-managed handoffs.

The optional `activity codex` path runs and privately records one direct small
task. Its result is `unjudged`, with no governed review or acceptance. Both
Codex execution paths need an available Codex CLI with its own model access;
model access and cost belong to that host. Exitbind requires no model, daemon,
cloud service or telemetry for its local record and verification commands.

The optional `work world` view renders static terminal/JSON motifs from work
events, such as a return door, a replaying black cat and an exit sign. These are
status cues with no authority. They are not an animated environment, autonomous
learning or exploration of unsaved conversations.

## Make completion trustworthy

**A reported “done” is not an accepted result.** A worker's completion, a passing
check, reviewer approval and lead acceptance are separate events. Exitbind
refuses acceptance when the configured check result is missing or reports
failure for the current worker artifact. Earlier evidence remains historical
when the result or covered inputs change. The owner chooses whether review is
required; an omission remains an omission.

In checked runs, Exitbind refuses acceptance when the configured check result is missing or reports failure for the current worker artifact.

Exitbind records the frozen command, its actual outcome and hashed logs, a
fingerprint of covered project files, and separate work/review/lead decisions.
A host-reported check is labelled `reported`; one Exitbind ran is `observed`.
An accepted checked run can produce a verifiable receipt covering recorded
artifacts. A receipt does not prove that the code is correct. See
[receipt limits](REFERENCE.md#exit-path-receipt-and-verification).

The host controls native tools and permissions; repository protection retains
its own merge rules. Exitbind is not an OS sandbox. Its ledger is
tamper-evident, not tamper-proof against an actor who can rewrite every local
file. Covered inputs exclude external files, environment and remote conditions;
Git mode also excludes ignored files. Goals, paths, commands and returned
artifacts can contain private data. Keep records private and read
[SECURITY.md](SECURITY.md) and the [authority boundary](REFERENCE.md#authority-boundary).

## Update, recover, or leave

Ask the same lead to update Exitbind, resume interrupted work or remove it.
Direct operators can use:

```sh
exitbind update
exitbind work resume
exitbind version --json
```

`version --json` reports the executable hash and embedded build commit for
comparison with release evidence. The update path refreshes managed host
guidance; `exitbind host install` is the explicit refresh route. Files changing
on disk do not prove that an existing session reloaded them.

Historical records retain their original producer/schema meaning. Existing
projects keep their historical paths. Consult the
[format map](CHANGELOG.md#public-tags-and-format-readers) and
[legacy compatibility](docs/legacy-compatibility.md) before a rollback.

Before removing the binary, remove optional hooks and choose which project
records to retain. `rm "$HOME/.local/bin/exitbind"` leaves project ledgers,
receipts, configuration and projected skills in place. See
[update and removal](REFERENCE.md#removal) and [run recovery](docs/repair-a-run.md).

## Read next

- [Onboarding and setup options](docs/onboarding.md)
- [First checked run](docs/first-checked-run.md)
- [Repair or resume a run](docs/repair-a-run.md)
- [Work mutation results](docs/work-mutation-results.md)
- [Command reference](REFERENCE.md) and [terminology](docs/glossary.md)
- [Optional memory, hooks and receipts](docs/optional-surfaces.md)
- [Contributing](CONTRIBUTING.md) · [Security](SECURITY.md) · [License](LICENSE)

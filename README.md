# Exitbind

**A reported “done” is not an accepted result.**

The code can be current. The tests can have passed. The review can have
approved. All three can still belong to different results.

Exitbind is a local acceptance boundary for coding-agent work. Checks,
independent review, and Lead acceptance count only for the exact result they
were taken on. When that result changes, evidence that no longer belongs to it
stays historical.

**The map can be stale. The room cannot.**

Packets, summaries, progress, and display are derived views. Recorded state and
the current subject identity decide what still holds. A real check or review
from another result remains historical evidence.

**No result exits unbound.**

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

Current stable release: `v0.23.0`. One local binary; it calls no model and runs
no daemon or cloud service.

[![Exitbind / Exit](https://github.com/envy404/exitbind/actions/workflows/ci.yml/badge.svg)](https://github.com/envy404/exitbind/actions/workflows/ci.yml)
[![Stable release](https://img.shields.io/github/v/release/envy404/exitbind)](https://github.com/envy404/exitbind/releases/latest)
[![License: MIT](https://img.shields.io/badge/license-MIT-blue.svg)](LICENSE)

## See a wrong door refused

This page describes `v0.23.0` for Linux x86_64 and macOS on Apple Silicon or
Intel. The pinned installer places the executable under `$HOME/.local/bin` and
verifies the archive checksum. Review the command and destination before
approving installation.

```sh
curl -fsSL https://raw.githubusercontent.com/envy404/exitbind/v0.23.0/install.sh | sh
export PATH="$HOME/.local/bin:$PATH"
```

That one command installs the binary and, for each coding host it detects on
this machine, the managed guidance that lets the lead you already use find
Exitbind: a small bootstrap skill and a session hook. It reports what it
installed and what it could not install, and never overwrites a file it does
not manage. Check it at any time with `exitbind host status`.

Discovery is not activation. An installed bootstrap means your lead can find
Exitbind and is told when to reach for it. Whether it selects Exitbind for a
given task stays the host and model's judgement, and only a work handle,
recorded check, review, or acceptance shows that governed work actually
started.

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
last worker result or before a covered project file changed, or a review of a
result that was replaced. **Evidence is not a master key**, so Exitbind will not
reuse it. The CLI does not print the words
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
- **Material or promotion-required work** binds its evidence to the exact result
  before it can exit. Classification follows consequence and promotion, not
  file count; read-only planning and disposable exploration can stay direct.
- **Resumed work** keeps evidence that still belongs to the same result, so a
  new session does not redo a valid check or review. A changed result needs fresh
  evidence.

A pasted URL is guidance, not proof that a host followed it.
[See the complete onboarding path.](docs/onboarding.md)

## How material work exits

```text
one link -> classify -> exact-result check -> review decision
          -> applicable review -> lead acceptance -> verified receipt
```

```text
normal request
  -> small and reversible: work directly
  -> material: bind the requirements and exact result
       -> implementation complete
       -> exact-result check passed
       -> review decision (Lead recommends; owner decides and may revise)
       -> applicable review complete
       -> lead accepted
       -> EXIT READY, EXIT REFUSED, or EXIT BLOCKED
```

These doors stay separate:

- Worker completion is not a passing check.
- A passing check is not reviewer approval.
- Reviewer approval is not lead acceptance.

For important work, the Lead recommends independent review and the owner decides
whether it is required. The owner can revise that choice while work continues.
An explicit omission is recorded as an omission; it is never called review
approval. When review is required, current independent approval remains
necessary. Checks, preservation obligations, currentness, and Lead acceptance
remain separate gates in either policy.

At a new governed entry, record that owner choice explicitly with
`--review-policy required` or `--review-policy omitted`. To revise a choice on
the marked running run, use the actual run interface, for example:

```sh
exitbind run review-policy lead .exitbind/runs/run.jsonl \
  --decision omitted --reason "Owner selected no independent review" \
  --config exitbind.json
```

A run started without `--review-policy` is the unmarked historical path. It
keeps the historical required-review semantics and cannot later use
`run review-policy`; a new run should choose its policy at entry.

In checked runs, Exitbind refuses acceptance when the configured check result is missing or reports failure for the current worker artifact. It rejects older evidence when a new worker result replaces it or covered project files change. For an accepted checked run it can emit a
verifiable receipt.

Core invariant: **recorded artifact bytes equal disk bytes, or no new run event is written.** A receipt detects covered drift; it does not prove that the work is correct or replace the exit states. It covers recorded artifacts such as worker results, not every project file; see tested-input coverage under trust below.

**Resume without restarting.** `work next` and `work resume` project a residual
packet from recorded state, not from a chat summary: what is established, which
evidence still opens its door, what remains, the next action, and which checks or
reviews not to repeat. A session restart alone does not erase valid evidence.
Before skipping work listed in a saved packet, `work validate WORK --packet FILE`
rechecks it against current state and returns the canonical packet to act on. A
finished run is history: its packet never authorizes skipping new work.

## The Lead sets the Frame

For work where structure matters, the Lead sets the Frame before workers move
in. The Frame fixes the shared decisions that must not be silently
reinterpreted: ownership, interfaces, stable identities, material state
transitions, compatibility constraints, preserved behavior, decisive cases, and
explicit non-goals.

The Frame is not a full implementation plan or a transcript summary. Workers
choose local implementation details inside its open zones. If implementation
exposes a contradiction, the worker returns it to the Lead instead of silently
choosing new meaning. The Frame is current authority, not eternal truth; the
Lead can supersede it with a successor basis when evidence requires one.

Workers share the Frame. The independent reviewer does not owe it agreement and
can challenge the shared assumption, preservation, compatibility, authority,
lifecycle, or evidence. A finding requires causal disposition and renewal of the
affected evidence.

For material work, the optional narration stays small:

```text
[Neuro] Exitbind progress: N%.
```

Exitbind—not the character—computes weighted progress, and an accepted terminal
run supplies the literal `terminal` display value `EXIT READY`. Hosts copy that
value verbatim; they do not reconstruct it from progress or append a suffix.
Alongside it, `work next` and `work resume` return a short line only when the
state actually moved—a check that went stale, evidence that fits again, a
decision that is yours—so the run is legible without a status report every
turn. Neuro never replaces the host agent's native identity or authority, and
removing it changes no semantics.

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
codex plugin marketplace add envy404/exitbind --ref v0.23.0
codex plugin add exitbind@exitbind-plugins

# Claude Code
claude plugin marketplace add envy404/exitbind
claude plugin install exitbind@exitbind-plugins
```

The Codex command pins `v0.23.0`; Claude Code follows the repository's current
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
rewrite every local file. Checks, approvals, and acceptance are bound to the
tested inputs of the project root: tracked and untracked, non-ignored files (or
every file outside Git), excluding `.git` and Exitbind state. Ignored files, files
outside the project, the environment, and remote or time-dependent conditions are
not covered. Raw ledgers and artifacts can contain goals, commands, paths, and
task results; keep them private and read [SECURITY.md](SECURITY.md) before real
work.

A project written by an earlier release keeps working: Exitbind reads historical
run records under their original producer and schema meaning, and compatibility
never relabels old evidence as new evidence. New installs create only Exitbind
paths. See the [public format map](CHANGELOG.md#public-tags-and-format-readers)
before choosing a rollback or release channel, and
[legacy compatibility](docs/legacy-compatibility.md) if you are opening an old
project.

## Update, recover, or leave

Ask the same lead to update Exitbind, resume interrupted work, or remove it; the
lead should explain and request any needed write before acting. Direct operators
can use:

```sh
exitbind update
exitbind work resume
```

Updating *from* an earlier release leaves that release's host guidance in place:
its updater re-synchronises the bridge from its own copy after the new binary is
installed. Run `exitbind host install` once afterwards, or install with the
command above instead of updating. From this release on, the newly installed
binary owns that step and `exitbind update` keeps the guidance current.

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
- [Legacy compatibility](docs/legacy-compatibility.md)
- [Contributing](CONTRIBUTING.md) · [Security](SECURITY.md) · [License](LICENSE)

# Instructions for coding agents contributing to Exitbind

This file is for external coding agents modifying the Exitbind repository. It
is not part of the installed end-user workflow or a maintainer's personal agent
configuration. Human contributors should start with [CONTRIBUTING.md](CONTRIBUTING.md);
if they use a coding agent, these repository-specific rules apply to that agent.
For setup in another project instead of changes here, follow the README's
"Give your lead one link" path and linked onboarding guide.

Read `README.md` before changing the project.

## Purpose

Exitbind is a local acceptance boundary for material coding-agent work. It binds
an exact result to check evidence, independent review, lead acceptance, and a
verifiable receipt. It is not a model runtime or a general autonomous-agent
framework.

## Public surface

When a user gives only `https://github.com/PANIC-ANGEL-SCIENCE/exitbind`, inspect this project and
the host capabilities, decide whether the governed path fits, and ask before
installation, project writes, permission changes, or another owner-controlled
action. After authorization, operate selectively inside the existing
conversation. Text guidance does not prove host or model compliance.

- Open public explanations with the supported work path: applicable goals and
  evidence, useful execution and continuation, then trustworthy completion.
  Explain exact-result acceptance as a trust mechanism within that path.
- Keep the default path legible as `one link -> classify -> exact-result check
  -> review decision -> applicable review -> lead acceptance -> verified
  receipt`; the Lead recommends review, the owner decides and may revise, and
  an omission is never approval. Group commands as operator details without
  hiding limitations.
- New governed entry must record the owner choice with
  `--review-policy required` or `--review-policy omitted`. An unmarked run is
  the historical path with required-review semantics; it cannot later use
  `run review-policy`.
- Every public demo must run in CI and prove only the check it exercises.
  Current releases do not compare all product changes with the declared write
  boundary, so never claim or demonstrate that they do.
- Keep one invariant aligned in README, CLI errors, run transitions, and CI:
  recorded artifact bytes equal disk bytes, or no new run event is written.
- Define each authority boundary once. Elsewhere link to that definition or let
  a focused test carry it; do not restate it for emphasis.
- Public artifacts show one accurate refusal instead of advertising a list of
  absent features or non-guarantees.

## Non-negotiable constraints

- A normal coding-agent session must work when Exitbind is absent or disabled.
- No daemon, cloud service, database, model call, telemetry, or background
  process may become required.
- Advisory use must not wrap or delay normal development commands.
- Strict enforcement must be explicit and limited to boundaries Exitbind
  actually controls. Never imply prompt text is an OS sandbox.
- Persistent memory is opt-in, role-scoped, reviewable, expiring, and revocable.
- Never create shared canonical memory or automatically ingest chat history.
- One lead owns the result. An orchestrator may coordinate only the active task
  and gains no implicit access to agents' role-scoped memories.
- Prefer a host's native subagent and review mechanisms. Do not rebuild model
  loops, provider clients, or process schedulers merely for uniformity.
- Delegation must name the profile, purpose, allowed scope, dependencies,
  verification, and return contract. Profile loading must be evidenced.
- Receipts record selected configuration/profile bytes and requested runtime
  metadata, not observed actions, hidden reasoning, secrets, or unnecessary
  command output.
- Reuse host capabilities and established formats before adding an adapter or
  dependency. dotagents integration must remain optional.

## Complexity ladder

Before adding a feature, stop at the first sufficient option:

1. Do users need it to satisfy a tested workflow?
2. Does Codex, Claude Code, dotagents, Git, or the OS already provide it?
3. Can a documented convention solve it?
4. Can the standard library solve it?
5. Only then add the smallest implementation.

Do not reduce validation at trust boundaries, deletion safety, privacy, or the
small verification needed to prove behavior.

## Change discipline

- Each file must have one explainable responsibility.
- A production source listed in `tests/fixtures/architecture-budget.txt` may
  not grow; any other stays under 32 KiB. Extract the responsibility you are
  changing instead of adding policy to a large facade, reducer, or router.
- Keep the CLI entry point as wiring and dispatch only.
- `config::validate` owns configuration acceptance. New or modified code must
  use `Loaded::agent` and typed fields instead of indexing `Loaded.config`
  directly; retain the raw value and source bytes only for compatibility and
  evidence hashing.
- Prefer plain TOML/JSON/JSONL files over a storage service.
- Add no dependency without documenting why native and standard-library options
  are insufficient.
- Converting a path to a string for a decision, command, identifier, or recorded
  evidence must fail on non-UTF-8 input. Lossy conversion is permitted only in
  human-facing error or display text. Path resolution failures must return an
  error; a missing final leaf may be rendered only after its parent is
  canonicalized.
- Every user-visible command needs one focused test or fixture.
- For the full native CI sequence, use `scripts/ci-local.sh` rather than
  assembling the commands again. See [CONTRIBUTING.md](CONTRIBUTING.md) for
  prerequisites, proof-base selection, and checks that remain host-specific.
- For a historical failed GitHub Actions run, use `scripts/ci-failure.sh RUN_ID`
  to see failed steps and bounded error lines. Keep full remote logs out of
  agent context; record the run ID, head SHA, failing step, and diagnosis.
- For noisy verification in an agent session, use
  `scripts/quiet-command.sh LABEL COMMAND [ARG ...]`: success is one labeled
  line and failure preserves the command's complete output. The wrapper changes
  presentation, never the command or its acceptance evidence.
- Measure startup and command latency before claiming the tool is lightweight.
- Update the plan when scope, authority, or a stop gate changes.
- Raise the minor version only when a product invariant changes or a previously
  accepted counterexample becomes mechanically rejected.
- Run `scripts/review-inventory.sh PREVIOUS_RELEASE` before a repository review.
  A second occurrence of a finding must add the mechanical gate that would have
  caught it; on a third occurrence, failure to adopt the gate is the finding.

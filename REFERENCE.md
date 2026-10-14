# Exitbind command and protocol reference

It verifies what you asked an agent to do and what came back, in the same
record.

Exitbind — No result exits unbound.

Bind the evidence to the exact result before exit.

For material work, supported product and host paths automatically surface a
concise English status derived from the external user's whole goal, the Lead's
divided tasks, and the current result prerequisite state:

```text
Goal: IN PROGRESS | Ship a change | Tasks: 2/3 complete | build=complete, review=complete, delivery=open | Result readiness: IN_PROGRESS
```

The product owns this status and says `unavailable` when the canonical goal or
decomposition is missing. Models do not calculate percentages, remember a
progress command, or render a progress explanation each turn.

Holytail preservation detail is exceptional: surface it only when requested or
when a material preservation failure, regression, unresolved invariant, or
decision-relevant change needs reporting. Exact technical state and refusal
reasons remain available. The packaged reference
`skills/exitbind/references/preservation.md` holds the route and quality
policy, the accepted-meaning and read-back templates, and the evidence labels;
a project needs no separate preservation install.

Exitbind's ledger and benchmark are local and model-free. It records task
envelopes, hashes, run transitions, and memory-lifecycle evidence. The optional
`activity codex`, `work act`, and `away` commands launch an installed Codex CLI;
Codex still owns model access and tool execution. Exitbind is not an operating
system or process sandbox, and host permissions remain with the host.

New setup uses `exitbind`, `exitbind.json`, `.exitbind/`, and `exitbind/`. A
project written by an earlier release keeps working under its own paths and
producer values; those surfaces are listed in
[legacy compatibility](docs/legacy-compatibility.md) and are never created by a
new install.

## Quick start

Install the published `v0.31.14` release as a single Rust binary. Node.js,
npm, Python, and Cargo are not required after installation:

```sh
curl -fsSL https://raw.githubusercontent.com/PANIC-ANGEL-SCIENCE/exitbind/v0.31.14/install.sh | sh
exitbind init --mode portable
exitbind brief worker --task "Describe the change you want to make" --config exitbind.json
exitbind run start change --goal "Describe the bounded change" --check-command "YOUR_TEST_COMMAND" --review-policy required --ledger .exitbind/runs/run.jsonl --config exitbind.json
exitbind check --config exitbind.json
```

Replace `YOUR_TEST_COMMAND` with your actual project check. These commands
prepare the run; the [complete first checked run](docs/first-checked-run.md)
continues through real result documents, check execution, review, and acceptance.
`v0.31.14` includes the `--event-id`/`--text` forms below. Older
0.12.0 binaries retain the JSON workflow but do not recognize these flags.
A skill refresh alone does not upgrade the binary.

v0.31.14 targets Linux x86_64 and native macOS on
Apple Silicon and Intel. Windows uses the Linux artifact through Ubuntu on WSL 2,
with the agent, Exitbind, and project inside that distribution. The
[platform matrix](docs/platform-support.md) names the native build and
installed-path gates; the [Windows guide](docs/windows-wsl.md) explains WSL
setup. Native Windows and other Linux architectures remain unsupported. The
installer rejects unsupported platforms before downloading an archive.

That creates `exitbind.json`, canonical profiles and empty public control
directories under `exitbind/`, and private evidence directories under
`.exitbind/`. It also copies the bundled skill into
`.agents/skills/exitbind/SKILL.md` and
`.claude/skills/exitbind/SKILL.md` as host discovery projections, not as
Exitbind identity. Downloading a release binary never copies those files or
changes global host configuration. The default public surface is four command
families: `init` creates the local contract, `brief` renders an assignment,
`run` records its resumable handoff, and `check` validates the current
configuration. The host still performs the returned assignment.

## Authority boundary

- Exitbind owns task envelopes, selected configuration/profile evidence,
  run-state events, artifact hashes, and explicit memory-lifecycle evidence.
- The native host owns provider authentication, model execution, subagent
  spawning, permissions, and enforcement of the declared boundary.
- The configured lead owns scope decisions and final `accepted` authority;
  reviewer `approved` is role-scoped evidence, not acceptance.
- Complete security limitations and non-guarantees live in
  [SECURITY.md](SECURITY.md).

[Native conversation continuity](docs/native-continuity.md) describes the
root-thread preservation rule, its distinction from durable role memory, and
the limits of the advisory hook and regression evidence.

## First run details

`check` validates the configuration, profiles, and declared boundaries. `brief`
renders one bounded task packet without invoking a provider or launching a
model. The following is an abbreviated rendering; the profile hash is omitted
only to keep the example readable:

```text
# Exitbind task envelope: worker

Display name: worker
Native task name: worker
Purpose: Implement one bounded task without redefining architecture.
Task: Describe the change you want to make
Profile: exitbind/agents/worker.md
Profile SHA-256: ...
Requested runtime: host=none, model=none, reasoning effort=none, fallback=none

## Declared boundary

- Observe: none
- Write: none
- Commands: none
- Skills: none
- Memory read: none
- Memory write: none
- Memory review: none
- Memory promote: none
- Memory reject: none
- Memory revoke: none
- Memory expire: none
- Memory forget: none
- Retention: task
- Cross-context: none

> This is a plan-only brief. Runtime fields are requested bindings, not a model invocation or OS sandbox. The lead remains responsible for scope and final acceptance.
```

Replace `worker` and the task text with the role and work you need. The brief's
empty lists are declarations, not permission enforcement; the full boundary is
in [SECURITY.md](SECURITY.md). The installed Rust binary is the sole language
runtime requirement. Inside a Git worktree, initialization and state mutation
also require `git` on `PATH` for the publication preflight.

For a complete checked task and rework, follow the
[first checked run](docs/first-checked-run.md). For the narrower artifact-drift
exercise, jump to [Evaluation](#evaluation).

For an unchecked handoff after initialization, use a new ledger. This form
has no check-result requirement; use [checked acceptance](#checked-acceptance)
when project checks must qualify acceptance. The explicit `--review-policy
required` below marks the new run and keeps the reviewer gate selected at entry:

```sh
exitbind run start change --goal "Describe the bounded change you want to make" --ledger .exitbind/runs/run.jsonl --config exitbind.json
exitbind run next .exitbind/runs/run.jsonl --json --config exitbind.json
# The native host performs the returned assignment and writes its artifact at
# the returned artifactPathHint, then submits the named assignment:
exitbind run submit AGENT .exitbind/runs/run.jsonl \
  --outcome OUTCOME --artifact ARTIFACT_PATH --artifact-root state \
  --config exitbind.json
```

Replace the placeholders with the assignment and role-appropriate outcome.
Repeat `run next`, native execution, and `run submit` for each returned
assignment (including any rework cycle). Only the configured lead can record
the final `accepted` outcome; reviewer `approved` is not acceptance:

```sh
exitbind run submit lead .exitbind/runs/run.jsonl \
  --outcome accepted --artifact ARTIFACT_PATH --artifact-root state \
  --config exitbind.json
```

Submissions are role-scoped evidence, not votes; matching outcomes do not prove
consensus.

In a run marked with `--review-policy`, a reviewer `rework` routes to the Lead
instead of a worker, and the Lead cannot accept until it decides the finding:

```sh
exitbind work disposition WORK ASSIGNMENT --decision defer --reason TEXT
exitbind work disposition WORK ASSIGNMENT --decision repair --reason TEXT \
  --repair-boundary TEXT --regression TEXT
```

`defer` and `reject` start no work and keep the finding; the review then reads
`resolved_by_lead_disposition`, never `approved`. `repair` starts one attempt
whose worker and reviewer see the Lead's repair boundary, and `supersede`
additionally requires `--successor-basis JSON`. Both need a fresh check and
review. Unmarked runs keep their historical rework routing.

The owner can revise the policy only on a marked running run. Record the actual
choice with the lead agent and ledger path:

```sh
exitbind run review-policy lead .exitbind/runs/run.jsonl \
  --decision omitted --reason "Owner selected no independent review" \
  --config exitbind.json
```

`--decision required` and `--decision omitted` are the only choices. A run
started without `--review-policy` is the unmarked historical path: it retains
required-review semantics and cannot use `run review-policy` later.

### Checked acceptance

Opt a new run into one frozen deterministic check command:

```sh
exitbind run start change --goal "Describe the bounded change" \
  --ledger .exitbind/runs/checked.jsonl \
  --check-command "cargo test --locked" --review-policy required \
  --config exitbind.json
```

The native host performs and submits the normal assignments. With the stable
binary, capture a completed worker submission before executing the check:

```sh
set -eu
worker_event=$(exitbind run submit worker .exitbind/runs/checked.jsonl \
  --outcome completed --artifact .exitbind/artifacts/worker-result.md \
  --artifact-root state --event-id --config exitbind.json)
check_exit=0
cargo test --locked || check_exit=$?
exitbind run record-check .exitbind/runs/checked.jsonl \
  --target "$worker_event" --check-command "cargo test --locked" \
  --exit-code "$check_exit" --config exitbind.json
exitbind run status .exitbind/runs/checked.jsonl --config exitbind.json
```

New checked runs in `v0.31.14` use run-event format 8. The
reader retains historical v1–v7 ledgers, including their original producer
values and guarantees. Current v8 records bind an Accepted Subject, tested-input
identity, and the v0.24 basis/review-policy extension to each applicable result
and evidence record. The stable release supports historical v3 caller-reported
and v4 observed-check histories.

A current v8 check, reviewer approval, protection, and lead acceptance additionally
record the tested-input identity of the product root (tracked and untracked
non-ignored files, or every file outside a Git worktree; `.git` and state
directories excluded). Evidence taken on other inputs is not current, and
acceptance recomputes the identity. Ignored files, files outside the product
root, the environment, and remote or time-dependent conditions are not covered.
Binaries that read only v1–v7 refuse v8 ledgers as an invalid header instead of
reading them partially; historical ledgers gain no v8 basis/review-policy extension.
After the worker submission, the
frozen command can instead be run and recorded by Exitbind without accepting a
caller command or result override:

```sh
exitbind run observe-check .exitbind/runs/checked.jsonl \
  --target "$worker_event" --config exitbind.json
```

This local observation runs synchronously in the configured ProductRoot with
the invoking environment and permissions. It has a 1,800,000 ms (30 minute)
default deadline; both `work check WORK` and `run observe-check` accept a
positive `--timeout-ms MS` override from 1 to 86,400,000 when a different bounded
deadline is appropriate. It keeps command output off JSON stdout,
records normal exits and POSIX signals as distinct results, and writes no check
event when launch, timeout, or durable binding fails. On timeout it terminates
the spawned POSIX process group and reaps the command process. The command runs
outside the ledger lock, then Exitbind reacquires the lock and revalidates the
exact durable binding before append. A local observation is still check
evidence: review and lead acceptance remain separate. Existing v3 ledgers keep
the caller-reported `record-check` representation.

New checked starts with an explicit review policy also record
`checkObservationProtocol: 1`. Their observer appends a bounded admission before
launch, binding target, subject, inputs, configuration, frozen policy,
requirement, producer/executable digest and capture/deadline limits. Repeated
exact requests recover a committed check; they do not execute it twice. Changed
bindings or limits are refused. An observation/capture/commit failure records
separate process, deadline, capture, storage and termination facts, never a
passing check. Stable partial captures retain bounded private state references.
`groupEnded` measures the owned process group separately from
`captureReadersEnded`; uninspectably unfinished readers keep termination
unknown even after that group ends. Marked observation also requires the
frozen configuration and current selected contract before launch, commit and
committed replay.
`work detail` exposes the current Lead choice: an ended attempt may be repaired
through `work disposition`, a new worker permit/result, fresh checks and any
required review. An unresolved admission, live descendant or uncertain cleanup
permits stopping but blocks repair, acceptance, retry and supersession. If
failure storage is unavailable, the response says no durable failure was
recorded and returns a bounded inspection route; the admission stays unresolved.
For a retained known-ended storage-failure response, `work recover-check WORK`
previews a configured-Lead decision. Its explicit apply records a reported
failure only; ordinary Lead repair, a new worker result, checks and required
review still follow. Unknown effects remain blocked. See
[check recovery](docs/check-recovery.md) for exact-byte and reader requirements.
These additive v8 fields/events require a reader that supports the marker;
older v8 binaries refuse them. Markerless historical ledgers are unchanged.
See [observation schema](schema/check-observation-v1.schema.json).

This fragment assumes the worker is pending and has written that fresh result
file. Use the project's actual command consistently at start and execution.
`--event-id` returns only the successfully appended submission hash and a newline;
failed submission stops the shell sequence. Default submission output stays
JSON with the hash at `event.eventSha256`. On a binary without `--event-id`,
the host must extract that field before executing the check. Do not use an
artifact hash or select the latest event after execution. `--target` remains
required. `--event-id` and `--json` conflict and fail before mutation.

The command string must match the frozen start-time policy exactly. This
records a caller report; it does not execute or authenticate the check. Repeat
for each current worker result. Results from an old attempt cannot qualify a
new one. Start prints checked/unchecked requirements on stderr and preserves
JSON stdout. The updated `init` starter includes `--check-command
"YOUR_TEST_COMMAND"` and asks for substitution. Omitting the flag still creates
an unchecked run; generic `check` does not execute project tests.

A missing or failed current result prevents final acceptance. A refusal records
its exact check evidence without appending an accepted submission. Existing
artifact drift still refuses mutation without adding any run event. Rerun the
check after repair and report the actual result, or request `rework` using the
normal workflow and submit a fresh artifact. Passing checks and reviewer
approval remain separate from the lead's final acceptance.

### Exit Path receipt and verification

For an accepted v5 checked run, emit the deterministic Exit Path receipt from
the ledger:

```sh
exitbind receipt .exitbind/runs/checked.jsonl --json --config exitbind.json
exitbind receipt .exitbind/runs/checked.jsonl --json --output .exitbind/receipts/exit-path.json --config exitbind.json
exitbind verify .exitbind/receipts/exit-path.json --config exitbind.json
```

The receipt binds the run ledger bytes and head, configuration, Accepted
Subject, current worker/reviewer/lead artifacts, check targets, review, and
acceptance. Verification fails closed when covered subject, artifact, ledger,
configuration, check, review, acceptance, or receipt data drifts. A receipt is
local evidence, not tamper-proof proof or host/model compliance. Receipt output
is confined to StateRoot: use a StateRoot-relative `--output` path (or an
absolute path whose existing parent is inside StateRoot); an outside or
traversing path is refused with an actionable StateRoot guidance error.

Generate a local aggregate from explicit ledgers:

```sh
exitbind run report .exitbind/runs/checked.jsonl --config exitbind.json
exitbind run report .exitbind/runs/checked.jsonl --json --config exitbind.json
```

Historical checked Soulmate runs use run-event version 3. A checked successor
retains its frozen check policy and source category. Historical ordinary
Soulmate starts use v1, or v2 when a harness receipt is supplied; new Exitbind
starts use v8. Old ledgers are not rewritten, and new readers inspect the
frozen old fixtures. Binaries predating v3 reject that format, so retain a
supporting binary for those ledgers instead of relabeling them as an earlier
format.

`--proof-origin synthetic` is reserved for intentionally seeded runs; the default
checked-run category is `local_report`. Legacy ledgers without that metadata
have unclassified origin. The [proof methodology](docs/value-proof-methodology.md)
defines report interpretation and the token-free `exitbind benchmark` example.

Output from an external planner or orientation tool may seed the task, but it
is not assignment authority. Before starting a run, restate exact observe,
write, and command limits in Exitbind configuration or a boundary manifest. If
the producer omits one of those structured fields, treat the handoff as
incomplete; do not recover authority by parsing guide prose or inferring the
producer's rules. Every exact observe path must already exist when the run
starts; predecessor-created paths for a later stage are not currently
supported.

For a project outside the current directory, pass `--root PATH` to `init` and
use the printed `--config` path with later commands. A checkout can also be run
directly with a release binary from GitHub.

After installing `v0.31.14`, update project skill copies after upgrading the
CLI with the explicit refresh path. It requires an existing valid `exitbind.json` and only updates
files carrying Exitbind's ownership marker; unowned or conflicting files cause
the command to refuse the update:

```sh
curl -fsSL https://raw.githubusercontent.com/PANIC-ANGEL-SCIENCE/exitbind/v0.31.14/install.sh | EXITBIND_VERSION=v0.31.14 sh
exitbind init --refresh-skills --root PATH
```

The installer verifies the tagged archive checksum and stages the replacement
before switching the binary. It never rewrites project configuration or skill
copies; refresh remains a separate, reviewable project action.

### Cache and session uncertainty

Diagnostic and update reports keep installed CLI bytes, canonical declaration
and lock, each selected host's active materialization/cache, and instructions
already loaded in the current session as separate layers. Report each as
current, stale, inactive historical, unavailable, or unverified, and qualify
success by scope. A global plugin update or one host's cache does not prove
convergence elsewhere or in the current session. Resolve the active host path
before deletion, use only supported host-specific refresh, and treat reload or
new-session evidence separately from on-disk state.

### Conversational update notice

An active native Codex or Claude `SessionStart` integration may present
additional context asking the root agent to mention a newer public release.
The user cache is disposable, checked at most every 24 hours, and failures use
a one-hour backoff. Set `EXITBIND_NO_UPDATE_CHECK=1` to opt out. The notice
never installs software, and SubagentStart does not receive it; model surfacing
is advisory and not proven by the hook. Stable builds consider stable releases;
prerelease builds may consider stable and prerelease releases. Run
`exitbind update` explicitly to install a validated release. Versions released
before this feature cannot self-notify, so install `0.20.0` once to enable
future notices when the integration is active. A first SessionStart may then
perform the bounded lookup; later starts use the cache.

The installer currently verifies the archive's SHA-256 checksum only. Online
GitHub provenance is a separate, optional check for downloaded archives; it
does not prove source correctness or maintainer intent. With GitHub CLI, verify
the release archive and its repository provenance explicitly:

```sh
gh attestation verify exitbind-x86_64-unknown-linux-gnu.tar.gz \
  --repo PANIC-ANGEL-SCIENCE/exitbind
```

Coffee is retired as a public brand and feature. Exitbind has no Coffee command,
flag, MCP server, daemon, provider integration, or automatic tool runner.

Removing the binary or project configuration does not delete these project
skill directories. Remove the selected `.agents/skills/<name>/` and
`.claude/skills/<name>/` directories explicitly when no longer wanted. Optional
Codex/Claude hooks remain a separate, explicit `exitbind hooks apply` choice.

Historical records remain inspectable through committed frozen fixtures;
the installed Rust binary and Rust test suite are the only runtime and
compatibility authority.

## Repository modes

Initialization inside a Git worktree requires an explicit publication choice.
Portable mode keeps `exitbind.json` and the public `exitbind/` control tree,
including canonical profiles under `exitbind/agents/`, with the checkout;
runtime state stays ignored and mutation is refused if Git tracks or stages it:

```sh
exitbind init --mode portable --root /path/to/product
```

Local mode is for separately owned harness and product repositories. It writes
no Exitbind-owned file beneath the product checkout:

```sh
exitbind init --mode local --project-id my-project \
  --root /path/to/product \
  --control-root /path/to/harness/my-project \
  --state-root /private/state/exitbind/my-project
```

`ControlRoot` owns configuration and canonical profiles. Project-scoped skill
copies may also live there as distribution or host projections; they do not
become agent identity. `ProductRoot` owns source and submitted product
artifacts; `StateRoot` owns ledgers, locks, receipts, and private runtime
evidence. The local roots must be existing,
canonical, non-symlinked, non-nested directories. A private machine-local
binding maps the portable project ID to ProductRoot and StateRoot. On another
machine, recreate only that binding with `exitbind bind --config CONFIG --root
PRODUCT --state-root STATE`; Exitbind does not copy or synchronize run state.

Existing projects that still reference `.agents/profiles/` can inspect an
explicit, deterministic migration before changing any bytes:

```sh
exitbind migrate layout --config CONFIG
exitbind migrate layout --apply --config CONFIG
```

The dry run prints the exact source, target, agents, hashes, and before/after
configuration hashes. Apply refuses symlinks, destination collisions, and
tracked or staged Git paths; it repoints only configured legacy profiles and
does not rewrite prior ledgers or receipts. It preserves the configured
portable or local repository mode; migration must not move active or resumable
evidence.

Existing projects that predate the full public/private directory contract can
inspect, then apply, path migration:

```sh
exitbind migrate paths --config CONFIG
exitbind migrate paths --apply --config CONFIG
```

The dry run reports missing directories and any `copy-retain-legacy` of a root
`harness-manifest.json`. Apply creates the directories and copies reviewed
manifest bytes to `exitbind/harness/harness-manifest.json` while retaining the
legacy file. It never moves ledgers or receipts, never rewrites historical
evidence, and is unchanged when repeated against an already-complete layout.
It preserves the configured portable or local repository mode; migration must
not move active or resumable evidence.

## Directory responsibility boundaries

Portable mode separates reviewable control material, private runtime evidence,
and optional host discovery copies:

```text
PROJECT/
├── exitbind.json                   # agent-system configuration
├── exitbind/                       # reviewable profiles and boundaries
│   ├── agents/lead.md
│   ├── agents/worker.md
│   ├── agents/reviewer.md
│   ├── boundaries/                 # prepared empty
│   ├── policies/                   # prepared empty
│   └── harness/                    # prepared empty; no manifest yet
├── .exitbind/                      # private runtime evidence
│   ├── .gitignore
│   ├── runs/
│   ├── memory/
│   ├── artifacts/
│   ├── receipts/
│   ├── away/
│   └── locks/
├── .agents/skills/exitbind/        # host discovery copy; not identity
└── .claude/skills/exitbind/        # host discovery copy; not identity
```

These names assign storage responsibilities, not ownership of product source
or host configuration. `exitbind/` is canonical control material;
`.exitbind/` is private runtime evidence; the skill directories are host
projections, never agent identity. `init` does not create `.codex/`,
`agents.toml`, hook settings, or a harness manifest. In local mode the public
tree lives under ControlRoot and private state under StateRoot, leaving
ProductRoot untouched.

## What you get

The primary standalone workflow is:

```text
exitbind init        create a starter contract and canonical exitbind/agents profiles
exitbind check       validate profiles and boundaries without running an agent
exitbind brief       compile one bounded task packet
exitbind plan        map a goal to bounded named-agent handoffs
exitbind run         resume handoff state and record artifact evidence
exitbind verify      check recorded configuration and profile bytes for drift
exitbind memory      record and inspect authorized memory lifecycle evidence
```

The JSON configuration in [examples/exitbind.json](examples/exitbind.json) is a
working fixture for the deterministic core. `exitbind doctor` can report
missing profiles and optional host commands without invoking them. When no
direct dotagents command is on `PATH`, it reports an observed `npx` launcher
and `agents.toml` separately without claiming that the package was invoked.
The optional `exitbind hooks` command manages only explicit project-local
Codex or Claude bindings.

To reuse a project-specific agent brief, audit it first and keep iterating on a
compact portable runtime brief until the audit is clean:

```text
exitbind profile audit ./old-agent --forbid-term OLD_PROJECT --json
exitbind profile import portable_worker ./old-agent --purpose "Carry a bounded portable brief"
```

Audit accepts a regular file or a directory with one of the narrow supported
runtime-brief candidates. It reports only finding categories and line numbers;
detectors cover private-home paths, terminal prompts, private-key material,
credential token signatures and assignments, project-coupled paths, and an
optional forbidden term. `valid`/clean means only that no configured pattern
matched; it does not prove portability, secret-freedom, identity removal, or
publication safety. The audit never prints matched content. Import copies the
audited bytes, grants no observe/write/command/skill/memory rights, retains the
profile for the task, and does not assign it to a workflow. After reviewing the
result, explicitly add the agent to the intended workflow and execution
boundary.

## Memory governance

Memory is an opt-in filesystem protocol. Content stays in the operator-named
project file; one private hash-linked ledger records only its relative
path/hash, scope, actor/profile evidence, lifecycle, and producer.

```text
proposed -> reviewed -> accepted -> revoked
    |          |           `-----> expired
    `----------`----------> rejected
```

`memoryWrite` grants proposal authority only. Review, promotion, rejection,
revocation, expiry, and content-free forgetting evidence require separate
exact-scope rights. Rejected, revoked, expired, changed, malformed, duplicate,
or unauthorized items are not recalled.

```text
exitbind memory propose invariant_keeper docs/invariant.md --scope invariants --ledger .exitbind/memory/invariant.jsonl
exitbind memory review invariant_keeper .exitbind/memory/invariant.jsonl
exitbind memory promote lead .exitbind/memory/invariant.jsonl
exitbind memory resolve invariant_keeper --json --config exitbind.json
```

Recall is disabled without an explicit `memory` object. It scans only shallow
regular `*.jsonl` files under the configured project-relative root and fails
closed on unsafe evidence or strict item/byte budget overflow. Briefs, plans,
and runs carry deterministic content-free references; the native host reads
their exact sources. Semantic retrieval, embeddings, global memory, transcript
capture, and implicit truncation remain out of scope. See
[SECURITY.md](SECURITY.md) for path, expiry, concurrency, and erasure limits.

When memory is enabled, managed SessionStart context names the Lead's role and
profile hash and lists bounded references to currently eligible items. Read
one item with `exitbind project context memory ITEM_ID`; the command checks
eligibility and source bytes again. `exitbind project context --json` reports
the current project, focus, rule, memory, and optional resource observations.
These are context, not a check, approval, or permission. A changed source or
unavailable observation is reported as unavailable, not projected as current.

An optional `project.architectureContract` selection pins a project-relative
JSON source, exact SHA256 and revision. `project architecture --json` reads the
current contract. `project architecture select SOURCE --decision reviewed
--reason TEXT --json` previews an already reviewed selection and returns its
exact bound apply argv, preserving custom configuration. A digest is not review
evidence. `project architecture check --json` evaluates its bounded
literal file assertions and returns nonzero for failure. Current assignment
detail and native role context receive only the applicable slice. See
[Architecture Contract](docs/architecture-contract.md) for the format,
current/proposed distinction, authority and check coverage.

## Run and recovery

The [run and recovery reference](docs/reference-run-and-recovery.md) covers direct activity, native Work, resumable handoffs and safe recovery.

### Direct activity and native Codex Work

See [direct activity and native Codex Work](docs/reference-run-and-recovery.md#direct-activity-and-native-codex-work) for the complete commands and boundaries.

## Optional distribution with dotagents

[dotagents](https://github.com/getsentry/dotagents) is optional. Use it when
you want to distribute Exitbind's portable skill to several host projects; it
is not needed for the standalone CLI quick start and does not install the CLI
into `PATH`. Canonical project profiles remain under `exitbind/agents/` and
must be declared separately when you manage their projections with dotagents.

For an existing project with `agents.toml`:

```text
dotagents --project add PANIC-ANGEL-SCIENCE/exitbind --ref v0.31.14
```

For a new dotagents-managed project:

```text
dotagents --project init
dotagents --project add PANIC-ANGEL-SCIENCE/exitbind --ref v0.31.14
```

During `dotagents --project init`, select the hosts you use. `dotagents add`
installs the selected plugin immediately; a redundant install is not required.

Exitbind does not implement a package manager, mutate global host configuration,
or activate hooks merely because a plugin was installed. The portable Agent
Plugins v1 manifest carries the root `skills/` bundle, which stable dotagents
releases can install without activating the preserved host-hook extension. The
historical `systems.soulmate/` directory keeps host-specific manifests
and hooks under neutral public identifiers. It does not alias an old plugin
registration; reinstall the current plugin when replacing one. A successful dotagents install
proves that the portable bundle was accepted, not that a host activated or
executed a hook.

## Optional Codex and Claude hooks

Hooks are explicit project-local, advisory, fail-open integrations:

```text
exitbind hooks plan --hosts codex,claude --root PATH
exitbind hooks apply --hosts codex,claude --root PATH
exitbind hooks status --hosts codex,claude --root PATH
exitbind hooks remove --hosts codex,claude --root PATH
```

They write only `.codex/hooks.json` and `.claude/settings.json`, preserve
unrelated settings, refuse malformed/conflicting/symlinked targets, and require
the expected `exitbind hook-run` protocol on `PATH`. Session context is a
bounded project reference summary; subagent context presents the selected
profile and declared boundary. `exitbind project agents --apply` is a separate,
explicit operation that projects configured profiles into project-owned
`.codex/agents/` and `.claude/agents/` files; it refuses external file conflicts.
Hook execution/presentation is not activation or model
compliance proof. POSIX behavior is CI-tested on Linux and macOS; Windows
mutation is refused. See [SECURITY.md](SECURITY.md) for race and transaction
limits.

In local mode, `hook-run` resolves ControlRoot only through the exact private
machine binding whose ProductRoot matches the host `cwd`; it never searches
arbitrary parent directories. Bindings created before ControlRoot was recorded
remain readable for normal configured commands. Repeat the original
`exitbind bind --config CONFIG --root PRODUCT --state-root STATE` command to
add ControlRoot after all three existing roots match exactly.

## Host bridge

One installation covers the binary and the host guidance. `exitbind host
status` reports three separate observations per supported host: whether the
host is present, the managed bootstrap skill (`current`, `stale`, `missing`,
`unmanaged`, `unsafe`, or `unreadable`), and the managed session hook.

```sh
exitbind host status
exitbind host install [--hosts codex,claude] [--all]
```

`host install` writes only missing paths and files Exitbind manages. A foreign
file at the same path, a symlink, or another tool's hook record is reported and
left alone. Installing on the Exitbind surface retires a superseded
`soulmate hook-run` session-hook record and keeps every other hook. `exitbind
update` refreshes a stale managed bootstrap so a current binary cannot keep an
older host bridge. An update started by an earlier release's binary is the one
exception: that published code re-synchronises the bridge from its own copy
after the installer finishes, so run `exitbind host install` once with the new
binary afterwards.

Managed locations are the installed hosts' own user-level directories:
`~/.codex/skills/exitbind/SKILL.md` with `~/.codex/hooks.json`, and
`~/.claude/skills/exitbind/SKILL.md` with `~/.claude/settings.json`. `--hosts`
targets one host; `--all` also writes for a host whose directory is absent.

None of this proves activation. A present bootstrap means the lead can discover
Exitbind; selection remains its judgement, and governed work is established only
by a work handle, a recorded check, a review, or an acceptance.

## Removal

Standalone users remove the installed binary explicitly:

```text
rm "${EXITBIND_INSTALL_PREFIX:-$HOME/.local/bin}/exitbind"
```

The project-scoped skill copies created by `exitbind init` are ordinary project
files and are intentionally not removed with the binary. Delete
`.agents/skills/exitbind/` and `.claude/skills/exitbind/` explicitly if the
project should no longer discover the Exitbind skill. Removing the binary does
not remove project files, ledgers, receipts, hooks, or skills.

Removing the binary leaves the managed host bridge in place. Delete
`~/.codex/skills/exitbind/`, `~/.claude/skills/exitbind/`, and the Exitbind hook
records in `~/.codex/hooks.json` and `~/.claude/settings.json` if the hosts
should no longer discover Exitbind.

An install or update keeps the replaced binary in one hidden
`.exitbind-previous-PID/` directory beside the installed binary, so a running
older updater is not disrupted; the next install removes it, or delete it
explicitly with the binary.

If optional hooks or dotagents are enabled, remove project hooks while the CLI
is still available, then remove the optional plugin and CLI:

```text
exitbind hooks remove --hosts codex,claude --root PATH
dotagents --project remove exitbind
rm "${EXITBIND_INSTALL_PREFIX:-$HOME/.local/bin}/exitbind"
```

Reload the host or start a new session after plugin, hook, or skill changes.
Removing Exitbind leaves the underlying host and project usable.

## Evaluation

From a cloned checkout, this creates a disposable one-file sample project,
records the file as a run artifact, changes its bytes, and shows Exitbind
warning about the drift while still presenting the pending assignment. It
invokes no model, consumes no model tokens, touches no user project, and removes
the temporary directory:

```sh
EXITBIND_BIN=target/debug/exitbind ./scripts/demo-drift-warning.sh
```

This example proves only the drift warning and assignment presentation. The
authority and non-guarantee contract is defined once in [SECURITY.md](SECURITY.md).

## Versioning and release contract

For the supported update and rollback path, including custom-file ownership
and readers for marked carried work, see
[upgrade and rollback](docs/upgrade-and-rollback.md).

Exitbind uses the change in its public contract—not elapsed time or the number
of commits—to choose a version while it remains in `0.x`:

- `0.y.Z` is a backward-compatible defect, security, documentation, packaging,
  or release correction with no new user-visible capability or persisted shape.
- `0.Y.0` changes a product invariant or mechanically rejects a counterexample
  that the preceding minor line accepted. An intentional compatibility break
  also requires a migration decision and backward inspection where the prior
  format promised it.
- A receipt, ledger, configuration, or schema shape change must version that
  persisted format, add frozen compatibility fixtures, and document its
  inspection/migration contract; changing only the package number is insufficient.
- A new language runtime or external executable requirement is admissible only
  when it changes such an invariant; it also needs the same
  native/standard-library justification as a dependency.
- `1.0.0` requires external-use evidence that the documented CLI and persisted
  contracts are stable, recoverable, and supportable. It is not a completeness
  label and is never selected only because the feature list looks substantial.

`0.26.0` adds current readable assignment/evidence/task delivery and optional
binding-fenced action forms. It also ships the native-context and managed-file
invariants developed after the previous stable tag: private evidence aliases
and reconstruction after managed state loss are refused by mechanical checks.
The readable representation is version 1; existing ledger, receipt and legacy
section contracts retain their versions and readers.

`0.25.2` corrects saved-result recovery in the existing `work act` path on the
`0.25` line. It preserves recorded returns, prevents duplicate provider execution,
and refuses automatic resume when recorded execution is uncertain or could have
external effects. Existing run-event v8 records retain their identity and remain
inspectable; no rewrite of historical records is required.

`0.9.3` is a patch release because it removes an unsupported experimental
observation path from the product binary and corrects local-mode hook discovery
without changing the supported CLI or any receipt, run-event, memory,
configuration, or schema shape. A release tag is created only after its exact
commit passes the declared Linux, macOS, Windows-WSL, dependency-audit,
compatibility, privacy, and release gates.

## Advanced integrations

Runtime `host`, `model`, reasoning, and fallback values are opaque requested
bindings. Exitbind never stores provider credentials, routes models, or
silently substitutes a fallback. An agent's `skills` list is selection intent;
dotagents and the native host own installation, discovery, and invocation.

Generic readiness guidance is part of the lead surface. Venus or another
cross-session system remains advisory: a human accepts/narrows its Goal before
Exitbind starts a run. Exitbind never ingests raw session history or gains
cross-session authority.

For opt-in dogfooding evidence, place a version-1 harness manifest beneath
ControlRoot and pass it only when creating an existing brief or plan receipt:

```json
{
  "$schema": "https://raw.githubusercontent.com/PANIC-ANGEL-SCIENCE/exitbind/v0.31.14/schema/exitbind-harness-manifest.schema.json",
  "version": 1,
  "project": { "id": "my-project", "session": "codex-2026-08-30" },
  "harness": { "name": "my-harness", "version": "2026.08.30" },
  "activations": [
    { "kind": "skill", "name": "exitbind", "evidence": "configured" },
    { "kind": "perspective", "name": "qa-engineer", "evidence": "presented" },
    { "kind": "ponytail", "name": "ponytail:ponytail", "evidence": "hook_observed" }
  ]
}
```

```sh
exitbind plan change --goal "Bounded goal" \
  --receipt .exitbind/receipts/harness.json \
  --harness-manifest exitbind/harness/harness-manifest.json --config exitbind.json
exitbind verify .exitbind/receipts/harness.json --config exitbind.json
```

The canonical manifest path is fixed beneath ControlRoot. The legacy root
`harness-manifest.json` path remains verifiable for existing receipt-v2 bytes.
This creates receipt version 2 and binds the exact manifest bytes while persisting only SHA-256
identities plus the non-sensitive kind/evidence enums. Raw project, session,
harness, activation, and verifier strings remain in the operator-owned
manifest and are omitted from the receipt; deterministic hashes can still
permit offline confirmation of guessed low-entropy identifiers. Without
`--harness-manifest`, Exitbind continues to create and inspect receipt version
1. `configured`, `presented`, `agent_declared`, and `hook_observed` report
their source level; none proves model compliance. `independently_verified` is
an off-box verifier claim: Exitbind validates its format and binds it, but does
not hash a local artifact or authenticate the verifier. Unknown fields and free-form content
are rejected, so prompts, transcripts, secrets, raw environment values, and
unrelated project content have no manifest field. See
[the design boundary](docs/post-0.3.0-harness-evidence.md). OTel or Langfuse may
mirror the canonical receipt but never replace it as authority.

The manifest `$schema` value is advisory; compatibility is controlled by its
`version`, so a manifest using an older release schema URL remains accepted.

## Identifiers

The intended public identifiers are:

- GitHub: `PANIC-ANGEL-SCIENCE/exitbind`
- package namespace, if needed: `@panic-angel-science/exitbind`
- CLI: `exitbind`

The unscoped npm package name `exitbind` is already owned by an unrelated
project, so Exitbind will not claim or depend on it.

### Initialize without project skill projections

`exitbind init --mode portable --root ROOT --skip-skills` creates configuration,
profiles, and private state while preserving all project skill paths. The same
option applies to local mode with its usual root and binding arguments. It does
not follow skill-directory symlinks or claim guidance was installed. Combining
it with `--refresh-skills` or `--with-coffee` is refused before writes.

## Durable project lessons

Opt-in [project lessons](docs/project-lessons.md) reuse the role-scoped memory
lifecycle and arrive in ordinary recipient context. Inspect applicable references
with `exitbind memory resolve AGENT --task CURRENT_GOAL --json`. Lead ownership,
expiry, source guards and delivery bounds keep this context separate from Work
authority.

## Approved next-launch capabilities and release policy

The commands in this section are included in the published 0.31.14 release.

Use [project capabilities](docs/project-capabilities.md) to register reviewed
skills or lenses for relevant configured native recipients. Registration and
projection grant no host rights. Current assignment, approval and complete
bounded context remain required.

`exitbind work detail WORK --view action --json` reads the current executable
forms and current decision metadata without redelivering consumed instructions.
The metadata is navigation, not evidence of complete instruction delivery or
review. Its exact section routes
retain mandatory context. After a permit/edit, follow the permit response's
current action route; do not reuse a pre-permit return binding.

`exitbind memory form --scope project-lessons.v1 --json` reports the complete
proposal shape, 2048-byte source and 512-byte UTF-8 fact bounds, and immutable
correction identity. See [release series](docs/release-version-policy.md) for
the 0.31.14 release series and publication gates.

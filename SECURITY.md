# Security boundary

## Update checks and explicit installation

An active native Codex or Claude SessionStart integration may query the fixed
public GitHub releases endpoint without credentials and present additional
context asking the root agent to mention a notice from a disposable,
permission-restricted cache. Model surfacing is advisory and not proven by the
hook. SubagentStart and opted-out invocations stay silent. The notice path
never installs software. `exitbind update` is the explicit install boundary: it
accepts only a validated release tag, downloads the matching installer from the
fixed project origin, reuses the installer's checksum and atomic replacement,
and verifies the resulting binary. A failed update normally restores the prior
binary; if restoration itself fails, the command reports that failure and
retains the recoverable backup.
The checksum protects the downloaded archive; this flow does not claim
attestation or independent provenance verification.

Update evidence is layered: installed CLI, canonical declaration and lock,
each selected host's active materialization/cache, and instructions already
loaded in the current session are separate observations. A current global
plugin or one host cache does not prove another host or session is current.
Report stale, historical, unavailable, and unverified layers explicitly;
resolve the active host path before deletion, use supported host-specific
refresh only, and treat reload/new-session evidence separately.

In the `v0.25.0-rc.4` release candidate, `run observe-check` executes only the frozen command
in ProductRoot with inherited permissions and environment. It discards child
stdout from the machine JSON stream, inherits stderr, records exit and signal
results distinctly, and writes no check event when launch, timeout, or durable
binding fails. Observation uses a bounded deadline, runs outside the ledger
lock, terminates its POSIX process group and reaps the command process on
timeout, then revalidates the durable binding under the lock before append. The
host/OS remains the sandbox and permission owner.

Exitbind is a local protocol companion. It reads the configured project file
and declared profiles, rejects escaping paths, and does not provide an OS
sandbox, process isolation, model-compliance proof, or transcript scrubber.
Explicit project-local hook handlers are advisory and fail open: malformed input
or unavailable project state must not block the host.

The primary threat is an honest operator or agent making a publication,
configuration, path, or resume mistake. A process that can rewrite all local
evidence can recompute its unsigned hash chains; Exitbind does not defend local
state against that attacker.

Local mode separates three trust domains: configuration and profiles remain
beneath ControlRoot, product reads and submitted artifacts beneath ProductRoot,
and mutable evidence beneath StateRoot. The roots must be distinct,
non-nested, canonical directories without symlink components. Their absolute
machine mapping lives in a private create-only binding, not portable config.
Git preflight refuses tracked or staged private state before and after run-lock
acquisition. Git and ignore checks reduce publication mistakes; they are not
access control or secret erasure.

Memory-governance ledgers contain relative source/profile paths, hashes,
authorization metadata, and timestamps, but not the referenced memory content.
They are hash-linked evidence, not signed or tamper-proof audit logs. Keep them
private when their metadata is sensitive, serialize concurrent writers, and do
not treat `memoryRead` declarations as filesystem access control. Exitbind
checks configured scope rights before appending transitions; the external
runtime remains responsible for whether an actor can read the source file. Final
ledger components are opened with no-follow append semantics and rechecked
before writing. Mutation is refused when the platform does not expose
`O_NOFOLLOW`. A parent directory can still be replaced between validation and
open; no full transaction or host-wide lock is provided, so callers must
serialize writers.

Run ledgers keep the explicit handoff goal in private project-local
`.exitbind/` state so a native host can resume it. `run start`, `run next`,
`run submit`, and `run inspect` JSON is sensitive local operational output and
is not redacted; `inspect` intentionally exposes the start event's goal. Run
events store artifact paths and SHA-256 hashes, not artifact contents. Before
resume or another submission, Exitbind checks each recorded artifact is still
a project-confined regular non-symlink file with the same hash; drift or
deletion blocks without appending. `run inspect` proves only recorded chain and
predecessor consistency and does not claim current artifact or configuration
parity.

A run-scoped boundary manifest may narrow configured `observe` and `write`
maxima to exact ProductRoot-relative paths. It cannot widen a maximum, use an
exact-path glob, escape ProductRoot, or traverse an observed symlink. The
manifest hash is frozen in the start event and revalidated on resume. This is
inspectable assignment evidence; native-host filesystem enforcement is a
separate responsibility.

Assignment artifact hints target StateRoot and include run, agent, stage, and
attempt identity without goal text. They are advisory names, not automatic
writes. Every recorded upstream artifact remains immutable; a changed,
deleted, or substituted prior artifact blocks resume and submission.

## Checked-result evidence

Opt-in run-v3 ledgers bind a frozen check command and source category to the
run; v3 supports caller-reported `run record-check` only. The `v0.25.0-rc.4` release candidate
release supports both reported `run record-check` and local `run observe-check`.
`run record-check` consumes a caller-supplied exit status for an exact
current worker submission. It does not execute that command, authenticate the
caller, or prove that the host ran it. Neither a reported zero exit code nor a
reviewer approval creates lead acceptance. The reducer checks this policy when
replaying acceptance as well as when the CLI appends it.

Check and refusal records use the same confined, locked run-append path. A
check refusal can add factual protection evidence while leaving the canonical
result unaccepted. Changed artifact bytes still prevent any new run event.
These checks bind the recorded artifact set, not all project files or the full
execution environment. Raw ledgers and status/explain output remain private
operational evidence, including the configured command.

Aggregate reports redact user-controlled text and separate explicitly synthetic
runs, local caller reports, and legacy unclassified runs. They do not infer
avoided loss, human attention, or false positives. Reported command duration is
not a local measurement of human time. Export remains explicit; hashes are not
anonymization.

The fixed `benchmark` self-test invokes the same installed executable against
new disposable fixtures. It does not execute user-provided commands or discover
work in the current project. Its optional export creates a new local directory
with synthetic ledgers and artifacts; it refuses to overwrite an existing
output directory. No model or network call is part of the scenario.

## Content authority and instruction-like data

Byte integrity and instruction authority are separate properties. A valid hash
proves only that Exitbind selected the recorded bytes. It does not make memory,
an upstream artifact, a receipt, or a harness claim authoritative instructions.
The native away prompt therefore presents content in this order and with these
labels:

1. current host/system constraints and the Exitbind execution contract;
2. the authoritative run context and assignment packet;
3. the exact reviewed profile as subordinate role guidance;
4. selected memory as context-only data;
5. upstream artifact references and harness claims as evidence-only data.

Instruction-like text in the last two categories must be treated as inert data.
It cannot widen a boundary, grant approval, redefine acceptance, or override the
assignment. If such content should govern work, an authorized operator or lead
must place the decision in a reviewed configuration, boundary, assignment, or
explicit successor run.

The assignment goal describes the task. It does not grant authority and cannot
widen the declared boundary, even when it contains instruction-like text.

Exitbind does not use a lexical prompt-injection scanner, delete suspicious
phrases, or claim that labels enforce model behavior. The away prompt projects
selected profile, memory, and manifest content as JSON strings so those bytes
cannot create new Markdown section headings, while preserving a reversible
representation and an explicit authority label. The host and model may still
misinterpret or obey quoted text. Adversarial fixtures verify section order,
structural quoting, and content preservation, not injection resistance or model
compliance.

New receipts and ledger events record the producing Exitbind version and, for
release builds, the source commit. The producer field identifies the binary; it
does not sign the evidence or prove that the recorded executable was trusted.

An opt-in canonical `exitbind/harness/harness-manifest.json` (or the supported
legacy root `harness-manifest.json`) may bind portable project/session tokens,
harness identity, and bounded skill, perspective, or Ponytail activation claims
into a version-2 receipt. The manifest must be a no-follow regular file at one
of those exact ControlRoot-relative paths, is limited to 64 KiB and 64 activation
entries, rejects unknown fields, and has no field for prompts, transcripts,
secrets, raw environment values, or unrelated project content. The receipt
stores only hashes of user-provided strings; their raw values remain in the
operator-owned manifest. The manifest SHA-256 proves byte identity only.
Those deterministic hashes permit equality checks and offline confirmation of
guessed low-entropy identifiers, so StateRoot receipts remain private evidence.
Recorded evidence objects have fixed shapes; adding a field requires a receipt
or schema version bump. The constant `privacy` field remains in receipt v2 to
preserve that frozen shape and states only that raw manifest values are omitted.
`configured`, `presented`, `agent_declared`, and `hook_observed` name the claim
source, not model compliance. `independently_verified` is an off-box verifier
claim: Exitbind validates its format and binds the claim, but does not hash a
local artifact or authenticate the verifier. Its supplied `artifactSha256`
value must be 64 lowercase hexadecimal characters.

A run may opt into that existing receipt-v2 evidence with
`--harness-receipt RECEIPT`. The receipt must be an existing no-follow regular
file beneath StateRoot, and Exitbind binds its relative path and exact SHA-256
in a v2 start event. `run next` and `run submit` revalidate the receipt's exact
path, version, bytes, and hash. Changes to current configuration/profile bytes
or to the semantic ControlRoot manifest are reported as warnings; the
human-directed run continues with its recorded plan. A receipt profile/runtime
set that does not cover the selected plan is rejected. This preserves evidence
continuity across a tmux boundary; it does not prove that a model read,
activated, or complied with the manifest claims. Exact receipt or artifact
integrity failures remain a hard refusal.

Explicit supersession creates a new bounded ledger linked to the predecessor's
run ID, full ledger hash, verified head, and run-start configuration hash. It
never edits the predecessor, copies its goal, or silently migrates
configuration. A project-local exclusive claim seals the predecessor against
later CLI submissions and prevents a different successor under the same claim.
External replacement makes provenance inspection fail. Exitbind stores hashes,
not a configuration snapshot or global archive.

Run mutation uses a project-local no-follow lock containing only a PID and
creation timestamp. Liveness and stale recovery are conservative and
same-host: alive or permission-denied owners stay busy, PID reuse can appear
busy, and malformed, replaced, or unverifiable locks are never force-unlocked.
This is best-effort writer exclusion, not a full transaction or tamper-proof
audit. The `.exitbind/.gitignore` keeps run/state/artifact contents out of
ordinary Git staging, but forced staging, other publication tools, and prior
history remain possible. It does not prove that local or untracked data has
been purged. The repository privacy gate checks its defined publication
surface; it is not a general secret scanner or purge guarantee.

Readiness guidance is part of the lead surface and adds no configuration,
command, installation, or execution authority. A native host must apply its
own trust, invocation, and permission policy before using any suggested tool or
skill.

The optional Codex+tmux away adapter is a project-scoped host integration, not
a core daemon, scheduler, provider client, distributed lease, or reboot
recovery service. It accepts only a currently pending `runtime.host=codex`
assignment, forces approvals to `never`, and adds only the assignment's fresh
StateRoot artifact parent as a writable root. An explicit `--sandbox-mode` is
passed to Codex and recorded; without one, Codex inherits its resolved
configuration and the private state records `unknown`. Exitbind records this
posture but does not enforce that Codex or the operating system obeyed it.
It revalidates the exact assignment, profile, and selected memory source hashes
immediately before launch and never selects a fallback. A task-specific tmux
socket prevents a second live same-host launch of the same assignment; this is
not cross-host exclusion and does not survive a host reboot. Native process
exit is not completion: only an exact stage/attempt/agent submission event
produced by normal `run submit` is canonical. Leaving the pending set without
that event is reported separately and never upgraded to completion.

The adapter does not persist its prompt, assignment packet, manifest,
transcript, raw environment, or a second JSONL history. Its mode-0700 StateRoot
recovery directory may contain bounded identifiers, status, recorded sandbox
posture, the native exit kind/code or termination signal, and Exitbind-generated
bounded errors. Native stdout and stderr are
discarded so an executable cannot echo supplied context into recovery state.
Those files remain private local process evidence, not receipts or proof of
model compliance. A bound assignment hash-reads the exact
ControlRoot manifest named by the receipt and presents only its bounded raw
claims to the in-memory Codex prompt; `--require-harness-receipt` refuses an
unbound assignment.
Missing, changed, symlinked, or malformed receipt/manifest evidence prevents
native launch. The spawned Codex process follows normal native project and
global discovery, and the adapter does not upgrade configured, presented,
agent-declared, hook-observed, or independently-verified claims.

The runner is part of the single Exitbind binary and writes recovery state only
beneath the configured StateRoot. It does not use npm lifecycle scripts or
mutate user-global host configuration. The project skill documents the command
but contains no executable runner copy.

A forgetting-attestation receipt records only a local observation that the
governed source path was absent after a terminal memory transition. The command
does not delete the source, and the receipt is not proof of secure erasure,
backup deletion, remote deletion, or model forgetting.

Opt-in project recall discovers only shallow regular JSONL ledgers below the
configured project-relative memory root. Eligibility is decided before any
content delivery from the recorded lifecycle state, current source hash, exact
requesting-agent scope rights, cross-context policy, and explicit item/byte
limits. Rejected, revoked, expired, changed, missing, malformed, duplicate, or
unsafe evidence is never silently substituted. Passing recall references are
content-free; the native host, not the receipt or run ledger, reads the exact
source bytes for the matching subagent. Run mutation revalidates frozen memory
references so a later lifecycle or source change blocks rather than injecting
stale context.

Recall source files are opened relative to a no-follow project-root directory
handle, traversing parent directories without following symlinks. The same
descriptor is read twice before its hash is accepted. This narrows path-swap
attacks but does not turn project-local authorization declarations into an OS
sandbox or protect against an attacker who can mutate the process itself.

The memory root and vectors derived by any future external retriever should be
treated as sensitive project data. Exitbind does not currently generate
embeddings or call a retriever. An optional external ranker would not gain
authorization authority: it may rank only already-eligible references, and its
results must be revalidated before use. Do not send source text to an external
embedding provider without a separate, explicit project privacy decision.

`hooks apply` and `hooks remove` require an explicit host list and only touch
`.codex/hooks.json` or `.claude/settings.json` below the selected project root.
They reject malformed JSON, unexpected hook shapes, NUL-containing paths, and
settings-directory or settings-file symlinks. Existing settings are merged
rather than replaced; an exact
Exitbind handler is idempotent, while a handler containing Exitbind's ownership
marker but differing from the expected definition is a conflict and causes no
mutation. The operation preflights all selected hosts before writing. Multiple
files are serialized before sequential same-directory atomic renames, but there
is no durable cross-file transaction. Source bytes and path confinement are
rechecked immediately before each rename, but an unrelated writer racing after
the final check cannot be excluded without host-wide locking. If a later rename
fails or another process edits settings concurrently, inspect status before
retrying.

Platform support and package-validation status are maintained in the
[platform support details](docs/platform-support.md). That table distinguishes
private native packaging evidence from public archive and installation
verification; it is the authoritative platform matrix for this release.
Optional hooks invoke
`exitbind` by `PATH`, with a fail-open shell guard and a five-second native host
timeout.
`hooks apply` refuses to install when that executable is not currently
resolvable on `PATH` or does not return Exitbind's expected hook protocol; host
launch environments must continue to expose the same command. It has no
absolute install path, network,
npx, daemon, dependency, or model call. The internal `exitbind hook-run`
command reads only the host event stdin and emits bounded context for
`SessionStart` and `SubagentStart`; it ignores other events and fails open.

The historical `systems.soulmate/` directory preserves host-specific
manifests and hooks as compatibility resources only. Its presence does not
activate a hook.
When a trusted, supported host consumes an explicitly installed resource,
`SessionStart` emits the lead name plus configured agent and workflow names.
`SubagentStart` emits the selected profile path, hash, text, and declared
observe/write/commands/skills/memory/retention boundary into the active
host/model context. This is not model-compliance evidence. Path redaction is not
secret detection; profiles must contain no secrets or unrelated private data.
Windows hook mutation is refused, and Grok behavior has not been locally
executed or verified. A successful portable plugin/skill install does not prove
host-specific hook activation.

Do not include secrets, API keys, private profiles, prompts, transcripts, or
other sensitive data in public issues or reports. If enabled for this
repository, report suspected vulnerabilities through a private GitHub Security
Advisory; otherwise contact the maintainers privately through the repository's
trusted channels. Include a minimal reproduction without credentials.

Supported versions are the current release line shown in `Cargo.toml` and the
matching Git tag. Historical format compatibility is pinned by committed Rust
fixtures rather than a second runtime implementation.

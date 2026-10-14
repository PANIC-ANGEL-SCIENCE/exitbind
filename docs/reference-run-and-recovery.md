# Run and recovery reference

[Command and protocol reference](../REFERENCE.md) covers setup, authority and checked acceptance.

## Run and recovery

Run state is private operational data beneath StateRoot. Each assignment fixes
the selected profile, requested runtime, declared skills/boundary, memory
references, upstream artifact hashes, and producer evidence. These are
selection/presentation records, not proof that a host or model complied.

### Direct activity and native Codex Work

In an initialized project, `exitbind activity codex < TASK_FILE` runs one
direct Codex task from standard input and writes a private activity record.
`exitbind activity show ACTIVITY_ID` reads it. The observation includes
structured command outcomes and a bounded before/after changed-file reference;
the task outcome remains `unjudged` and acceptance is not applicable. The
default sandbox request is `workspace-write`. A check run separately by the
host or CI creates no Exitbind activity record.

For automatic external control/state placement on managed hosts, see
[project placement](project-placement.md). The optional Linux file enforcement
cell and its support matrix are described in
[controlled file effects](controlled-file-effects.md).

For governed Work, the lead uses `exitbind work act WORK [--model MODEL]
[--reasoning-effort EFFORT]` through the current detail's
`actionForms.productManaged.command` when the configured role selects Codex.
Its `--current-binding` fence refuses stale delivery before provider execution;
the companion inspect command is read-only. A worker or reviewer action launches Codex with the checked packet
and profile and records one structured native result. The reviewer receives
the packet-bound current worker result. A later check action runs the frozen
check (`work check WORK` is also available); a later Lead action requires an
explicit `--outcome` and `--reason`. These are separate calls and neither
check success nor acceptance is inferred from a provider result. A recorded
resumable worker turn may use `work act WORK --resume`; an uncertain running
journal is a stop, not permission to start another provider process.

Native actions receive complete current project rules and eligible opted-in
role memory alongside the checked profile and packet. Oversized sources refuse
launch instead of being truncated. A native worker also receives an assignment-bound
file tool. `edit read PATH` captures permitted UTF-8 bytes or explicit absence;
`edit edit PATH` takes the complete replacement on stdin. The supplied tool owns
the executable/configuration binding, read baseline and durable retry identity.
Use `edit inspect PATH` after a conflict or uncertain response, and `edit refresh
PATH` only for a deliberate new edit after the prior request is resolved.
It requires the exact current unconsumed mutation grant and reuses declared
paths, protected controls, file preconditions and atomic replacement. Inspect
`actionForms.currentGrant` for the exact current grant and `actionForms.nextRequest`
for required re-plan/evidence before another mutation request; a new permit
form is present only when a new permit is admissible. `replan_required` and
`evidence_required` apply to the next new mutation request; they do not revoke
that current grant. A validated sensor request or inert
sensor result preserves it; a conservative sensor result or later semantic
governor event makes it stale. A retained worker result takes priority over
editing and must be resolved through the current recovery form. Worker completion
still requires the existing ready-governor state. Existing expert `work write
WORK ASSIGNMENT PATH --operation ID --expected-sha256 SHA_OR_absent` remains
compatible. Managed read/refresh/inspect requires a single-link regular file,
checked on its actual descriptor before reading and before returning content.
Hard links, including ordinary source aliases, refuse without disclosure.
Neither route intercepts other native tools or remote effects.

For a known Work or explicit locator, `work detail WORK --json` is the direct
ordinary current read. A new `work begin ... --detail` delivers that same
verified current recipient context with a `creation` record, including the
Lead's initial scope decision. Without `--detail`, the historical start envelope
is unchanged. Applied setup emits the detail form. Failed delivery after a
recorded start retains the Work and a bounded read-only recovery; a lost reply
uses `work resume --json` with the same configuration, never another begin.

Unknown project Work uses `work resume --json` and its
emitted detail route. `work next WORK --json` also exposes `current.details.grouped`, a versioned
`ref:work-detail:v1:<binding>` route for ordinary consumers. `work detail WORK
--json` reads the same current route directly. The product validates the
ledger, configuration, recipient and task binding, then returns grouped
assignment, evidence and task sections with exact verified UTF-8 artifact
content. `recipientContext` also delivers only the current recipient's exact
profile, configuration identity, declared boundary and current project rules.
Ordinary recipients can use this read without separate profile/rule discovery.
Instructions are checked before and after assembly; changed recipient, profile,
configuration, rule or Work bytes refuse delivery. Worker/reviewer return forms
and the worker's pre-edit permit accompany the distinct Lead choices. A pending
frozen check has its mechanical command with no review or acceptance authority.
The bounded grouped response is at most 256 KiB; `complete` is false
when non-UTF-8 or oversized evidence cannot be represented as readable text,
and the response names the current scoped evidence route for the supported
fallback. Legacy `current.details.assignment`, `.evidence` and `.tasks` routes
remain available for expert hex paging and preserve their 24 KiB page and
64 KiB response contracts.
An oversized or non-UTF-8 recipient profile is also explicitly incomplete and
names the existing profile reader; it is never silently truncated. Project
rules are complete UTF-8 content with a 32 KiB per-rule and 48 KiB aggregate
source limit. Ordinary detail also limits the JSON-encoded rule projection to
48 KiB, leaving room within the ordinary 256 KiB grouped Work detail bound.
Native current context remains bounded at 64 KiB and the Codex prompt at 256 KiB.
Invalid UTF-8, unsafe paths, changed inputs and over-limit rules refuse without
truncation.

Native launch performs the same grouped evidence validation before starting a
provider and supplies the resolved exact assignment, evidence and task sections
directly. Its existing profile reader retains the separate 64 KiB native bound.
An inline-profile fallback does not waive evidence validation or block a profile
that the native reader can fully validate. A provider does not write paging,
hex decoding, checksum, or JSON-join code. The current `actionForm`
also supplies executable, assignment-bound Lead choices: failed checks use
`work return WORK ASSIGNMENT --outcome rework|blocked --reason <REASON>`;
pending findings use only their current repair/defer/reject/supersede choices
and required terms; acceptance exposes accept, rework and blocked. Placeholders
are Lead decisions. Forms include `--current-binding` to reject a changed goal,
recipient or ledger before mutation; the legacy assignment fence also remains.
Preparation has an immutable, synced record beside the Work ledger; missing
session state refuses reconstruction. Preserve private StateRoot integrity:
joint loss of both preparation record and session is indistinguishable from
first use. See the linked guide for recovery limits.

`work closeout WORK [--export | --output PATH | --receipt PATH] --json`
resolves the exact Work through existing receipt owners. Without an export or
receipt request it is read-only and returns recorded Work acceptance, canonical
goal/task readiness, current evidence availability and the optional export argv.
Terminal compact responses expose this as `current.completion`; complete detail
exposes `completion`. Terminal `effectiveAction.exists=false` still denotes no
Work advancement. A canonical external goal remains separate from Work readiness
and any publication/install obligations it records.

`--export` creates the existing `exit-path-v1` receipt at the Work-specific path
beneath StateRoot's managed receipts directory. `--output PATH` is an explicit
export to another normalized StateRoot path beneath existing real directories.
When StateRoot is ProductRoot, new exports stay beneath its managed receipts
directory so the export cannot invalidate the checked product inputs.
No symlink directory/leaf alias, hard-linked receipt or outside destination is
accepted. Existing files are never overwritten: a repeated/lost-reply export
succeeds only if those bytes verify for this exact Work. `--receipt PATH` checks
existing evidence read-only and cannot combine with export options. Corrupt,
missing, stale or cross-project evidence refuses without changing the ledger,
permits, checks, review or acceptance. Closeout responses stay within 8 KiB.

Historical ledger receipts retain their existing validation meaning through
`receipt`/`verify`. Current Work closeout additionally uses the established result
reference evidence owner to require current configuration, profiles, artifacts
and bound tested inputs; an old unbound or stale accepted Work remains historical
and has no current export action. This does not add a receipt prerequisite to
previously completed Work or to small direct work.

`work world WORK [--plain --reduced-motion]` reads six canonical event motifs.
`--export --json` omits private task markers and source text. `work next
--themed` and `work act --themed` add the same projection to their JSON replies.
An explicit finding `reuse` record needs a later current passing observed
check; retrieval alone never lights the signpost. See
[current context and the quiet hall](continuing-native-work.md) for the
commands, bounds, state meanings and recovery limits.

The lower-level `work next`, `work bind`, `work child prepare`, and `work child`
route remains for host-managed child handoffs and cross-host continuation;
see [onboarding](onboarding.md) and
[cross-host continuation](cross-host-continuation.md). `project context
--json` provides bounded current project facts and `goal status --json`
separates recorded closure from current readiness. Neither output is a
substitute for executing and checking the selected path.

For a readable current assignment with the stable binary:

```sh
exitbind run next .exitbind/runs/run.jsonl --text --config exitbind.json
```

`--text` presents the validated goal, stage/attempt, pending role and actor,
result path/root, frozen check policy, and upstream artifact references marked
as prior/current attempts. It does not copy artifact contents or memory into a
summary. A terminal run reports its actual status and no pending assignments.
Default `next` and `--json` retain their JSON packet; combining `--text` with
`--json` is an error. Presentation is read-only and never executes displayed
paths or goals. It reconstructs recorded work, not a host conversation.

Human `run status` shows the current attempt and a concrete shell-quoted
read-only `next --text` command when the assignment validates. Terminal or
drifted runs instead point to inspection/recovery. Default `run inspect` JSON
retains all attempts; it checks recorded history without revalidating current
artifact or governing-input bytes. Keep these outputs private. Your project CI
remains the executor and enforcement point for its own checks; a local report
binding does not authenticate CI execution or replace branch protection.

`goal status --json` keeps the recorded `closure` as history and adds
`currentReadiness` (`current`, `stale`, or `unknown`). A historical closure can
remain recorded after its tested inputs or required support change. Ordinary
status stays plain. Add `--themed` to interactive `goal status`, `run status`,
or `run explain` to show the three-line whole-goal room only when the current
named goal is ready; machine and noninteractive output never include it. The
room is local display, not completion evidence, and its replaceable display
memo does not affect tested inputs.

Optional prospective usage observations belong to an existing canonical goal
or Work. Read them with `exitbind goal usage --goal-id ID --json` or
`exitbind work usage WORK --json`. Recording requires explicit `--apply` and
one allowlisted numeric JSON object on standard input, limited to 8 KiB:

```sh
exitbind goal usage --goal-id ID --apply --json <<'JSON'
{"id":"setup-1","source":"host_reported","scope":"setup","phase":"setup","status":"observed","semantics":"delta","lifetime":"invocation","values":{"inputTokens":100,"cachedInputTokens":20,"outputTokens":30}}
JSON
```

Use actual host counters and stable observation identities; this example is
illustrative. The route stores no prompt, output, or transcript and launches no
provider. It records private telemetry without changing the semantic goal,
mutation grant, check, or acceptance. The account retains missing coverage
instead of treating it as zero. Phases are `setup`, `implementation`, `review`,
`repair`, `resume`, and `closeout`; scopes are `setup`, `root`, `direct`,
`native`, and `child`. Supported counter semantics are `delta`, `per_turn`,
and `cumulative`. Cumulative observations require a baseline and compatible
lifetime identity. Session counters and per-turn observations require an
explicit `sessionId` and `counterId`; goal counters require `goalId` and
`counterId`. Adapter, scope, and lifetime differences keep streams separate.
The projection map names ingestion scope `root` as `rootLead`; stored observations retain `root`.
Replay and declared parent/child overlap do not add twice.
Native completion snapshots with unknown additive semantics remain separate.
Cached input is a subset of input; total tokens are input plus output. Partial
coverage and host-reported counters do not establish whole-goal savings.
An observation recorded through `work usage` with the current `goalId` uses
the same goal observation owner, so goal and Work inspection see it once.
Without `goalId`, Work observations remain isolated. A supplied Work link is
caller-reported numeric attribution and grants no semantic goal ownership or
permission. The account keeps unassociated native and root coverage unknown.

The same JSON status includes product-owned `goalProgress` when a canonical
Lead goal exists. It lists the Lead's explicit obligation tasks, keeps
performed-but-stale result history visible, and reports current Work
`resultReadiness` separately. Missing goal decomposition is reported as
`unavailable`; progress is never inferred from a worker packet or an elapsed
percentage. Each bounded task entry includes a stable `taskId` hash alongside
its shortened label; the full goal-status route retains canonical detail. The
read-only `work continuation` and `work expand` commands accept
an optional redundant `--json` for consistent scripted discovery.
The same compatibility flag is accepted by `work validate` and `verify`; all
four routes retain their existing JSON output and continue refusing unknown
options.

`work resume` reports its discovery scope in JSON: it reads only the configured
project's StateRoot-relative `.exitbind/runs` directory, without recursion or
automatic widening, and stops before processing more than 128 valid work
ledgers. Its opaque `discovery.directory` value is `work_ledgers`, under
`configured_state_root`; transport paths stay outside the normal envelope.
A bounded refusal names the observed count and gives either the
saved focus's exact `work next WORK --json --config CONFIG` route or asks for
an explicit Work and config.

Use an exact run boundary to narrow configured maxima without editing config.
This example uses the separate advanced fixture `examples/exitbind.json`; it
is not the starter config created by `exitbind init`:

```sh
exitbind run start change --goal "One bounded change" \
  --ledger .exitbind/boundary-run.jsonl \
  --boundary examples/boundaries/change.json \
  --review-policy required \
  --config examples/exitbind.json
```

Every attempt uses a fresh StateRoot artifact path. Prior artifacts are
immutable. Exact artifact or receipt path/byte/hash failures, deletion, unsafe
paths, tracked/staged private state, unreadable required boundary inputs, and
unverifiable locks still refuse mutation. Ordinary configuration, profile,
memory, boundary-content, and semantic harness drift is reported as a warning;
the active human-directed operation continues with its recorded plan. Use
explicit supersession only when you want a new run bound to changed inputs.

For a host-managed attended assignment, use the host's native subagent spawn
with the exact `nativeTaskName`; unavailable spawn leaves that route pending.
The separate product-managed Codex route is `work act`, which consumes
structured Codex completion events and refuses missing results. An ad hoc
shell `codex exec` does not bind a Work result.

When the operator explicitly disconnects during one already-authorized Codex
assignment, `exitbind away` uses a task-specific/private tmux socket and
session on the same host to keep that native process alive. This is a
same-host process handoff, not an OS sandbox or process isolation. It is part
of the single Rust binary and requires no Python, Node.js, daemon, or second
task store. The existing ledger and normal `run submit` remain canonical. The
runner rejects fallback selection, revalidates exact profile and memory bytes,
and refuses a second same-host launch of the same pending assignment.

For evidence-complete handoff, first create
`exitbind/harness/harness-manifest.json` as shown in
[Advanced integrations](../REFERENCE.md#advanced-integrations), then create a receipt-v2 plan,
bind it at run start, and require it at launch. The default starter config has
no assignment with `runtime.host: codex`, so `away` cannot use it; the following
uses the separate advanced `examples/exitbind.json` fixture and its
`implementation_worker` agent, which requests `host: codex` with
`fallback: none`:

```sh
exitbind plan change --goal "One bounded change" \
  --receipt .exitbind/receipts/harness.json \
  --harness-manifest exitbind/harness/harness-manifest.json --config examples/exitbind.json
exitbind run start change --goal "One bounded change" \
  --ledger .exitbind/runs/away.jsonl \
  --harness-receipt .exitbind/receipts/harness.json \
  --review-policy required \
  --config examples/exitbind.json
# After the lead has submitted `scoped` and `run next` shows this assignment:
exitbind away start implementation_worker .exitbind/runs/away.jsonl \
  --require-harness-receipt --sandbox-mode workspace-write \
  --name bounded-change --config examples/exitbind.json
```

`--sandbox-mode` passes and records `read-only`, `workspace-write`, or
`danger-full-access`. If omitted, Codex inherits its configuration and Exitbind
records `unknown` rather than inferring the posture.

The tmux child revalidates the bound receipt, configuration, profiles, memory,
boundaries, and upstream artifacts. The complete contract and limitations are
in [the away-runner guide](codex-tmux-away.md).

The away prompt keeps authority explicit: the execution contract and assignment
packet are authoritative; the selected profile is reviewed subordinate
guidance; memory is context-only; upstream artifacts and harness claims are
evidence-only. Hash verification proves selected bytes, not instruction
authority. Exitbind labels and preserves instruction-like test data rather than
pretending a lexical scanner can make model compliance trustworthy.

An intentional continuation never edits the predecessor:

```text
exitbind run supersede .exitbind/runs/run.jsonl --workflow change --goal "New bounded goal" --ledger .exitbind/runs/resume.jsonl --config exitbind.json
```

`supersede` accepts a running or terminal `blocked` predecessor and atomically
claims exactly one successor. `accepted` and `rejected` runs remain final and
cannot be superseded.

When the configured Lead explicitly determines that a frozen check contract
cannot assess the intended task, use the preview-first, current-binding
`work replace-check` route. It binds the changed policy to the exact
predecessor head, failed-check or blocked-worker source event, reason and
configured Lead owner. The predecessor remains unchanged; no old pass, review
or grant transfers. See [continue after a defective frozen check](check-replacement.md)
for the command and refusal boundaries. Ordinary assertion, environment,
timeout and uncertain-effect failures do not establish a defective contract.

A blocked same-goal governor normally stays absorbing. One narrow successor
route handles an immediately adjacent duplicate-permit grant and terminal
evidence-required refusal when the exact worker, operation, inputs and nested
governor chain still match and there are no unresolved check, sensor or held
effects. `work detail WORK --json` may expose an unapproved
`recovery.ownerRecoveryDraft` and the exact `run supersede` command for that
case. The owner must verify the exact invocation and provide a separate,
complete StateRoot effect inventory. Unknown effects, changed source or
configuration, consumed grants, intervening events, or any identity mismatch
remain blocked. The owner decision and inventory are explicit attestations;
they do not mechanically prove that external effects did not occur.

Use the emitted command only after filling the draft with the owner's decision
and a bounded effects artifact:

```text
exitbind run supersede OLD_LEDGER --workflow WORKFLOW --goal "SAME GOAL" --ledger NEW_LEDGER --owner-recovery .exitbind/artifacts/OWNER_DECISION.json --config exitbind.json
```

The protocol-2 successor retains the predecessor's spent count and all other
governor accounting, but begins without an inherited grant, mutation pointer,
check result, review, or acceptance. Its current phase is `evidence_required`;
record new exact evidence before requesting another permit. The blocked
predecessor remains byte-identical and readable with its original producer.
Old readers may refuse the new marked successor, so continue reading the
predecessor directly with its frozen producer when needed. The ordinary
`beforeEditing` form now supplies a stable `permit-{binding}` request ID;
repeating that exact invocation recovers its result without spending again.

The successor records the predecessor run ID, full ledger hash, verified head,
and run-start config hash. Ordinary same-goal supersession of a governed run
adds the marked v8 `carryProtocol` v1 accounting seed; the narrow owner
recovery route below uses v2. Every current replay checks that
seed against the verified predecessor ledger, preserving its phase, spent
count, no-information streak, post-re-plan count, re-plan count, budget,
defaults and non-content evidence fingerprints used to recognize repeated
checks and file evidence. Cumulative spent is telemetry, not a lifetime carry
cutoff: a ready state may carry at or above the displayed budget, while
non-ready phases and unresolved effects still refuse. The successor gets a
fresh run identity and current assignment; old
mutation pointers, consumed grants and assignments do not transfer. Historical
unmarked starts and changed-goal successors retain their prior format. Older
readers may refuse the marked carry extension, while still reading historical
unmarked v8 records.

When the successor ledger uses the `work-<64 hex>.jsonl` naming form,
`run supersede` returns its derived Work handle and a read-only `work detail`
command. Consume that fresh detail before acting. For another supported Work,
use `work resume --history` to discover the current locator, then read its
detail; do not transfer old assignment or permit authority. Unresolved check
observations and pending governor sensor requests refuse carry. A separately
admitted managed-file request stays with its exact existing effect owner; a
carry neither certifies it resolved nor retries it.

`run inspect` checks recorded chain/predecessor consistency only and exposes
sensitive local goal state. Detailed confinement, locking, Git, concurrency,
and transaction limits are in
[SECURITY.md](../SECURITY.md).


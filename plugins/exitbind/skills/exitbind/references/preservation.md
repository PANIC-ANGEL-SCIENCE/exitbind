<!-- exitbind-managed-skill:v1 -->

# Preservation detail

Load this when accepted behavior has to survive an implementation. Everything
here ships with Exitbind: a project that uses it needs no separate preservation
install, checkout, or second instruction source.

Exitbind owns authority, evidence, and acceptance. Preservation contributes
evidence inside that lifecycle; it is never a second acceptor and never a
second ledger.

## Two axes, neither inferred from the other

| Axis | Values | Meaning |
| --- | --- | --- |
| Route | `INLINE`, `FORMAL` | How much lifecycle the work needs |
| Quality | `FULL`, `ECO`, `MODE-UNBOUND` | The assigned quality and cost mode |

`FULL` is an assignment, not an achievement, and `ECO` is not a failure. Never
derive either axis from a provider or model name, a host reasoning-effort
label, a minimizer setting, a banner, or a passing test.

Standing policy: **every `FORMAL` task is assigned `FULL`.** For `INLINE`,
assign `FULL` when consequence or uncertainty is material and `ECO` otherwise.
Do not ask the owner to pick a mode when this policy already resolves it; meaning,
scope, permissions, publication, and irreversible effects remain their decisions.

`MODE-UNBOUND` means the required assignment is not established. It is an honest
report, not a default to hide behind.

## Choosing the route

`INLINE` preserves meaning without ceremony: state the accepted behavior in a
sentence or a few internal bullets, name only the distinctions materially at
risk, and verify them in proportion to the risk. It needs no governed run, no
separate worker, no artifact file, and it must never acquire a percentage or a
fabricated preservation claim.

Use `FORMAL` — an Exitbind governed run carrying accepted preservation
requirements — when the configured workflow requires it, the owner asks, or:

- a persisted schema, migration, public interface, or compatibility guarantee changes;
- authority, permission, trust boundary, or memory rights change;
- effects are destructive or irreversible, including publication;
- lifecycle states, role identities, or evidence strengths could collapse into one;
- raw evidence would be replaced by a summary without traceability;
- distinct contexts or identities could merge into one global representation;
- this is the first implementation after meaning-rich exploration and the
  protected meaning is still unresolved;
- two equally faithful mechanisms would close different accepted options.

A subject area does not escalate work by itself, and open exploration must not
be forced into accepted meaning early. Tiny, obvious, reversible work stays
direct.

## Accepted meaning before implementation

The owner or an explicitly authorized lead accepts meaning. A model may
normalize, deduplicate, and propose clarification; it may not infer acceptance,
choose among alternatives, widen scope, or promote evidence into instruction
authority. Existing public behavior, persisted data, security boundaries, and
compatibility commitments remain constraints to verify even when the request
does not restate them.

Freeze each protected item with its own identity and source:

```text
requirement id   a stable short id, reused by every later reference
requirement text one sentence, observable, falsifiable
source           who accepted it, and where
check            the command that would fail if it were broken
non-goals        what this increment deliberately does not change
uncertainty      what remains open, and who decides it
```

Later explicit decisions by the same or a higher authority supersede earlier
ones; record what was superseded instead of rewriting it. A blocking accepted
requirement cannot be moved into a remainder or a summary because it is hard.

## Carrying it through an Exitbind run

Start the run with the frozen requirements:

```sh
exitbind work begin change --goal "..." \
  --check-command "YOUR_FUNCTIONAL_CHECK" \
  --preserve-requirement ID:TEXT \
  --preservation-check-command "YOUR_PRESERVATION_CHECK"
```

Exitbind then requires a functional check policy, refuses acceptance while a
requirement's check is missing (`preservation_missing`) or failing
(`preservation_failed`), and binds each result to the exact subject and tested
inputs. A passing aggregate command is not evidence for every requirement: give
each requirement a check that actually discriminates it, and never substitute
`true`.

`work next` returns the resolved assignment in
`humanHelp.preservationAssignment` — route, quality, and what resolved them.
Carry that assignment to any child agent you dispatch. A packet that arrives
without exactly one assignment, or with conflicting assignments, is refused
work rather than run on a guess. Exitbind records the assignment; it does not
enforce a host's own quality setting.

Do not create `.holytail/accepted.md`, `.holytail/check.md`, or any parallel
authority file in a configured project. The run state already holds the
accepted requirements, the evidence, and the acceptance.

## Read-back after implementation

Compare the exact current result against the same frozen requirements, item by
item:

```text
requirement id   status: preserved | broken | untested
evidence         the check event, its acquisition, and what it does not prove
notes            anything observed but unverified, and what would settle it
```

Report untested requirements as untested. Functional tests passing while a
declared invariant is broken is the case this whole path exists for.

## Evidence labels

| Label | Means | Does not mean |
| --- | --- | --- |
| `command_observed` | Exitbind ran the requirement's check itself and recorded the result | that the output's meaning was independently judged |
| `agent_declared` | the host reported a result Exitbind recorded | that anyone observed the command |
| `hook_observed` | a session hook ran | that anything was selected, activated, or complied with |

None of them is independent verification. Recording a declaration in a ledger
does not promote it. Observing a command's execution is not the same as the
truth of every claim about its output.

## Roles

The worker implements and reports `completed` or `blocked`. The reviewer
evaluates the exact current result and returns `approved`, `rework`, or
`blocked`. The lead alone changes scope, supersedes a goal, or records
acceptance. Use the host's native isolation when it is actually available and
observed; a role name or a fresh prompt is not enforcement. When required
isolation cannot be established, report that specific condition instead of
relabelling self-review.

## Currentness

Evidence stays bound to the accepted requirement, the exact result, and the
tested inputs that existed when it ran. A changed result, checker, or
requirement invalidates the evidence that depended on it. Historical acceptance
remains history: it never authorizes new work. Before skipping work a saved
packet lists, run `exitbind work validate WORK --packet FILE` and skip only on
`usable`.

## v0.22 context, checkpoint, sensor, and recovery

The canonical run reducer remains authoritative. A v0.22 context object is a
projection with a digest; it is valid only when its run identity, exact subject,
tested-input identity, ledger head, and digest match a fresh projection. The
`obligations` and `evidence` fields carry exact requirement text, checker
identity, target/check event hashes, acquisition, result, and provenance.
Every omitted ledger event or artifact is reachable by an `expansions` entry
with an exact hash/path reference. A summary is not evidence and never replaces
those references.

The current context emits one bounded `ledger_history` reference. Pass its
opaque digest `id` string to `exitbind work expand WORK REF` to retrieve
the exact lower-case hexadecimal ledger bytes. Event and observed-check-log
references are resolved against the same ledger snapshot and state-root
artifacts; stale heads, changed bytes, traversal, symlinks, cross-work
references, and malformed identities fail closed. Observed v8 checks retain
separate stdout/stderr raw artifacts, each capped at 1,048,576 bytes; reported
checks and historical versions do not claim captured logs. Expansion does not
capture arbitrary host logs or dereference an unpersisted reference.

Before each mutation unit, call `exitbind work permit WORK ASSIGNMENT
--operation OPERATION` and mutate only when it returns `allowed=true`. This
cooperative action revalidates the assignment and tested inputs and appends
its governor event under the canonical run-ledger lock before returning
permission. Replay governor events in order. The marker's budget is the
bounded no-information-path default, while total mutations remain monotonic
telemetry, not a lifetime cap. Wrappers, comments, wording, subject hashes,
and evidence-equivalent replans consume the same trajectory; only a newly
verified exact artifact resets its consecutive streak. A completed worker
submission remains a separate lifecycle transition.

After two consecutive no-information mutations, use `work replan WORK
ASSIGNMENT --hypothesis TEXT` (or another material semantic field) before
continuing. The typed governor then permits one further no-information unit;
the next request appends one durable blocked/refusal event. To reset only the
consecutive trajectory, use `work evidence WORK ASSIGNMENT --artifact PATH`
with an exact current product/state artifact; artifact bytes are resolved and
revalidated under the ledger lock. Historical SHA-only telemetry is not reset
authority.

Sensor input/output is versioned and bound to run, subject, attempt,
checkpoint, and input digest, with at-most-once deduplication. Unavailable,
malformed, stale, duplicate, low-confidence, or optimistic results are inert.
A conservative high-confidence low-information result may stop earlier, but
can never grant READY, waive a check, reset/extend the hard limit, or request a
retry. Native-host interception outside the cooperative checkpoint remains an
explicitly unverified link.

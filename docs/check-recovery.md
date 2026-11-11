# Continue after a known-ended check could not save its failure

When a check ends but storage is unavailable, Exitbind can retain its admission
without a durable result. Freeing space does not itself authorize another
execution. Inspect the existing Work with its compatible reader and configuration.
Do not start a replacement Work to reset accounting.

In 0.30.1, the configured Lead can recover a retained storage-failure response
whose process group and capture readers ended, whose cleanup is complete, and
whose partial capture bytes still exist. Preview first:

```sh
exitbind work recover-check WORK --json --config CONFIG
```

For an eligible known-ended storage failure, the read-only response supplies
`ownerDecision`. Preserve its exact
recovery request binding, admission snapshot, earlier observer identity and timeout.
This binding is specific to this preview, with the `check_recovery_request_v1`
domain, current Lead assignment and original admission. A general Work-detail
binding cannot substitute for it. Canonical readers reconstruct the request
binding from the pre-recovery state as well as checking the actual ledger prefix.
After confirming the retained failure belongs to that attempt, set `approved`
to true, supply the Lead's reason, put the complete saved response bytes in
`response`, and set `responseSha256` to their SHA-256. Pass that complete JSON
object through standard input:

```sh
exitbind work recover-check WORK --apply --current-binding BINDING --json --config CONFIG < APPROVED_DECISION.json
```

The decision is limited to 32 KiB. It binds the exact ledger prefix, configured
Lead, admission, original inputs, check policy and retained response. Each
partial capture must match its admitted operation, path, size and digest.
Changed inputs, missing captures, stale decisions, live or uncertain termination,
and incomplete cleanup refuse without launching a check. A lost apply reply can
reuse the identical decision only at its current recovered head; later repair
or another transition makes that replay unavailable.

Recovery records one **Lead-reported failed observation**. A saved process exit0
does not supply passing check evidence when its observation was not committed.
The decision is not an authenticated host observation: the configured Lead must
report the actual retained facts. Local records remain tamper-evident rather than
tamper-proof against their filesystem owner.

Follow fresh Work detail after recovery: the Lead decides repair, then the new
worker obtains its ordinary permit, returns its result, and completes fresh
checks, required review and acceptance. Recovery supplies no permit, check
credit, acceptance or accounting reset.

An ordinary Work's known-ended observed check can become stale when tested
inputs change. Follow current Work detail: a pending Reviewer may only rework or
block, never approve stale evidence; marked runs route rework through a real
Lead disposition, while legacy runs return to the same-Work Worker. That new
Worker result needs a new check, fresh review and Lead acceptance, without
replaying the old admitted check or resetting accounting; quarantine verification
remains one-check with no ordinary repair, and unknown termination stays blocked.

The distinct v8 recovery event requires a compatible 0.30.1 reader for the
affected Work's remaining lifecycle. Keep its original binary/configuration
and all unrelated Work pins. Older readers refuse this action without changing
saved bytes; switching a selector does not downgrade the record. If the complete
failure response or known-ended evidence is unavailable, this recovery route
leaves the outcome unresolved. The distinct lost-caller report below can record
a configured Lead's decision without treating it as a recovered observation.
Source changes, private launchers and hand-edited ledgers are not consumer steps.

For a durable failed observation with unknown termination whose Work is already
blocked, the `work recover-check` preview returns `historical_unknown_termination`.
This is an immutable diagnostic: it identifies the original admission, failure
and blocked head, reports the recorded process facts and separately describes
current input availability. It exposes no captures, recovery decision or apply
command. Changed source does not prevent this historical inspection. `--apply`
refuses; the original Work remains blocked with no check credit. A later process
listing cannot establish the missing original execution identity.

## Report a lost observation caller

When the observation caller is gone but its native outcome was not recorded,
the configured Lead can inspect a read-only preview:

```sh
exitbind work report-lost-caller WORK --json --config CONFIG
```

The preview supplies an `ownerDecision` and an exact apply route. Set `approved`
and a reason, and provide one to four `evidence` objects with `source`, the
retained UTF-8 `content`, and its exact `sha256`. These are Lead-supplied
incident records, not a native check response. Each text is at most 20 KiB and
the entire encoded decision is at most 32 KiB. Send the complete decision on
standard input using the emitted binding and route. Preview changes nothing.
The decision also binds the reporting executable's digest and producer; these
identify its bytes and claimed build metadata, not authenticated source or a
provider. The earlier admission's producer and observer digest remain separate.

New Linux admissions record hashes of host, boot and PID namespace context. A
report requires that available recorded context match the current context and
that the original caller be locally identifiable as ended. An unavailable or
mismatched context, matching live caller, missing start marker or uncertain
lookup refuses. The hashes are comparison aids, not authenticated identities.
On older admissions with no context, the Lead must explicitly set
`legacySameOriginAttested` to true and provide retained incident evidence. The
old host and boot remain historically unverified; an inaccurate Lead assertion
cannot be disproved from the old ledger. An attestation never overrides a
recorded mismatch. Hosts unable to supply the context cannot report a loss, and
the automatic dead-caller closure remains unavailable for their new admissions.

Use a compatible reader for a Work as soon as its admission records the new
observer context, before any lost-caller report is applied. The report event and
work-only protocol-3 marker also require compatible readers. An older reader
refuses these shapes, and a version number alone does not establish support;
retain and select the exact compatible executable for the affected lifecycle.
Admissions without the optional context retain their existing meaning.

The new event records `configured_lead_reported` and preserves the original
admission, producer and accounting. Execution, descendants, capture and cleanup
remain unknown. The report starts nothing, sends no signals, and gives no check
credit, mutation permit, retry, repair or acceptance. The same exact committed
decision can be replayed while its event remains current; changed or stale
decisions refuse. Use a separately reviewed isolation plan and the existing
quarantine route before any independent verification. An accepted implementation
Work in another worktree does not substitute for the same-root plan Work.

On Linux, new observations use a fresh exclusive observer process to own and
reap their checker descendants, including children that change session. The
caller never becomes a process-wide subreaper. Exact checker status and raw
streams remain separate from the private termination proof. The observer waits for normal background children to end within the observation
budget and sends no cleanup signal on that normal path. Timeout cleanup signals only currently owned children, remains bounded
and remains a failed observation even when all children are reaped. Missing or
incomplete proof keeps termination unknown. This does not retroactively resolve
an older unknown execution or provide a sandbox against hostile processes.

## Verify a separate artifact while preserving an unknown execution

An eligible Work whose current observation is durably failed with unknown
termination or has a configured-Lead lost-caller report, including a blocked
Work, can receive a separately authorized quarantine verification. The original
Work stays unknown and uncredited. This route does not retry, repair, supersede,
or reset the original execution. An unreported running observation is
ineligible. Earlier ended check history stays intact; eligibility is determined
by the current admission.

First obtain current acceptance of a separate feature Work that checks and reviews
the verification plan. Its Worker artifact is a JSON object with
`kind: "quarantine-verification-plan"`, `protocol: 1`, and a `plan` object containing
`environment`, `effectInventory`, `artifactSha256`, `inputsSha256`, `command`,
and an explicit typed `basis`. The accepted plan's basis is frozen as the new
verification Work's basis; the original Work's legacy basis is preserved as-is
and is never retrofitted.
The artifact hash identifies the exact bytes to return in the new verification;
it is distinct from the plan report's own hash. The environment and effect
inventory must describe the actual permitted execution. Their hashes are
commitments to that reviewed plan; Exitbind does not create an operating-system
sandbox from these declarations.

The configured Lead previews the separate admission using the original Work's
current binding:

```sh
exitbind work quarantine OLD_WORK --feature-config PLAN_CONFIG --feature-work PLAN_WORK --artifact-root state --artifact FROZEN_ARTIFACT --request-id ID --reason REASON --current-binding BINDING --json --config ORIGINAL_CONFIG
```

Use the emitted apply command with the same executable and configuration. The
plan Work may use a different configuration, but must share the product and state
roots and have compatible Lead and reviewer identities. Stale source, changed
artifacts, unaccepted plans, retained children, or conflicting Goal admissions
refuse the start. Preserve the original binary, configuration, ledger, captures,
and accounting.

To verify only the artifact without selecting or changing a native Goal, add
`--work-only` to the preview and follow its emitted apply command. This mode
does not claim whether the original Work has a Goal association. It keeps every
existing Goal journal unchanged and requires the same current configured Lead,
same roots, independently reviewed current plan and exact frozen inputs. Its
marker records `associationMode: "work_only"`; the original admission still
reserves only one verification across either mode. A work-only verification
cannot become a native Goal member or supply a Goal quarantine residual, even
if a Work is linked later. It gives no original check credit or repair authority.
An ordinary explicit result reference remains distinct from membership relief.
Work-only does not accept an owner-configuration option or a follow-up check.

If the original native Goal belongs to a separate project configuration, provide
`--owner-config OWNER_CONFIG` on the preview. It must resolve to that existing
fixed Goal owner in the current native session; Source keeps its original
configuration. The emitted apply command binds the owner configuration's digest
as well. This does not establish a new Goal or change its owner.

Admission reserves one stable verification Work for the original check admission.
Changing the request ID, artifact, or reason supplies no second allowance. After
a lost reply, refresh the original Work's current navigation and binding, then
replay the exact request; it reads back the existing Work without another start.
A recorded start whose Goal link failed exposes that existing Work's link recovery.

Follow the new Work's detail. Its Worker acknowledges the frozen artifact without
implementation changes. It allows one actual observed check, required independent
review, and a distinct Lead acceptance. It offers no mutation permit, replan,
reported-check substitution, check replacement, or repair allowance. An admitted
check is not rerun after an interrupted result; a committed identical request
can recover its recorded event without another execution.

Once a Goal-bound separate verification is currently accepted, the original
native Goal owner can preview `goal quarantine OLD_WORK --source-config ORIGINAL_CONFIG
--verification-work NEW_WORK --request-id ID --reason REASON --json`, then use
its emitted apply command. The new verification must be a real member of that
Goal. The original Work may be the original member or its exact validated
same-Goal successor; the residual preserves the original member and its actual
unknown endpoint.
The Goal retains the original as `quarantined_uncredited`, separately from
historical fixtures and current accepted results. Earlier accounting, unknown
facts and the observation's provenance stay intact. Every other original
outcome, admission, currentness check, and closure gate still applies.

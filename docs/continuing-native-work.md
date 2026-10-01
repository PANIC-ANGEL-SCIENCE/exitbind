# Current context, mediated file writes and the quiet hall

These additions extend the existing Work path. Start with `work next WORK
--full`, follow its current assignment, and use `work act WORK` for a native
Codex action. Checks, independent review and explicit Lead acceptance still
have their own results.

## What a native assignment receives

Before a provider starts, Exitbind delivers the current configured role,
project identity and location, Work and assignment identities, requested
native binding, complete current `AGENTS.md` and `CLAUDE.md` bytes when present,
and complete sources for eligible, accepted, opted-in role memory. A new Work
gets a fresh projection; its predecessor's approval and mutation grant do not
carry over. Work identity, role identity and provider session are separate.

Memory remains disabled unless configured. Revoked, expired, scope-ineligible,
changed or unsafe sources are excluded or refuse launch through the existing
memory checks. Ambient transcripts are never collected. The provider receives
the selected bytes; this observation does not prove that a model followed them.
Memory is context, and cannot expand the assignment's authority.

Delivery refuses oversized content rather than silently shortening it: each
rule and memory source is at most 16 KiB, at most eight memory items are
included, each memory reference is at most 8 KiB, and the combined current
context is at most 64 KiB. Narrow an explicit selection or split an oversized
source before trying again. A configured non-Codex runtime cannot be launched
through the Codex executor.

## Replace one UTF-8 file through the product boundary

The current worker can replace a file within its declared write paths after
receiving a mutation permit. Use the exact current assignment from `work next`:

```sh
exitbind work permit WORK ASSIGNMENT --operation source-update
exitbind work write WORK ASSIGNMENT src/example.txt \
  --operation write-example-v1 --expected-sha256 absent < replacement.txt
```

For an existing file, replace `absent` with the SHA-256 of its current bytes.
Standard input is the complete UTF-8 replacement, at most 256 KiB. The command
refuses a different file state, a stale or completed assignment, a missing
current grant, an out-of-scope path, symlinks, and protected configuration,
profile, evidence and host-control paths. It replaces the name atomically and
preserves ordinary Unix permission bits of an existing file, excluding set-id
bits. A task ledger lock fences assignment transitions during the replacement.

Keep the operation ID for this exact request. An identical completed retry
returns `replay: true` and `effect: no-change` only while the recorded file
bytes still match. A changed request under the same ID is refused. An admitted
operation whose completion is unknown is also refused: inspect the actual file
and resolve the uncertainty before choosing a new operation. A missing native
process does not authorize another effect.

This mediator controls this file-replacement class. Native tools outside it
retain their host permissions. It does not intercept arbitrary shell writes,
network requests or remote APIs, authenticate the local operating-system
principal, or guarantee exactly-once external effects. A local principal able
to modify control state is trusted; this is not an OS sandbox.

## Read the quiet hall

```sh
exitbind work world WORK
exitbind work world WORK --plain --reduced-motion
exitbind work world WORK --export --json > world.json
```

The static terminal renderer uses six motifs. Each active marker comes from
the corresponding current record, without changing the records that decide
checks or acceptance. `--plain` has the same status and markers; all rendering
is static, so reduced motion requires no alternate animation. Export includes
renderer status and motif state only, excluding local Work/session markers,
paths, source text and logs.

| Motif | What activates it |
| --- | --- |
| The same door | A confirmed receiving child after a binding change, or an actual saved native return/replay from the same Work |
| A passing black cat | The current `work act` response replayed a recorded native result without another provider execution |
| A distant payphone | A current bound child was claimed or its native result was recorded; binding alone is insufficient |
| A low-resolution poster | A canonical blocked/refused state, unresolved failed/uncertain operation, or required owner disposition |
| An old poster becomes a signpost | An explicitly applied finding has later current observed passing evidence |
| The exit sign | The named whole goal has explicit Lead closure and its required evidence is still current |

Add `--themed` to `work next` or `work act` to include the same world projection
in that JSON response. The black cat belongs to the actual replay response;
polling `work world` later does not invent another replay. A slice's passing
check or accepted run does not light the exit sign for an unresolved whole
goal. Missing or stale support suppresses the corresponding marker. Ordinary
unadorned Work responses retain their existing shape.

The world renderer writes no authority or evidence. The surrounding Work
status path may refresh its existing replaceable presentation memo; that memo
is outside tested inputs. No provider call or artwork cache is needed.

## Record actual finding application

Reading a diagnosis does not establish reuse. The bound Lead may record a
`reuse` through `work record WORK`, using the current goal and binding revisions
from `work continuation WORK`:

```json
{
  "action": "reuse",
  "expectedRevision": 4,
  "bindingRevision": 1,
  "operationId": "repair",
  "diagnosisRevision": 4,
  "resultEventSha256": "LATER_OBSERVED_CHECK_EVENT_SHA256",
  "conditionsSha256": "CURRENT_CONDITIONS_SHA256"
}
```

The diagnosis must already exist with retained evidence and an observation,
cause or verified-repair class; a hypothesis is insufficient. Its result must
be a later passing observed check in the same Work, with verified artifact
bytes and current acquired configuration, tested inputs and conditions.
Reusing the original check, a synthetic result, missing evidence or stale
conditions is refused. `work continuation WORK --section reuses` separates the
retained relation from its current applicability. Changed inputs, conditions
or unavailable evidence suppress the signpost.

Application is explicitly `lead_reported_application_with_observed_check`:
the later check supports the application claim, but does not independently
prove the semantic cause. These features provide local behavioral evidence;
they do not establish comparative superiority or external-user adoption.

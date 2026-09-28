# Reviewer evidence handoff: finish the delivered patch

Author: GPT-6-Astra-Pro_ChatGPT  
Date: 2026-09-28 13:29 KST (UTC+09:00)  
Revision: 1  
Status: Source-derived findings to reproduce; no repair result asserted

## Keep the existing design

At `c2091b9`, `src/host/assignment_evidence.rs` owns bounded evidence read-route
presentation; assignment selection supplies the validated context and
`src/host/runtime.rs` appends it. Keep this division and the existing exact
`work expand` mechanism. Do not replace it with copied ledgers or Lead summaries.
A passing check remains evidence for the reviewer's own judgment, not approval.

## A. The emitted command must preserve its execution context

The inspected formatter emits a caller label plus `work expand WORK REF` or
`work next WORK --full`. It does not carry the loaded configuration or the
selected executable. The hook can discover an external local-project config,
while ordinary CLI loading uses an explicit `--config` or its default filename.

The current test extracts WORK/REF from the displayed command, then its helper
selects the candidate binary, supplies the working directory, and appends
`--config exitbind.json`. This tests expansion but can mask a broken delivered
command. This is a static finding; reproduce the local-config case first.

Repair the smallest command-delivery owner. Reuse existing structured read
commands and path-rendering rules. Preserve the correct executable, config,
project and declared working directory. Do not require the reviewer to infer
missing flags or change global PATH. Keep private-path redaction in human
presentation distinct from the authorized executable route; never silently
redact a path into a different command. Prefer existing argv representations
with an exact declared context, with safe shell rendering where needed.

Acceptance:
- Portable/default and local/external-config fixtures both work.
- Execute the product-emitted route exactly as delivered in its declared
  context. The assertion helper must not secretly add flags, replace the binary,
  or correct the working directory. Explicitly delivered context may be used.
- Cover spaces and non-ASCII path text, a stale same-name binary earlier on
  PATH, and invocation from the documented non-root location when supported.
- Returned record/log bytes match the expected event. Cross-work and stale
  references still refuse. Do not broaden read authority or move private
  configuration into the public repository to make the test pass.

## B. Requirement-bound checks need their own actual check routes

`src/context.rs::evidence` already carries executed check references for both
ordinary checks and requirement-bound preservation targets. The inspected new
formatter handles only `kind == "check"` in its detailed branch. A preservation
item falls through to a target-event route instead of showing its actual check
record and log. The full packet remains available: this is incomplete first
handoff, not deletion of canonical evidence.

Reproduce with a preservation-enabled fixture, then fix the formatter without
inventing support or flattening every event into an ordinary check.

Acceptance:
- Both check kinds expose the actual check-event and log routes when present.
- Preserve kind, requirement identity, status/currentness and checker identity.
  A target event is not the event proving execution of its check.
- Failed, missing and unavailable-log conditions remain explicit. Never invent
  a log reference, omit an adverse state as if it passed, or infer approval.
- Preview limits apply to display, not truth. Decide absence from the complete
  canonical set, not just the first eight entries. Say "not shown" when the
  preview omits evidence and keep an exact full-packet route.
- Cover an over-limit mixed set with evidence beyond the preview. A preservation
  entry must not disappear merely because several other check targets came first.

## C. Preserve bounded context without discarding required identity

The host currently caps the serialized child context. Evidence lines share that
budget with profile and perspective bytes. Add one adjacent boundary regression:
use an otherwise supported near-limit profile/perspective plus the new preview.
When space is tight, shorten the optional evidence preview and retain a usable
read route before declaring required profile acquisition unavailable. Do not
silently truncate profile/perspective bytes while calling them complete. Keep a
truthful refusal for a profile that is itself outside the supported limit.

This is a budget-boundary test and a local repair if reproduced, not a new
pagination service or permission to increase all context limits. Keep the
no-evidence path, legacy behavior and synchronized installed skill copies valid.

## Source and proof boundary

Inspect `src/host/assignment_evidence.rs`, `src/host/assignment_context.rs`,
`src/host/runtime.rs`, `src/context.rs`, `src/config/mod.rs`, `src/cli/mod.rs`,
and `tests/reviewer_evidence_handoff.rs` at the actual candidate.

Existing tests and CI establish program behavior, not that a native Claude
reviewer used the new route. Retrieve any retained native observation under the
existing access boundary; missing evidence remains unverified. Card 03 defines
bounded validation without purchasing another run.

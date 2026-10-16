# Carry a specialist's qualified experience to a new task

Ask your Lead to use a suitable configured specialist and its relevant verified
experience. The Lead supplies the actual purpose, role and available native
host. Legacy selection uses literal task eligibility; the opt-in
[approved current-task context](project-lessons.md#select-against-approved-current-task-context)
lets the Lead select exact qualified resource versions after assessing their
meaning. Semantic suitability stays with the Lead. Small routine work can
remain direct.

```sh
exitbind project route --task "Aggregate invoice records" \
  --purpose record --role worker --host codex --json
```

One eligible agent is selected automatically. Multiple matches return
`lead_choice` without loading their resources; the Lead can repeat with
`--agent NAME`. No match returns `lead_held`. An incompatible primary host stays
excluded; this command does not select a runtime fallback or another reviewer.

The route returns a selected `snapshot`. It contains the
exact configured identity, purpose, profile, requested runtime, unchanged declared
boundary, and at most three complete applicable experience sources. A profile
over 4 KiB uses its complete supported `profile` read route; profiles over
64 KiB refuse. `snapshotId`
identifies those projected bytes. This read-only path starts no model, installs
no resource and changes no running Work. Projection, actual native delivery and
observed use require separate evidence. Repeated reads of unchanged inputs return
the same snapshot and append no event. The host remains responsible for avoiding
duplicate execution after a lost reply.

## Author once, qualify before reuse

At project setup the owner selects a bounded policy; the Lead then handles routine
drafting and qualification without asking the human to manage individual files.
Use the existing configuration and [memory lifecycle](../REFERENCE.md#memory-governance)
with an exact scope `specialist.AGENT_NAME.v1`:

- The specialist has proposal rights and read rights for its own scope.
- A separate reviewer holds its review right; the configured Lead holds promotion
  and revocation rights. The author has no review or promotion right.
- Opt-in memory includes the scope in `protocolScopes`, with bounded `maxItems`
  and `maxBytes` and the reader's compatible `crossContext`.
- Trial commands, private output roots and retention are selected upfront.
  Skill text supplies no additional command, spending, reader or host permission.

After a useful permitted task, the specialist writes a concise attributable
observation or inactive skill source inside its approved private project scope.
Use `exitbind memory form --scope specialist.AGENT_NAME.v1 --json` for the
current author/project identity, typed shape and bounds. Fill its placeholders
from the actual task and distinct trial evidence; the form writes nothing.
Run `exitbind memory form SOURCE --scope specialist.AGENT_NAME.v1 --json`
to validate the exact inactive source using the delivery parser. Its `validation`
records source bytes/hash and current author/project/config/profile bindings.
This read-only preparation changes no lifecycle and proves no trial, review,
qualification or task applicability. Guard file currentness is checked at actual
delivery. Corrected sources use a new immutable revision and ledger.
It runs relevant trials and an independent evaluator checks a different case and
inappropriate use. The Lead records the reviewer's actual opinion, then uses
the existing `memory propose`, `memory review` and `memory promote` operations
with an explicit expiry. Draft files and proposed/reviewed items stay inactive.
Proposal/review replies return `nextAction` for the authorized separate reviewer
and configured Lead. Multiple eligible reviewers remain an explicit Lead choice.
These events evidence the caller's declared acts, not authenticated human identity
or proven model independence.

The source is complete UTF-8 JSON, at most 4096 bytes, with these fields:

| Field | Meaning |
| --- | --- |
| `version`, `id`, `revision`, `kind` | Version 1; stable portable ID; bounded revision; `observation` or `skill` |
| `author`, `projectIdentity` | Exact configured specialist and current `project.memoryIdentity` from `project context --json` |
| `taskTerms`, `hosts` | One to four literal task substrings and compatible `codex`/`claude` hosts |
| `summary`, `preconditions`, `procedure`, `verification` | Attributable finding, applicability, procedure and meaningful verification; uncertainty stays explicit |
| `requiredCommands`, `evidence`, `guards` | Already declared exact commands, task evidence references, optional confined file/SHA-256 prerequisites |

`guards` contains zero to four objects, each with exactly `path` and `sha256`.
Use `[]` for none. `path` is a nonblank confined relative project file path of
at most 256 UTF-8 bytes, without parent traversal. `sha256` is its expected
64-character lowercase hexadecimal digest; each guarded file is at most 4096
bytes. For example:

```json
{"path":"docs/current-contract.md","sha256":"0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef"}
```

Narrative authority or applicability restrictions belong in `preconditions`,
not guard strings. The public form lists every field's item/UTF-8 byte bounds.
Source validation checks shape and identity; lifecycle and actual qualification
remain separate.

This is a consumer format inside the existing generic memory source; it adds no
ledger schema. Contextual selection is a separate opt-in memory configuration
and uses the same lifecycle. It does not rewrite shared project lessons.
The source author must equal the proposal actor, an independent actor must have
reviewed it, and the configured Lead must have promoted it. Expiry is mandatory.

At the next task boundary the normal route obtains the eligible current version.
The fresh native session reads and uses the method on its different task, and
returns the useful result and exact source reference. Saved bytes or an
acknowledgement alone do not establish useful reuse.

## Currentness and ownership

Changed sources or profiles, unsafe paths, foreign project identities and
incompatible recorded policy refuse delivery. Expired/revoked items, unrelated
tasks, incompatible hosts, changed guarded bytes and unavailable declared commands
are excluded. Diagnostics disclose no excluded private item names or contents.
The complete route is capped at 16 KiB; excess resources refuse instead of being
silently truncated.

Concurrent sessions append separate candidates through separate memory ledgers.
Two accepted versions with the same stable ID cause a conflict, including when
one has become inapplicable. The Lead retires the predecessor through the existing
lifecycle before adopting a correction; there is no last-writer overwrite or
revival of a revoked item. A revoked source digest remains retired even if its
bytes are proposed from a different source path and ledger. This consumer detects conflicting adoption; the generic
memory writer does not provide an atomic expected-version transaction across
different ledgers. Reconcile a conflict before launching dependent work.

Local sources need no dotagents process. Existing native and `.agents` layouts
are preserved: this command writes none of them, and unsafe selected profile
symlinks refuse rather than replacing a custom layout. Shared project facts stay
in [Lead-owned project lessons](project-lessons.md).

Current Work recipient detail and native `work act` use the same experience
selector for the already assigned agent, role, task and actual requested host.
Generic memory routes exclude reserved experience and source aliases; ordinary
generic scopes and project lessons retain their own consumers. Administrative
`memory inspect` remains explicit lifecycle inspection.
Complete reserved JSON in Markdown quote/list/indented containers, fenced blocks
(including EOF termination), explanatory/code text, or bounded JSON object,
array and string envelopes is excluded too. Loose field names and nested ordinary
JSON without a complete reserved record remain ordinary guidance. Recognition
removes conventional container syntax and parses complete JSON values; it is
bounded content recognition, not semantic paraphrase
detection. Actual delivery enforces the existing total item/source-byte memory
budget across generic guidance, applicable project lessons and specialist
experience together. Overflow refuses delivery instead of dropping ordinary
references or granting a second allowance.

For a supported native child handoff, bind the receiving native session and use
the emitted `work child prepare` route with the current `--agent-type`. Specialist
experience alone requires the current named assignment, without a manually copied
route packet. The child context and managed start hook revalidate prepared
source/profile/configuration/lifecycle/guard bindings before presentation. An
unbound or unknown-host child receives no specialist content; a stale preparation
must be prepared again. A host without these hooks needs the supported native
Work execution path. Presentation, receipt and useful use remain separate facts.
Experience never changes frozen checks or rights, and no live refresh is claimed.

When contextual selection is enabled, use `memory candidates AGENT --task TASK
--host codex|claude --json` to discover only metadata allowed by the recipient's
existing read rights. Expand one candidate with the optional `ITEM_ID` before
assessing a paraphrase or a literal-match collision. Both operations remain
read-only and do not activate source text. The configured approved task context
must select the exact item, source and ledger-head digests with the assigned
agent's current profile, unique role and exact purpose. Empty or other-task
selections supply no guidance. Follow the common contextual form and bounds in
the linked project-lessons guide; do not invent a second qualification or
authority store. Native preparation and delivery retain their existing
currentness checks and carry the selected context identity. A source read or
route snapshot still does not establish actual native use or a net benefit.

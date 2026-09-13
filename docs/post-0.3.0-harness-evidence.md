# Post-0.3.0 harness evidence follow-up

Status: introduced as the opt-in receipt v2 path in 0.4.0 and hardened in 0.5.0;
not part of 0.3.0.

## Gap

Existing receipts and ledgers deterministically identify selected configuration,
profiles, requested runtime, declared skills, producer version/commit, and
submitted artifact bytes. They do not bind a host project/session to evidence
about which skill, perspective, Ponytail injection, or hook the harness
reported active.

In 0.3.0, a native agent may record those facts in its existing submitted
artifact with the levels `configured`, `presented`, `agent_declared`,
`hook_observed`, and `independently_verified`. The artifact hash preserves the
declaration without changing any persisted Soulmate shape. Missing levels stay
missing; presentation is never compliance proof.

## 0.4.0 decision

The implementation extends the canonical Soulmate receipt/evidence protocol
rather than adding another JSONL store. A version-1 harness manifest:

- binds a host-visible project and session identifier to the existing producer,
  config, profile, and runtime evidence;
- states one evidence level for each activation claim;
- keeps recording explicit and project-local;
- remains backward-inspectable and optional when absent;
- excludes prompts, transcripts, environment dumps, credentials, and hidden
  reasoning;
- treats OTel or Langfuse as optional mirrors of canonical Soulmate evidence,
  never as the authority.

`brief` and `plan` accept the manifest only when writing an existing receipt.
The receipt becomes version 2 and binds the fixed manifest path, exact manifest
SHA-256, hashed identifiers, and non-sensitive kind/evidence enums. Raw manifest
strings are omitted. Without the option, receipt version 1 remains unchanged.

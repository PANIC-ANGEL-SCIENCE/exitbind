# R14 closeout: achievable goal setup and responsibility ownership

Author: GPT-6-Astra-Pro_ChatGPT  
Date: 2026-09-28 13:29 KST (UTC+09:00)  
Revision: 1  
Status: Selected bounded follow-up; reported failures are not new reproductions

## D. Diagnose an incompatible whole-goal setup before late-stage recovery

Reported R14 case: an ordinary Work earned a current passing check, reviewer
approval and Lead acceptance. A separate continuation record still had three
unsupported requirements. The Work began without preservation policy, so its
ordinary check events had no requirement IDs. The requirement-bound observed
check route could not supply support under that frozen Work configuration.

The ordinary acceptance and the unresolved whole-goal claim were different
facts. Do not make them agree by relabelling old events, dropping requirements,
weakening support validation or treating an owner's release decision as evidence.

Inspect `src/session_goal/continuity/record.rs`, `src/run/observed_check.rs`,
the relevant begin/goal-binding entry and existing preservation tests. Reproduce
in a disposable fixture without importing an operational ledger.

### Smallest selected change

At the earliest entry that knows both the named requirement set and the Work's
frozen capabilities, detect the missing or incompatible support route. Reuse
existing policy/currentness checks; do not parse arbitrary prose to invent a
policy. Return the affected requirement IDs, the precise missing prerequisite,
whether any state was written, and a supported next action.

An unbound draft goal may exist. The product must not silently present it as an
executable, closable continuation of an incompatible Work. Reject an incompatible
binding before mutation, or explicitly mark a draft as unbound with a recovery
route consistent with the existing contract. Do not silently change existing
persisted-event meaning; a required schema/authority change exceeds this patch.

Do not enable FORMAL preservation for every small task. Do not add policy to an
already-started Work behind the user's back. If the operator has already selected
requirement-bound verification, the normal entry should reveal the supported
preservation-enabled setup without requiring an internal-protocol lesson.

For an existing incompatible record, return a read-only explanation. A new,
explicitly configured Work may earn new support through the already-supported
binding route; historical check events are not retroactively converted into
requirement evidence. If no such binding route exists, say so and record the
narrow design question instead of inventing a migration in this patch.

### Decisive regression set

1. Ordinary Work, no requirement policy: ordinary check/review/acceptance remains
   valid; those events do not satisfy unrelated requirement-bound support.
2. An attempt to bind explicit requirements to that incompatible Work yields the
   actionable early diagnostic, not a silent late-stage dead end.
3. A correctly configured new preservation-enabled fixture earns applicable
   observed support and can reach its legitimate readiness state. A passing
   ordinary check alone does not satisfy this positive case.
4. A stale or wrong-requirement check still refuses. A whole-goal room never
   lights from accepted sub-work or a deliberately unresolved continuation.
5. A refused operation leaves prior ledger bytes unchanged; any intentional
   draft write is reported with its actual state and recovery route.

These close the diagnosed setup/diagnostic family. They do not require repairing
historical operational sidecars or building the rest of W2/W3/W5/W6.

## E. Extract the R14 policy added to the large run facade

R14 added `validate_evidence_governor_identity` and carried-mutation preparation
logic to `src/run/mod.rs`. The file grew within its older architecture ceiling;
passing that absolute ceiling does not establish responsibility separation.

Move only this changed evidence/identity responsibility to an appropriate small
owned module, reusing the existing module tree. Keep reducer/facade calls thin.
Keep serialized events, error meaning, input/subject/attempt checks, lineage and
replay unchanged. Do not move unrelated run functions merely to lower a number.

Preserve focused regressions for historical failed check -> changed inputs ->
passing check -> review -> acceptance, plus current evidence after Lead rework
and forged subject/nested-input rejection. Reuse existing tests where sufficient.

For every touched grandfathered production file, compare final size to its actual
size at `c2091b9`, not only to an old higher ceiling. It must not grow. Tighten the
checked-in ceiling for a successfully reduced touched file; never raise it or
add a new exception. New modules remain responsibility-owned and within the
existing 32 KiB rule. No arbitrary sharding or helpers dumping ground.

## F. Preserve the R14 gains rather than restarting the campaign

Do not rediscover already-covered R13 false-success cases without a contradictory
reproducer. Do not rewrite the personal agent catalog, remove dotagents, run new
token comparisons or expand the world motifs in this follow-up.

Retain native handles and a coherent repair owner. A timeout is not termination
and does not justify another worker or reviewer. Do not impose a universal agent
count or claim a cost improvement merely from fewer spawns.

Product-development agents must inspect one-step adjacent failures of these
specific boundaries; the operator is not the real-time invariant checker.
Preserve useful task progress through non-authority product friction, but never
manufacture acceptance, grant authority or whole-goal completion.

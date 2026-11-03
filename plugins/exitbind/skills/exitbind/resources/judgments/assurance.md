<!-- exitbind-managed-skill:v1 -->
<!-- exitbind-managed-bootstrap:v1 -->

# Assurance and claim-scoped verification

Use this reference for a bounded reliability, architecture, privacy, security
or verification assignment. Establish the exact artifact, claim, authority,
relevant trust boundaries and current evidence. Review the actual source and
observed behavior rather than a summary. Distinguish a demonstrated failure,
a plausible risk and missing evidence. Do not grant acceptance, permissions,
publication or an exception to an invariant.

For operational reliability, examine applicable failure handling, concurrency,
stale state, retry safety, recovery, rollback, observability and maintenance.
For architecture, trace accepted responsibilities and constraints through the
changed interfaces, storage and evidence. State a concrete counterexample,
impact, smallest useful check and uncertainty. Treat a passing build as a
build result, not proof of the whole user path.

For a mechanical claim, select a falsifiable input, expected observation and
the cheapest decisive existing check or focused probe. Exercise relevant
boundaries, invalid states, permissions and lifecycle effects. A simulator is
evidence only for the boundary it actually models. Report a counterexample or
the missing observation; do not silently repair the subject being verified or
claim that a simulated physical, radio or timing boundary was exercised.

For an invariant claim, obtain the approved invariant and source, then trace
the changed data and state across relevant observers, persistence, recovery,
derivatives and disclosures. Test the smallest prohibited state or transition
within authorized scope. Include relevant observers, access and record
cardinality, and applicable create, revoke, recover and delete transitions.
If the invariant or decisive evidence is absent, report the gap instead of
inventing a guarantee.

For a security or privacy claim, define the actor, assets, permitted effects,
observation boundary and success condition. Examine only authorized code and
controlled fixtures; preserve secrets and do not target external systems or
create persistence. Use a safe discriminating check for the relevant boundary.
Where the claim depends on repeated observations or combined information,
include the specified duration, applicable combinations and measurable success
condition. State the model and limits of the evidence; no finite local check
establishes a universal security guarantee. Use PASS, FAIL or INCONCLUSIVE
only for the named claim, and name the specific evidence or missing condition.

## Preparing an independent review

When preparing a packet for a selected review, identify the accepted contract
and exact subject: its base and revision, plus a digest of included changes and
any included or excluded untracked files. A commit alone does not describe a
modified worktree. Check that each material requirement, implementation and
falsifiable evidence, including relevant invalid and bypass cases, refers to that
subject and applicable environment. Identify the independent reviewer and the
scope of that review. Distinguish missing or stale evidence from an unavailable
artifact, conflicting accepted decisions or unestablished independence; name
the owner or observation needed to resolve each gap. Packet preparation is not a
correctness verdict, review approval or acceptance, and does not add a new gate
or require a second record when the existing route carries the facts.

## Interface and verification changes

When removing or changing an interface asset, trace relevant routes, templates,
styles, translations and dynamically named resources as well as direct imports.
Use the project's supported build or render path where available; a text search
alone does not establish what a user sees. For check or CI changes, compare the
actual local and selected CI toolchains, inputs, platform and temporary-file
behavior, including checkout and comparison-base differences when relevant.
Resolve the project's pinned formatter, linter and affected tests before using
them; a different installed toolchain is not equivalent. Prefer an early
decisive existing check, and reuse qualified evidence only for matching inputs.
Preserve invalid-state and final checks; do not loosen a budget or test merely
to obtain a pass.

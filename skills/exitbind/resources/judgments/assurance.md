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

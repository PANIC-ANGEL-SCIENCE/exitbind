# Release-boundary governance binding

Design follow-up for the gap identified on 2026-09-19: Exitbind correctly
recorded that required review had not happened, and a release could still
consume the affected change.

Status: **design only, not implemented.** No enforcement change accompanies
this document. It exists so the enforcement question is not silently expanded
into the provider-fallback change.

## The problem

The release path never consults Exitbind state.

Verified current facts:

- `.github/workflows/release.yml` triggers on `push: tags: - "v0.*"`. It runs
  `scripts/check-public-history.sh` (ancestry), an exact-SHA assertion, and
  `scripts/check-release-refs.sh` (version-string parity across manifests and
  docs). None of these read a run ledger, receipt, or acceptance decision.
- `grep -rn "exitbind work\|exitbind run\|exitbind check" .github/workflows/
  scripts/` returns nothing. No workflow or script invokes the governance
  surface.
- `tests/release_integrity.rs` covers version drift, README install commands,
  plugin manifests, and git parentage. It does not read run state.

So on 2026-09-19 a commit produced by a run that closed `blocked` reached the
`v0.22.0` ancestry, and no mechanical check objected. The ledger was honest;
nothing consumed its honesty.

Scope caveat, stated up front: **this must not make every arbitrary Git tag
depend on Exitbind.** Any binding must apply only to a clearly configured
release path. A general-purpose repository must remain able to tag without a
governance database present.

## The enforcement seam

The narrowest existing seam is the release workflow's own gates. The workflow
already fails the release when an assertion about the checked-out commit does
not hold (exact GitHub SHA; tag-at-main). A governance binding is the same kind
of assertion, so it belongs beside them rather than in a new system.

Candidate seam, in order of increasing scope:

1. **A configured-path gate in `release.yml`.** A new step, active only when
   the repository declares a governed release path, that refuses the release
   when the commit being released is covered by a non-terminal governed run
   for that same commit. This is the smallest change and reuses the existing
   failure mechanism.
2. **A receipt requirement at tag time.** Stronger: require a verified receipt
   for the exact commit before the tag is considered releasable. Broader
   compatibility impact, because it imposes a precondition on how tags are cut.

Option 1 is the recommended starting point.

## Threat model

In scope:

- A change produced by a `blocked`/`refused` governed run reaching a release.
- A release proceeding while the governing ledger for that commit says the
  required roles did not execute.
- Provider unavailability creating pressure to ship outside the governed path.

Out of scope (explicitly):

- An adversary with push access to tags who can bypass the workflow entirely.
  Server-side rulesets are the existing answer to that, not this gate.
- Proving the released code is correct. This gate asserts *which procedures
  ran*, not code quality.
- Repositories that have not opted into a governed release path.

Residual risk to name honestly: this gate can only bind the release path it is
configured on. The 2026-09-19 release happened through a tag push; a gate in
`release.yml` would have caught *that* route but would not catch a maintainer
tagging and publishing by hand outside CI.

## Compatibility impact

- Must be inert when no governed release path is configured, so existing
  repositories and the project's own history are unaffected.
- Must not alter `check-release-refs.sh` or `check-public-history.sh`
  semantics; it is an additional assertion, not a replacement.
- Must not require run state to exist. A commit with no governing run is not
  the same as a commit with a non-terminal one — the gate should refuse only on
  a positively non-terminal governed result, or on a configured requirement
  that is absent. This distinction needs an explicit decision before
  implementation.
- Published history stays untouched; the gate is forward-looking only.

## Tests required before this can be called implemented

1. A commit covered by a `blocked` run is refused on the configured path.
2. A commit covered by an accepted run passes.
3. A commit with no governing run behaves per the explicit decision above
   (either passes or is refused — the test must pin whichever is chosen).
4. An unconfigured repository tags successfully with no Exitbind state present.
5. The gate cannot be satisfied by a stale acceptance for a different commit.

## Honest limits of this design

- It is untested. Nothing here has been run.
- The "no governing run" and "configured requirement is absent" cases are
  genuinely ambiguous and are flagged as an open decision, not resolved.
- This document does not claim the 2026-09-19 release would have been blocked
  by option 1, because the release route and the exact non-terminal coverage
  of that commit's run have not been traced end to end.

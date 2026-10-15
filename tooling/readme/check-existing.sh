#!/bin/sh
# Reuse existing tests. Each --exact invocation must execute one test, not zero.
set -eu
root=$(CDPATH= cd -- "$(dirname -- "$0")/../.." && pwd)
cd "$root"
log=$(mktemp)
trap 'rm -f "$log"' EXIT HUP INT TERM
while read -r target name; do
  cargo test --locked --test "$target" "$name" -- --exact > "$log" 2>&1 || { cat "$log"; exit 1; }
  ./scripts/assert-test-ran.sh "$name" < "$log"
done <<'TESTS'
public_positioning first_use_surfaces_cite_no_research_papers
public_positioning readme_links_only_exitbind_owned_or_badge_locations
docs_workflow checked_work_journey_links_resolve_in_the_checkout
docs_workflow contribution_audiences_and_coffee_contract_stay_separate
docs_workflow checked_result_docs_keep_v3_v4_and_current_stable_boundaries_consistent
value_contract claim_registry_and_schema_are_valid
value_contract registry_accepts_wrapped_claims_but_rejects_changed_words
release_integrity release_policy::current_release_and_historical_changelog_pass_with_installer_overrides
release_integrity release_policy::published_stable_is_separate_from_candidate_source_references
release_integrity release_policy::current_guides_reject_obsolete_development_only_availability
release_integrity release_policy::stale_and_malformed_readme_install_commands_are_rejected
release_integrity release_policy::html_comments_cannot_hide_the_readme_install_instruction
first_impression conversation_first_readme_keeps_setup_and_proof_safe_inside_git
TESTS

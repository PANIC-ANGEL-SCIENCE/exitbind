# README status assets

This contributor-only directory and `assets/readme` are excluded from Cargo
packages and Git source archives. Native release packaging already includes only
the binary. No user installation runs this tooling.

The badge workflow runs trusted main on main updates, CI and stable-release workflow lifecycle events, release
changes and manual dispatch. Its only published branch is `readme-badges`, containing
`ci.svg`, `release.svg`, `license.svg` and `status.json`. That exact branch requires
a narrowly scoped branch-rule exception. The job uses the existing job-scoped
GITHUB_TOKEN, no personal access token. Never exempt other branches to run it.

CI describes the newest CI run for the exact current main SHA. A missing run is
unknown, not a previous success. Release uses GitHub's latest published stable
release; an unpublished tag is not a release. License uses GitHub's detected SPDX
identifier and falls back to unknown. `status.json` records the checked subject and
time. GitHub's image proxy can delay the visible image; these are snapshots, not a
continuous connection. Opening the badge link shows the authoritative source.

Only nonempty README.md-only diffs run the existing 13 README-related Rust gates.
No rules are rewritten. Every other change, including tooling, assets, workflow,
unknown bases and mixed changes, runs the existing full jobs. README-only tests
still compile their required targets; avoiding all compilation would change the
existing rules rather than reusing them.

Run `python3 -m unittest discover -s tooling/readme -v` for focused tooling checks.
The regular full CI remains required when modifying this directory or its workflow.

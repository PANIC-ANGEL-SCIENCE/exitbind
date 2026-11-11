# Design Pack 0.7.24 candidate

A skills-only design partner for turning real content and inspected references into deliberate visual work. It covers interfaces, portfolio presentations, brand, editorial, diagrams, objects, spaces, fashion, motion, sound and 3D. Start at [Design Pack](skills/design-pack/SKILL.md) and load only relevant [methods](skills/design-pack/references/methods.md).

## Make, compare and refine

New or substantially rejected directions need construction, not another page of adjectives. The core routes use actual-content thumbnails, type/crop studies and state strips to resolve uncertainty, then inspect the artifact at thumbnail, use and source-detail scales. Ordinary first drafts include refinement. Only an explicitly untouched evaluation uses [first pass](skills/design-pack/references/first-pass.md).

- [Screen production](skills/design-pack/references/screen-production.md): develop the product relationship and meaningful action/state changes alongside the presentation when they affect one another.
- [Portfolio presentation](skills/design-pack/references/portfolio-presentation.md): compose distinct narrative/evidence roles, not repeated hardware by default.
- [Mockup Image](skills/mockup-image/SKILL.md): preserve clean asset masters and repair the relevant source layer, avoiding needless whole-board raster degradation.
- [Source roles](skills/design-pack/references/reference-roles.md): scope constraints by medium and distinguish casting, expression, photographic craft, interface and presentation benchmarks.

No source catalog, fixed palette, layout recipe, external app or private calibration is required for planning. Production uses available authorized capabilities. Missing essential inputs remain explicit limits. A specification is not a made artifact; a static image is not a tested product.

## Scope and privacy

Keep private references, project identity, completed briefs and trial outputs outside the distributable. Preserve required rights notices for any added assets. Viewing, provider submission and public reuse are distinct. The pack's [MIT license](LICENSE) covers its original written methods, not third-party assets.

Existing governed work can use [Exitbind integration](skills/design-pack/references/exitbind.md). Files or manifests do not prove host activation or useful delivery. Record the guidance actually used and a few consequential decisions, without turning documentation into the work itself. The archive contains no runtime hooks, provider client or installation automation.

## Validate and package

From this package root:

```sh
PYTHONDONTWRITEBYTECODE=1 python3 scripts/check_pack.py --self-test
PYTHONDONTWRITEBYTECODE=1 python3 scripts/test_check_run.py
PYTHONDONTWRITEBYTECODE=1 python3 scripts/test_check_image_run.py
PYTHONDONTWRITEBYTECODE=1 python3 scripts/test_check_outcome.py
PYTHONDONTWRITEBYTECODE=1 python3 scripts/test_age_casting_guidance.py
PYTHONDONTWRITEBYTECODE=1 python3 scripts/test_attitude_guidance.py
PYTHONDONTWRITEBYTECODE=1 python3 scripts/test_expression_guidance.py
PYTHONDONTWRITEBYTECODE=1 python3 scripts/test_capture_styling_guidance.py
PYTHONDONTWRITEBYTECODE=1 python3 scripts/test_garment_structure_guidance.py
PYTHONDONTWRITEBYTECODE=1 python3 scripts/test_mirror_selfie_guidance.py
PYTHONDONTWRITEBYTECODE=1 python3 scripts/test_typography_guidance.py
PYTHONDONTWRITEBYTECODE=1 python3 scripts/test_outcome_test_support.py
python3 scripts/package.py --output /absolute/private/output/design-pack.zip
```

The archive uses an exact allowlist and byte parity. [Validation](VALIDATION.md) states check coverage and limits. [Evaluation scenarios](EVALUATION.md) support behavioral review; they are not completed production trials. Optional private run receipts check declared evidence only.


## Candidate status and history

This is a source candidate. Structural and synthetic checks do not establish
installation, host activation, visual improvement or user acceptance. Actual
source inspection, guidance consumption and output review require their own
evidence.

See the [change history](CHANGELOG.md) for preserved version notes and their
original limits.

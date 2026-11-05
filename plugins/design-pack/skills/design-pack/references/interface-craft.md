# Interface craft in real states

Use for a concrete refinement or state/fixture investigation alongside [interface.md](interface.md). These methods extend existing visual guidance with a more explicit evidence boundary. They are independently written synthesis from the bounded [source review](interface-source-notes.md); source-specific numerical and tooling prescriptions are not pack defaults.

## Choose the operation

| Operation | Work product | Boundary |
| --- | --- | --- |
| Reference fidelity | Comparison of the requested frames and implementation | Preserve authorized intent; distinguish exact source values from screenshot estimates and token substitutions. Surface accessibility conflicts rather than blindly reproducing them or secretly changing the brief |
| Exploration | Distinct ideas on common content in the actual context | Different concepts are allowed only in the ordinary exploratory route; explicit first-pass alternatives stay text-only |
| State workbench | Existing component with switchable reachable states | Helps inspect one state at a time; it is not proof that transitions, backend permissions or real requests work |
| Adversarial fixtures | Existing component under applicable difficult inputs | Tests content/environment boundaries, not a redesigned lookalike |
| Review | Findings tied to current artifact, state, viewport and evidence | Separate observed failures, hypotheses and untested coverage; review alone changes nothing |

Choose the smallest useful operation. A one-line copy change does not need a harness. Do not import mandatory tool, agent, one-look budget or approval policies from an upstream skill. Respect existing authorization and the strict [first-pass freeze](first-pass.md): post-freeze inspection may not alter the artifact or create replacement concepts.

## Make the fixture representative

Read the component's inputs, data boundary, rendering branches and production context before choosing cases. Select only reachable states and plausible content shapes. A loading branch, permission state or missing image deserves a case when it exists; an invented cross-product of every possible flag does not. Missing desired behavior is a product finding, not a license to add branches just for a test.

Import the actual component with its real styles, fonts, tokens and providers into an isolated local workbench when that implementation is in scope. Supply synthetic product-shaped data through props or the project's established mock/query/provider seam. Keep fixtures away from live customer data and production writes; prevent refetching from replacing them. Do not rewrite the component into a test-only approximation. If introducing a new seam changes architecture beyond the task, identify the boundary and return it to the decision owner.

Use long names and translated copy that could occur, realistic currency/date formats, representative quantities, missing/failed images, glyph fallback and stable pending/error states. Three tiny identical sample rows make weak evidence. Confirm that the displayed fixtures actually reached the component: an empty page caused by a client/server boundary or a live request overwriting a seed is a harness defect, not a passing empty state. Keep diagnostic controls visibly separate from the artifact under evaluation. Record which cases were omitted and why; do not infer live journey success from a static fixture.

Keep scratch setup removable and excluded from production delivery unless requested. Its retention follows task needs, not a blanket “leave running” or “delete immediately” rule. After an authorized ordinary revision, recheck the affected states and shared paths; do not silently edit a frozen first-pass artifact to improve the evidence.

## Refine the relationship that actually failed

- **Image and text:** inspect the real crop and image pixels behind text, including scrims, gradients and alpha. Check alternate crops and hover/focus/pressed backgrounds. Border, text and disabled roles require their own tokens; a faint separator color is not automatically a readable caption color. Protect the face, product seam or other evidence the image exists to show.
- **Nested edges:** inspect the visible inner and outer corner contours with their actual insets and border thickness. Adjust their relationship so the gap reads intentional; copying a radius formula into every shape can create the wrong silhouette. Check the crop and focus ring as well as the resting card.
- **Optical marks:** compare icon visual mass and baseline against adjacent glyphs, then preserve a stable control target while correcting the visible mark. Equal SVG bounds or stroke numbers do not establish equal perceived size. Check the result at actual size rather than only zoomed in.
- **Repeated actions:** test motion in a realistic repeated sequence. A welcome transition can become tiring on a frequently used control; prefer responsiveness and clear stable feedback where repetition makes flourish costly. Choose duration, amplitude and reduced-motion behavior from context, not a fixed press scale, blur or stagger.
- **Copy and state:** check labels against actual destination/action and loading/error meaning. Preserve user input during recovery. Long names, partial data and failure text must remain specific enough to explain the state; shortening everything can conceal the issue instead of fixing layout.

Example: a translucent caption passes over one dark crop but vanishes when the image shifts to a bright sleeve on mobile. Record that exact crop and blended background; test moving the caption into flow, changing the crop while preserving product evidence, or changing the actual overlay/text pair. A global font-size increase may not address the cause. These alternatives are ordinary revision options; for a frozen first pass, report them as proposed next-pass repairs only.

## Report evidence and cause separately

Use: **artifact/version → component/state/input → viewport/conditions → observation/evidence → suspected cause and confidence → user/visual consequence → proposed repair → retest**. A screenshot can show clipped text but cannot prove the backend caused it. Prefer a reproducible current counterexample over a generic “polish” recommendation. Report inspected coverage and unverified coverage separately, including a harness that could not render. No numeric taste score or claim of full accessibility from a few fixtures.

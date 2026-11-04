# Generative color decisions

Do not limit the palette to this pack, a repository's examples or an industry lookup. Generate a role-based system from the actual service, audience, environment, identity and information hierarchy. The optional seed palettes in `elements.md` are examples to reject freely, not an allowed list.

## Diagnose before choosing hues

Write the service's primary task and emotional requirement separately. Ask internally: Is use frequent or occasional? Is the screen read for hours, scanned under time pressure, explored for inspiration, or used outdoors? What is the cost of confusing a status? How much competing color is already in content? Which brand associations are supported by this client rather than guessed from the industry?

Examples: a finance service does not automatically need blue; a health service does not automatically need green; a creative service does not automatically need saturated chrome. Associations are culturally and contextually dependent hypotheses. Preserve verified identity and accessibility while exploring alternatives.

## Construct rather than retrieve

1. **Allocate attention.** Decide what should dominate: content, primary action, status or atmosphere. Set the neutral-to-expressive balance and the number of simultaneous attention signals.
2. **Select hue families.** Start from identity, subject/material and desired distinction, then compare an alternative family. Give a reason other than a category stereotype. Separate decorative hues from critical status meanings.
3. **Set lightness architecture.** Establish distinguishable canvas, surface, boundary, text and accent layers. Most readability depends on luminance contrast, not simply hue difference. A palette can be colorful yet unreadable when its values collapse together.
4. **Control chroma/saturation.** Use stronger chroma intentionally for emphasis; reduce it for large fields or dense repeated surfaces if visual fatigue/competition appears. Low chroma is not inherently sophisticated, and high chroma is not inherently accessible. Consider how the surrounding colors alter perceived strength.
5. **Create semantic roles.** Canvas, surface, text primary/secondary, border, primary action and its foreground, selection, focus, status, and categorical data. Design actual foreground/background pairs, not disconnected swatches.
6. **Produce light/dark systems if needed.** Rebuild the lightness hierarchy and check elevated surfaces, imagery and accent perception. Do not invert each channel mechanically. Use explicit role tokens for each mode.
7. **Test and revise.** Measure the exact rendered pairs; inspect optical balance with real images, text, data, disabled/error states and relevant surroundings. Use color-vision simulations as aids, not a substitute for redundant encoding or actual accessibility testing. Check gamut/export and physical proofs where relevant.

Perceptual color spaces such as OKLCH can help reason separately about lightness (`L`), chroma (`C`) and hue angle (`h`). Their values are design controls, not guarantees of equal perceived contrast. Validate the final gamut-mapped RGB result in the actual output. In print/textile/material workflows use the producer's color specification and proofing process; screen values alone do not specify a physical result.

## Conditional starting strategies

These are hypotheses to test, not presets or universal service rules.

| Context and task | Hue / chroma hypothesis | Lightness and attention strategy | What could change it |
| --- | --- | --- | --- |
| Frequent, data-dense operational tool | Quiet near-neutral chrome; one controlled accent; data categories only as needed | Strong reading hierarchy and clear selection; avoid a saturated field behind every panel | Existing brand, category density, low-vision needs or screen environment |
| Image-led fashion/art discovery | Derive a restrained support hue or use near-neutral framing so imagery carries variety | Keep controls legible over changing images with a stable surface/scrim; reserve emphasis for discovery actions | An intentionally expressive community identity may justify stronger chrome |
| Trust-sensitive booking or advisory service | Explore warm or cool families according to evidence of tone, not the sector name | Clear primary action, generous readable contrast, calm status treatment and visible recovery | Audience research may prefer warmth, authority, familiarity or vivid accessibility cues |
| Expressive event/cultural identity | Consider a strong hue or contrasting pair tied to the actual program/material | Distinct display hierarchy with a quieter reading/control layer | Poor print gamut, long reading, mixed imagery, language and accessibility constraints |
| Outdoor/mobile wayfinding | Hue distinguishes categories only with shape/labels; select against environment | High legibility and stable contrast under glare; avoid subtle surface distinctions | Physical testing, ambient light, material finish and local signage requirements |

## Worked example A: outfit discovery

Task: inspect clothing photographs and reveal item links. Hypothesis: colorful interface chrome would compete with varied garments. Choose a low-chroma warm canvas, dark neutral text and a subdued green action family. Example project tokens: canvas `#F6F5F1`, ink `#242824`, action `#334C3E` with white foreground. These are this example's choices, not a global fashion palette.

Compare an achromatic alternative and a warmer brown action family with the same photographs. Check whether the accent looks like status, whether white image tags remain visible over bright clothing, and whether focus remains distinct. A surface behind labels is more reliable than assuming every photograph will be dark. Dark mode would need independently selected dark canvas/surface and a lighter accent, followed by measurement; these light values are not a dark-mode recipe.

## Worked example B: operational scheduling

Task: locate conflicts and reschedule quickly during repeated daily use. Start with near-neutral surfaces and reserve saturated color for selected actions and meaningful status. Compare a cool accent derived from existing identity with a warm accent that is distinguishable from warnings. Choose the one that preserves status semantics and scanning under representative data.

Make two full state matrices: normal, selected, pending, conflict, disabled and focus. If the accent and conflict color compete, change their role allocation, lightness/chroma or redundant labeling before adding more hues. Test rows with zero, one and many conflicts. Do not infer that a blue interface establishes trust or improves speed without user evidence.

## Worked example C: expressive publication/event

Task: establish a distinctive identity while keeping a long program readable in print and mobile. Generate a saturated display color from the program's actual imagery or concept, then pair it with a quieter reading surface and strong text. Compare a complementary accent with a single-hue system using different lightness/chroma.

Prototype a poster, a dense schedule and a mobile detail page. If the poster succeeds but small schedule text fails, split expressive display roles from functional information roles; do not discard the entire direction or force display colors into body text. Check the actual print process: a vivid screen color may require a different physical specification, and a proof is needed before promising a match.

## Deliver the reasoning and verification

For each role, record value, color space, intended pairings, light/dark state, reason, source/status and actual contrast observation. Record exceptions and missing tests. Explain alternatives in user language: “warmer and more intimate,” “quieter around dense data,” or “stronger separation from imagery,” then tie that claim to the prototype. Never present measured contrast, audience preference or conversion impact unless actually tested.

## Attributed source specimens, not a closed palette list

The following are tiny observed samples from source files inspected on 2026-11-04, not usage-frequency evidence or recommendations for every client:

- [Taste palette library](https://github.com/Leonxlnx/taste-skill/blob/main/skills/taste-skill/library/palettes.json), blob `9de29b16db9c9f57b977dd38648a47f866572e54`: one specimen uses white `#ffffff`, ink `#111111` and blue `#1a4be0`. Its many candidates illustrate choice, not a universal house palette.
- [Diagram Design style guide](https://github.com/cathrynlavery/diagram-design/blob/main/skills/diagram-design/references/style-guide.md), blob `454be56816afe8b1fa3f188c417b761b20742a12`: light paper `#f5f5f5`, ink `#2d3142` and accent `#bf4520` belong to that author's default skin; the source specifies separate dark tokens.
- [Video Shotcraft palette file](https://github.com/Vincentwei1021/video-shotcraft/blob/main/template/src/themes/palettes.json), blob `ab3cd777979edcd18ea9ac7df540064d1a9696d3`: its deep-ocean specimen uses page `#0b1620`, text `#e8f1f5` and accent `#7cc8b4` for one video theme.

No full palette library is reproduced. Tool repositories such as Penpot, Remotion, Earshot and Blender integration do not establish a universal design-output palette merely by having branded READMEs. Mark that field not applicable instead of inferring it from their logos. Conflicting source styles should be resolved by medium, purpose and context, never majority vote.

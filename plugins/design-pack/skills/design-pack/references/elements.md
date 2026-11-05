# Design elements: choose a system, not isolated decoration

Proactively cover every element relevant to the medium. The client should not have to enumerate color, type, spacing, layout, iconography, shape, imagery, material, motion or sound. Inspect the active identity first, then make missing choices within the brief and delegated authority. Record exact values in the project specification, not this reusable pack.

## Color and palette

1. Determine desired hierarchy and meaning: dominant atmosphere, supporting surfaces, primary action, selection, emphasis, status and data categories. Distinguish brand colors from functional colors.
2. Read reference colors as observations under their capture/lighting conditions, not automatically accurate brand tokens. If extracting colors, record method and color space; photographs include lighting and material effects.
3. Choose a coherent relationship: analogous restraint, complementary emphasis, monochrome material contrast or intentionally varied categorical color. Explain why it fits the subject and use.
4. Build a role palette: canvas, raised surface, primary/secondary text, border, interactive accent with foreground, selected state, focus, success/warning/error, and chart colors only when needed. Include hover/pressed/disabled states and dark mode only if in scope.
5. Test actual foreground/background pairings, not swatches alone. Check muted text, text over images, disabled-state clarity, charts and all relevant themes. Keep status distinguishable without color alone. Do not assume a darker shade always solves contrast coherently.
6. For physical output, coordinate substrate, illumination, process and producer profile. A screen hex value is not a sufficient print, fabric or finish specification. Proof the result.

Original optional seed combinations for quick exploration (not defaults, not copied upstream):

| Intended character | Canvas | Ink | Accent | Why / next test |
| --- | --- | --- | --- | --- |
| Quiet editorial | `#F6F5F1` | `#242824` | `#334C3E` | Content and material dominate; check accent/text pairs in real components |
| Technical clarity | `#F3F6FA` | `#142335` | `#2458A6` | Cool structural tone; check status colors don't collide with the primary action |
| Expressive cultural | `#F5EBD9` | `#302324` | `#9C312B` | Warm print-like emphasis; test imagery and small-text contrast |

Choose, adapt or reject these from the brief. Three values are only a direction seed, not a complete accessible system. Never claim all pairings pass without measuring the intended use. No user's historical project palette is included here.

## Typography and content hierarchy

Specify roles (display, title, body, label, caption, numeric), language/script coverage, weight, line height, letter spacing, line length and fallback. Choose families for content, tone, readability, licensing and availability rather than prestige. A mixed Latin/CJK design needs comparable apparent weight and baseline rhythm; a reference's Latin display face may not support the actual language.

Begin with body readability at the intended device or physical reading distance. Establish a small, purposeful hierarchy and test long/short headings, numbers, punctuation, translated labels and dense data. Use optical alignment where geometry alone looks wrong. Do not force every label into uppercase or every number into a display face. Define truncation/wrapping behavior and preserve access to the full content.

## Spacing, scale and layout

Measure important reference ratios at a known frame size: title-to-body, image-to-text, outer margin, repeated gaps and density. Translate ratios into rules suited to the new medium. Choose a small spacing scale and document intentional exceptions; consistent relationships matter more than adherence to a fashionable unit.

Design the hierarchy, grid, alignment, reading order and density with real content. Group related things more closely than unrelated things. Decide where a section begins and ends through space, contrast and content before adding boxes. Test at narrow and wide sizes, zoom, long content, translated text and actual print scale. Reflow is a behavior, not a screenshot resize.

## Iconography, symbols and shape

Define an icon family's perspective, fill/stroke language, optical weight, corner treatment and level of detail. Check recognition at actual size and provide labels when meaning is uncertain. Match optical size rather than merely using identical bounding boxes. Icons should reinforce a task or identity; do not add them to every heading by habit.

Choose radius, borders, silhouettes and decorative motifs from concept and medium. A softened control and angular photograph can coexist deliberately. Cohesion means shared reasoning, not making every object pixelated, rounded, outlined or metallic. Keep interactive affordances distinguishable from decoration.

## Imagery, illustration and composition

Specify the image's job: show the actual product, communicate evidence, establish context, narrate or express. Prefer truthful subject-specific material. Record source, creator, permission, edits and whether generated. Do not present illustrative people or data as real customers.

Choose crop, camera/viewpoint, scale, focal point, light, texture, background and treatment together. Protect essential information under text overlays and responsive crops. Inspect missing-image behavior. Illustration style should support content and scale, not substitute ornament for a clear story.

For a concrete parameter, budget, fallback and rejection sheet, use [visual-methods.md](visual-methods.md). Choose assets/effects only after the composition and content roles are clear.

## Material, surface, light and depth

For digital surfaces, use elevation or shadow to communicate hierarchy, layering or interaction where useful. Avoid an arbitrary shadow on every object. Consider contrast and compositing across backgrounds.

For 3D/physical work, specify material intent separately from lighting: reflectance/roughness, texture scale, translucency, finish, joining and wear. A glossy render does not establish the material's real feel or durability. Use samples and tests for physical claims. Decorative “grain” or depth should remain subordinate to legibility and the concept.

## Motion and interaction

Specify trigger → transition → stable state → interruption/reversal. Name purpose and frequency. Choose timing and easing from distance, responsiveness and narrative rather than fixed numerical rules. Limit simultaneous competing movement. Include keyboard, touch, reduced-motion and loading/error behavior where applicable. See [motion-sound.md](motion-sound.md) and [interface.md](interface.md).

## Sound and sensory identity

Specify event meaning, priority, timbre, register, attack/decay, repetition and mix relationship. Map sound to actual events rather than decorating every action. Define mute/silent equivalents and environmental context. Verify actual playback and rights; analysis alone is not listening. For spaces or wearables, note relevant tactile, acoustic and material qualities with the same evidence discipline.

## Source observations versus advice versus this pack

| Source cluster | Observed source content | Authors' advice / sample boundary | Pack synthesis |
| --- | --- | --- | --- |
| Taste, UI UX Pro Max | Directions, typography/palette/system choices and page/app distinctions | Their styles/databases are choices, not universal requirements | Select role tokens from brief and context; don't import the corpus |
| Emil, Impeccable | UI detail, motion, layout, type/color refinement and edge-case review | Specific timings/easing and slop rules express author judgment | Explain purpose, test real states and allow justified exceptions |
| Diagram Design, Penpot | Brand tokens, semantic forms, components and handoff | Demo brands and default tokens stay specific to their source | Choose relation first, then apply the active project's system |
| Open Claude UI, Craft Anything | Component/interaction observations and reconstructive analysis | Observations of one product do not define every UI | Measure references, distinguish unknown behavior and preserve provenance |
| HyperFrames, Remotion, Howseen, Cinetic, Shotcraft | Timelines, frames, motion/shot structure and audio concerns | Code license does not license every media asset; samples don't prove reproducibility | Storyboard, reproducible timing, preview, actual playback and rights ledger |
| Lemo-Opuscar, Awesome videos | Style/sample/prompt showcases | Demonstrations and creator prompts are not a general quality guarantee | Use visual-language diversity as inspiration, not a template dependency |
| Earshot, Blender integration, img2threejs | Sound analysis/generation and staged 3D tooling | Tool availability and inferred geometry remain limits | Cue/scene specifications, evidence and medium-appropriate verification |

See [sources.md](sources.md) for every repository and inspected license. This inventory summarizes documentation, not rendered sample inspection or independent tool testing.

For a deeper role/specimen/asset workflow, read [typography.md](typography.md). For coherent alternatives across these elements, read [aesthetic-construction.md](aesthetic-construction.md); do not select each element independently from a style menu.

For an actual image deliverable, use [Mockup Image](../../mockup-image/SKILL.md): medium, available references, authorized use, scene/camera/light/material, crop protection and paired image/UI validation. A prompt-only specification is not a generated image; conceptual imagery is not factual product evidence.

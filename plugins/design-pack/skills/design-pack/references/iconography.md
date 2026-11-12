# Design icons as meaning, artwork and controls

Use for icon selection, original icon design, a family refresh or icon-bearing UI components. Inspect the actual rendered symbols and their context; a library name or equal SVG bounds cannot establish visual consistency. This is not a mandate for a particular icon family, outline style, grid, stroke width or platform.

## Decide whether a symbol helps

Identify the role: decoration already explained by text, information, status, action, navigation, object identity or brand mark. Remove a redundant symbol when it adds noise. Use a visible label when the meaning is unfamiliar or consequential; a tooltip is not a substitute for discoverable essential instruction. Do not use a logo as a generic category decoration or conflate expand, open, select and move.

Separate semantic meaning from artwork. A product can map a stable meaning such as dismiss, disclosure or warning to an approved drawing, with explicit contextual exceptions. Component slots identify purpose; they should not force a literal asset filename into every consumer. Record defaults, overrides, missing-icon behavior and fallback precedence when a registry already exists. Do not build a registry for a one-off mark. A missing meaningful icon must not silently become the wrong action or an empty unnamed control.

## Build or select a coherent family

Start with permitted assets and inspect their license, provenance and supported sizes/variants. For original drawings, compare silhouette, visual center, negative space, stroke and terminal treatment, corner language, detail density and filled mass across representative shapes. Use a few unlike subjects, including a sparse mark and a dense mark, rather than judging ten similar arrows. Preserve distinctive brand artwork separately from utility symbols.

Choose geometry for the intended use size and medium. A construction grid helps maintain relationships; it is not a reason to keep a fuzzy or cramped drawing. Compare horizontal, vertical, diagonal and curved edges on the actual renderer. Small-size variants may need fewer details or wider counters; mechanically scaling a detailed master is not always enough. Keep clean editable masters and document meaningful deviations, not a universal pixel-snapping rule.

Compare marks at native use size, next to actual label glyphs and neighboring icons, on intended backgrounds. Then inspect source detail for clipping, broken paths and unintended artifacts. Equal stroke numbers do not imply equal perceived weight across families. Adjust visible artwork without shrinking its operable target. Validate optical changes in context rather than making every icon fill its box.

Outline, filled and other treatments may indicate selection or simply belong to the identity. Define that relationship instead of universally using fill for selected or outline for default. Preserve recognition across variants; supplement state with text, shape or programmatic state as appropriate. Decorative detail should not obscure the action.

## Keep artwork separate from the interactive contract

The containing button or link owns role, activation, keyboard behavior, target, focus and accessible name. The glyph is not the hit target. Keep target size and spacing appropriate to the actual platform and accessibility requirements even when the mark is visually small. Check focus-ring clipping and overlapping targets, not just declared dimensions.

For icon-only actions, name the action in its current state; for toggles distinguish the action label from selected state. In labeled controls, hide redundant decorative glyphs from the accessibility tree. Informative standalone symbols need a text equivalent. Verify the resulting accessible name, not merely the presence of an aria attribute. Loading artwork must preserve an intelligible name, stable geometry and truthful pending state.

Use semantic foreground roles and check contrast on actual backgrounds and states. Inherited color can keep label and icon coherent, but warning/error or intentionally distinct artwork requires an explicit role. Color alone does not convey status. Test high-contrast/forced-color behavior when relevant; an SVG may disappear despite a correct token name.

Classify directionality by meaning. Mirror directional navigation when appropriate to the target language/platform, not logos, text, clocks or every asymmetric object. Test placement and action meaning separately in both supported directions. Reduced-motion alternatives must retain state information; decorative spin or morphing should not block operation.

## Verify the asset and its consumers

For an SVG, check viewBox, aspect ratio, bounds, fill/stroke behavior, scale response, IDs/definitions collisions in repeated instances and inherited color where intended. Review external references, embedded scripts and provenance before use; sanitization or conversion is not a rights grant. Do not flatten multicolor branded art just to satisfy a monochrome utility convention. Keep attribution and required license notices with redistributed assets.

Build a small actual-size specimen of representative meanings, sizes and applicable variants beside labels and within real controls. Include at least one difficult case selected from the task: a dense mark, long label, selected state, narrow layout, theme contrast, direction change or missing asset. Then inspect the real screen for excess symbol density, ambiguous neighboring actions and consistent meaning across consumers. This is not a universal matrix to fill for a one-icon edit.

Record the chosen meaning/family, source rights, optical exceptions, accessible behavior and observed failures. Separate artwork quality, runtime resolution, semantics and accessibility. Automated name or mapping checks cannot establish recognizability or optical fit; a beautiful icon cannot prove its action works. Use [components](components.md) for integration and [interface craft](interface-craft.md) for actual-component fixtures.

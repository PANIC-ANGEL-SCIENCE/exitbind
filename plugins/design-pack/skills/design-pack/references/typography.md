# Typography: voice, script and optical craft

Typography shapes an artifact before it communicates individual words. Select it with content, composition and emotional intention; do not finish a generic layout and swap the font name. This is independently authored guidance, not a font corpus or a guarantee that a family fits a client.

## Voice and role system

Describe letterform behavior: compact/expansive width, open/tight counters, even/contrasting strokes, sharp/soft endings, upright/sloped stress and regular/irregular rhythm. Relate visible properties to the subject and intended feeling. “Premium” alone cannot choose a font. A geometric sans can be expressive; a serif can be functional; neither is a universal shortcut or ban.

Set roles before counting families: display, section title, reading text, action/label, caption and numeric data. Contrast can come from size, width, weight, case, spacing or family. Change the dimensions needed to distinguish roles. Compare how the actual roles can be distinguished through letterform, width, weight, scale or spatial relationships; family count and display intensity are project choices. A pair should share some rhythm or structure while differing where roles need contrast. Two nearly identical voices can look accidental; competing display faces can erase priority.

Begin body text at the real reading distance and medium. Compose the title as a shape and set supporting labels in relation to it. Test whether personality survives at the actual size, and whether reading text remains pleasant through a paragraph. An alphabet specimen alone is insufficient.

## Shortlist with actual assets in mind

Before direction/production, inventory scripts, content length, numerals, punctuation, symbols, emphasis and deployment constraints. For the required languages, retain a real script/numeral shortlist with family, actual weight and role size. Ordinary work renders comparable specimens before selection; explicit first-pass plans before its single build and inspects the frozen result. A system stack is a deliberate candidate, not evidence of brand expression by itself. Compare a small shortlist using identical text and intended role sizes: first equal declared size, then optical compensation with the difference recorded. Equal CSS size is not equal perceived size. Keep content and layout comparable; better words or shorter titles can bias selection.

Compare actual available paths: an integrated multi-script family, independently selected display and reading roles for each required script, a related family system, or a deliberately contrasting script pair. Name the exact candidate files in the project note. Related naming does not prove matching metrics or script coverage. A display face missing a required script needs an explicit companion and punctuation strategy; do not silently let fallback choose its voice. Inspect actual supported weights and axes; a static regular face does not acquire a drawn bold because CSS requests it. Retain exact license and embedding/subsetting obligations with each asset. No font is bundled or prescribed here.

## Optical relationships across required scripts

Scripts distribute dark mass differently. Compare apparent height, stroke density, counters, punctuation position and baseline rhythm in actual mixed text. Nominal size cannot establish equivalent perceived size across writing systems. Decide the dominant script for each role from the task, then tune companions without making another language subordinate by default.

Inspect weak or oversized companions, crowded forms and floating or sunken punctuation. Adjust supported weight and, where necessary, role/script-specific size or spacing cautiously. Avoid blanket vertical transforms that clip selection or disturb line boxes. Check at normal scale and enlarged glyph detail; distinguish rasterization artifacts from letterform design.

A multi-script family's supplied companion may not have the intended expressive voice. An explicit complementary family can work if placement and fallback are verified. With Unicode-range splitting, test actual glyph ownership—including shared punctuation, digits and symbols—rather than assuming declarations create the intended result. Avoid boundaries that break required combining sequences. Set appropriate language metadata for shaping and line-breaking behavior.

## Real specimen sheet

Use project copy first. The specimen roles below describe what to inspect; they are not client data or a complete coverage test. Extend them for the actual vocabulary and audience.

| Role | Specimen / what to inspect |
| --- | --- |
| Short display | Actual short title — silhouette, counters, phrase breaks and balance between required scripts |
| Long title | Longest plausible title in each required language — line endings, wrap and responsive recomposition |
| Mixed paragraph | A real reading passage including required script transitions — density, punctuation, apparent height and rhythm |
| UI/state | Actual action, loading, retry and longest error strings — control fit, expansion, ellipsis and missing glyphs |
| Numbers | Required currencies, percentages, negative values, times and dates — separators, decimals and tabular figures when supported |
| Distinctions | Ambiguous letters/digits, dense or combining forms, names and symbols in the actual content — distinguishability and completeness |

Construct specimens from each task's actual vocabulary. For a content-free study, label invented strings as synthetic and replace them before approval; no example language is the default. Test required extended characters, historic forms, names and combining characters separately. A few common glyphs cannot establish full coverage. Test emphasis, links and bold within paragraphs, not only isolated style labels.

## Scale, measure and line breaks

Choose role contrasts from the artifact, not a universal modular scale. A poster may use an overwhelming title; a table may need small size differences and stronger alignment. Specify size, weight, width, leading, tracking and measure per role. Derive reading size and leading with actual paragraphs, script metrics, viewing distance and required zoom. Set title relationships using the real words and intended emphasis, then inspect line breaks and glyph fit at the target sizes. Record selected values and their evidence in the project instead of borrowing an example scale.

- **Weight:** compare actual dark mass. Numeric weights are labels within a family, not cross-family optical equivalence. Inspect thin strokes on the intended background, especially small reversed text. Prefer real faces to unintended synthesis.
- **Tracking:** tune by role and script. A tight heading in one writing system does not establish safe spacing in another. Inspect character spacing, kerning, joining behavior and mixed punctuation independently where the scripts require it. Never transfer a source's negative tracking to every language or to body text. Check joins, counters and texture at normal scale.
- **Leading:** separate lines without dissolving a paragraph into stripes. Check mixed-script ascenders/descenders, accents, bold and fallback lines for collisions and uneven rhythm. Fixed tight heights can clip glyphs or fail under user text-spacing changes.
- **Measure:** use real language and role. A character-count rule for one writing system cannot transfer unchanged to another. Consider phrase length, word spacing, glyph width and distance; compare actual paragraphs at narrow and wide widths.
- **Breaks:** compose display titles around meaning and rhythm. Check line-start/end punctuation and relevant language rules; do not force the source language’s hard breaks into every translation. Prefer flexible body/UI wrapping. Use `word-break`, `overflow-wrap` or hyphenation for the intended language/content, then test long names and URLs. Blanket `break-all` and `keep-all` can each fail in the wrong context.

## Asset and implementation contract

Record family/file version, exact source, license file, deployment/embedding/subsetting permissions, attribution or reserved-name obligations, formats, scripts, weights/styles and axes. A code license or platform label does not license every asset. Unknown rights stay unknown; use an authorized alternative or state the blocker. Public preview loading is not permission to redistribute a font.

Map each role to real font faces and fallbacks. Match `@font-face` weights/styles to files; declare a supported range for variable fonts and map static weights correctly. Inspect `wght`, `wdth`, `opsz` and other axes before assigning values. `font-optical-sizing: auto` only uses optical sizing where supported; variation settings cannot create a missing axis. Prefer ordinary properties for standard controls where appropriate and check interactions with explicit variations. Browser-generated bold/italic may not have the intended drawn form; disable synthesis only with a real fallback/emphasis plan.

For web delivery, budget font requests and required-script subset coverage together. A subset may omit a rare name; test actual and dynamic content. Select a loading strategy and inspect both fallback and loaded phases, including missing files and slow loading, for invisible text, layout shift and truncation. Metric adjustments such as `size-adjust` require measured metrics and target-browser checks, not copied percentages. Verify the rendered font for representative glyphs with available diagnostics; the CSS family declaration alone cannot prove glyph source.

For print/PDF, inspect embedding permissions, substitution and the final export at actual size. For motion/3D, inspect rasterization, distance, movement and frame timing. Record performed and unavailable tests separately. Consult current font and language-layout specifications for the selected target when needed; a specification citation alone does not prove conformance.

## Finish and handoff

Inspect real text on intended light/dark/image backgrounds; narrow/wide layouts; zoom and user text spacing; long labels/errors; loading/fallback; and required scripts. Also judge voice, title silhouette, paragraph color and transitions between roles. Technical success does not establish expressive success. Retain an annotated specimen with chosen/rejected pairings and optical reasons, role specifications, asset/license records and coverage gaps.

Ordinary work revises the affected relationship and retests it. Explicit one-shot work prepares the type plan before production, freezes the first artifact and reports failures without changing it; [first-pass.md](first-pass.md) governs. No generated-output quality improvement is demonstrated by this chapter alone.

## Naming language across a set

Choose a task-scoped naming policy before typography is finished: one language or a deliberate division by role, without silently translating approved proper names. Inspect real strings at use size for script mass, baseline, weight, line breaks, wrapping and hierarchy. A language change can alter layout across states and pages; recheck the whole affected set. English-only is not a quality rule, and nominal font metrics are not visual evidence.

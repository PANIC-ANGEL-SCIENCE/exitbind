# Typography: voice, script and optical craft

Typography shapes an artifact before it communicates individual words. Select it with content, composition and emotional intention; do not finish a generic layout and swap the font name. This is independently authored guidance, not a font corpus or a guarantee that a family fits a client.

## Voice and role system

Describe letterform behavior: compact/expansive width, open/tight counters, even/contrasting strokes, sharp/soft endings, upright/sloped stress and regular/irregular rhythm. Relate visible properties to the subject and intended feeling. “Premium” alone cannot choose a font. A geometric sans can be expressive; a serif can be functional; neither is a universal shortcut or ban.

Set roles before counting families: display, section title, reading text, action/label, caption and numeric data. Contrast can come from size, width, weight, case, spacing or family. Change the dimensions needed to distinguish roles. Expressive display with quiet text is one route; one family with contrasting widths/weights is another; restrained size with strong spatial separation is another. A pair should share some rhythm or structure while differing where roles need contrast. Two nearly identical voices can look accidental; competing display faces can erase priority.

Begin body text at the real reading distance and medium. Compose the title as a shape and set supporting labels in relation to it. Test whether personality survives at the actual size, and whether reading text remains pleasant through a paragraph. An alphabet specimen alone is insufficient.

## Shortlist with actual assets in mind

Before direction/production, inventory scripts, content length, numerals, punctuation, symbols, emphasis and deployment constraints. For Korean work, retain a real Hangul/Latin/numeral shortlist with family, actual weight and role size. Ordinary work renders comparable specimens before selection; explicit first-pass plans before its single build and inspects the frozen result. A system stack is a deliberate candidate, not evidence of brand expression by itself. Compare a small shortlist using identical text and intended role sizes: first equal declared size, then optical compensation with the difference recorded. Equal CSS size is not equal perceived size. Keep content and layout comparable; better words or shorter titles can bias selection.

Possible paths, never defaults:

- **Integrated bilingual family:** evaluate a Korean/Latin family to reduce accidental fallback while checking balance. Noto Sans KR offers an even-stroke candidate; Noto Serif KR offers a different stroke-contrast candidate. Suitability is a specimen question.
- **Expressive Hangul display with quiet reading text:** investigate Black Han Sans or Do Hyeon for a strong title silhouette, or Gowun Batang for a different text texture. Test a short real title beside a longer Korean paragraph. Do not assume a display's voice works for captions or that every weight exists.
- **Related family system:** investigate IBM Plex Sans KR with an appropriate related Latin family. Related naming suggests a shortlist, not matching metrics, coverage or identical releases.
- **Deliberate script contrast:** a Latin display face such as Fraunces may serve a Latin-only title with a separately chosen Korean companion. Choose the companion for compatible dark mass, contrast and rhythm. Latin display selection must not silently leave every Korean heading to the operating system.

Reported primary Google Fonts metadata inspected 2026-11-05 supplies these bounded candidate facts. No font was downloaded, rendered or paired in this update; the table is not a recommended combination or full glyph/license audit.

| Candidate metadata | Reported coverage and faces/axes | Selection consequence |
| --- | --- | --- |
| [Noto Sans KR](https://github.com/google/fonts/blob/main/ofl/notosanskr/METADATA.pb) | Korean/Latin; variable `wght` 100–900 | Explore weight within the exact asset's supported range |
| [Noto Serif KR](https://github.com/google/fonts/blob/main/ofl/notoserifkr/METADATA.pb) | Korean/Latin; variable `wght` 200–900 | Compare stroke contrast at real body and heading sizes |
| [IBM Plex Sans KR](https://github.com/google/fonts/blob/main/ofl/ibmplexsanskr/METADATA.pb) | Korean/Latin; static weights 100–700 | Map actual static files, not an invented variable range |
| [Black Han Sans](https://github.com/google/fonts/blob/main/ofl/blackhansans/METADATA.pb) / [Do Hyeon](https://github.com/google/fonts/blob/main/ofl/dohyeon/METADATA.pb) | Korean/Latin; static 400 only | Do not request 700 and assume a real drawn bold exists |
| [Gowun Batang](https://github.com/google/fonts/blob/main/ofl/gowunbatang/METADATA.pb) | Korean/Latin; static 400/700 | Compare the two real weights and reading texture |
| [Fraunces](https://github.com/google/fonts/blob/main/ofl/fraunces/METADATA.pb) | Latin, no Korean listed; `wght` 100–900, `opsz` 9–144, `SOFT` 0–100, `WONK` 0–1, italic file | Test the supported expressive axes; provide an explicit Korean strategy |

All seven metadata records label OFL. Inspect the exact license and files before adoption; a metadata label is not every distribution obligation. No `opsz` is declared in the six Korean candidates above. Never invent optical-size support based on a family name.

## Korean and Latin optical matching

Hangul syllable blocks and Latin letters distribute dark mass differently. Compare apparent height, stroke density, counters, punctuation position and baseline rhythm in mixed sentences. Latin x-height, capital height and a Hangul block cannot all be matched by one nominal size. Decide the dominant script for the role, then tune the companion without shrinking Korean merely to mimic Latin lowercase.

Inspect whether Latin feels weak or oversized, Korean crowded, and punctuation floating or sunken. Adjust supported weight and, where necessary, role/script-specific size or spacing cautiously. Avoid blanket vertical transforms that clip selection or disturb line boxes. Check at normal scale and enlarged glyph detail; do not confuse rasterization artifacts with a face's design.

A mixed-script family's supplied Latin may not have the desired expressive voice. An explicit complementary Latin family can work if placement and fallback are verified. With Unicode-range splitting, test actual glyph ownership—including shared punctuation, digits and symbols—rather than assuming declarations create the intended result. Avoid boundaries that break required combining sequences. Set appropriate language metadata for shaping and line-breaking behavior.

## Real specimen sheet

Use project copy first. These original synthetic specimens are not client data or a complete coverage test. Keep them labeled as examples; extend for the actual vocabulary and audience.

| Role | Specimen / what to inspect |
| --- | --- |
| Short display | `사물의 새로운 표정` / `Objects, reconsidered` — silhouette, counters, phrase breaks and script balance |
| Long title | `매일 쓰는 물건에서 발견한 작은 변화` / `Small changes in the things we use every day` — line endings, wrap and responsive recomposition |
| Mixed paragraph | `서울의 작은 전시에서 Form과 재료의 관계를 살펴봅니다. 빛의 방향이 달라지면 같은 표면도 다르게 보입니다.` — density, punctuation, Latin height and reading rhythm |
| UI/state | `자세히 보기` · `저장 중…` · `다시 시도` · `변경 사항이 저장되지 않았습니다.` — control fit, error expansion, ellipsis and missing glyphs |
| Numbers | `₩128,000 / 12.5% / −3.2 / 08:05 / 2026.11.05` — currency, minus, separators, decimals and tabular figures when supported |
| Distinctions | `Il1 O0 rn m` · `가 각 갂 한 힣` · `“빛” (Light) — A/B · ×` — ambiguous forms, Hangul complexity and symbols |

Test required Hanja, historic Hangul/Jamo, names, other languages and combining characters separately. A few syllables cannot establish full coverage. Test emphasis, links and bold within paragraphs, not only isolated style labels.

## Scale, measure and line breaks

Choose role contrasts from the artifact, not a universal modular scale. A poster may use an overwhelming title; a table may need small size differences and stronger alignment. Specify size, weight, width, leading, tracking and measure per role. As an independent illustrative example, a 17-unit reading role with 27-unit leading and a 44-unit title might suit one editorial frame. These are not source measurements or defaults, and may be wrong for the chosen Korean face or screen.

- **Weight:** compare actual dark mass. Numeric weights are labels within a family, not cross-family optical equivalence. Inspect thin strokes on the intended background, especially small reversed text. Prefer real faces to unintended synthesis.
- **Tracking:** tune by role and script. Tighter large Latin headings do not establish a safe Korean setting. Hangul block spacing, Latin kerning and mixed punctuation need separate inspection. Never transfer a source's negative tracking to every language or to body text. Check joins, counters and texture at normal scale.
- **Leading:** separate lines without dissolving a paragraph into stripes. Check mixed-script ascenders/descenders, accents, bold and fallback lines for collisions and uneven rhythm. Fixed tight heights can clip glyphs or fail under user text-spacing changes.
- **Measure:** use real language and role. A Latin character-count rule cannot transfer unchanged to Korean. Consider phrase length, word spacing, glyph width and distance; compare actual paragraphs at narrow and wide widths.
- **Breaks:** compose display titles around meaning and rhythm. Check Korean line-start/end punctuation behavior and relevant language rules; do not force Latin hard breaks into every translation. Prefer flexible body/UI wrapping. Use `word-break`, `overflow-wrap` or hyphenation for the intended language/content, then test long names and URLs. Blanket `break-all` and `keep-all` can each fail in the wrong context.

## Asset and implementation contract

Record family/file version, exact source, license file, deployment/embedding/subsetting permissions, attribution or reserved-name obligations, formats, scripts, weights/styles and axes. A code license or platform label does not license every asset. Unknown rights stay unknown; use an authorized alternative or state the blocker. Public preview loading is not permission to redistribute a font.

Map each role to real font faces and fallbacks. Match `@font-face` weights/styles to files; declare a supported range for variable fonts and map static weights correctly. Inspect `wght`, `wdth`, `opsz` and other axes before assigning values. `font-optical-sizing: auto` only uses optical sizing where supported; variation settings cannot create a missing axis. Prefer ordinary properties for standard controls where appropriate and check interactions with explicit variations. Browser-generated bold/italic may not have the intended drawn form; disable synthesis only with a real fallback/emphasis plan.

For web delivery, budget font requests and Korean subset coverage together. A subset may omit a rare name; test actual and dynamic content. Select a loading strategy and inspect both fallback and loaded phases, including missing files and slow loading, for invisible text, layout shift and truncation. Metric adjustments such as `size-adjust` require measured metrics and target-browser checks, not copied percentages. Verify the rendered font for representative glyphs with available diagnostics; the CSS family declaration alone cannot prove glyph source.

For print/PDF, inspect embedding permissions, substitution and the final export at actual size. For motion/3D, inspect rasterization, distance, movement and frame timing. Record performed and unavailable tests separately. The [CSS Fonts draft](https://www.w3.org/TR/css-fonts-4/) and [Korean layout draft](https://www.w3.org/TR/klreq/) are reference material, with status in [preview-evidence.md](preview-evidence.md), not automatic conformance evidence.

## Finish and handoff

Inspect real text on intended light/dark/image backgrounds; narrow/wide layouts; zoom and user text spacing; long labels/errors; loading/fallback; and required scripts. Also judge voice, title silhouette, paragraph color and transitions between roles. Technical success does not establish expressive success. Retain an annotated specimen with chosen/rejected pairings and optical reasons, role specifications, asset/license records and coverage gaps.

Ordinary work revises the affected relationship and retests it. Explicit one-shot work prepares the type plan before production, freezes the first artifact and reports failures without changing it; [first-pass.md](first-pass.md) governs. No generated-output quality improvement is demonstrated by this chapter alone.

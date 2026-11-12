# Isolate a subject without redesigning it

Use for background removal, subject isolation, cutout assets and transparent PNG delivery. Choose this route when the subject should remain the same, rather than treating a product photograph as a prompt for a new product. A transparent preview is not proof of preserved detail or a correct alpha channel. A pack-edit request does not authorize a live image trial.

## Establish what must stay unchanged

Inspect the original at full frame, intended display size and edge detail. Identify the subject, real openings, overlapping parts, fine fibers, shadows, reflections and translucent material. Distinguish a white garment or highlight from white background. Mark protected identity and product evidence: seams, hems, logos, hardware, texture, proportions and color relationships. Do not invent hidden construction to complete a silhouette.

Decide whether the task removes only the background, also separates a cast shadow, or deliberately reconstructs missing material. Preserve the untouched source, working mask and export as separate files. Record source dimensions, orientation and color interpretation before editing; derive previews without silently replacing the master. Public availability does not grant provider-upload or redistribution rights. Use only authorized tools and destinations, and respect host tool requirements; this method grants no installation, access or upload permission.

## Choose by fidelity, not by convenience

A generated edit can be useful for an authorized reinterpretation, but it may redraw product detail even when asked only to remove a background. Compare the result with the source before accepting it. If seams, logos, folds, reflections or geometry changed, it fails an exact-preservation task even when the silhouette looks cleaner. Do not keep rerolling the same instruction and call the result source-preserving.

When pixel preservation is required and a permitted masking workflow is available, prefer extracting an alpha mask around the original subject pixels to resynthesizing the subject. Keep the normalized source RGB plane unchanged for an alpha-only operation, and verify that claim on corresponding coordinates before any resizing or color conversion. A hash of the output PNG cannot prove source fidelity. If only an unsuitable generation route is available, report the capability limit rather than covertly switching tools or promising exactness.

Changing edge colors to remove spill, retouching, geometric repair, resampling or profile conversion is not strictly alpha-only. Keep those changes separate, bounded and disclosed when permitted. Fidelity and a clean composite can conflict at contaminated edges; do not conceal that tradeoff by describing both as exact.

## Build a conservative mask, then repair locally

Choose segmentation from the actual image: controlled simple background, complex scene, soft edge and transparent subject need different treatments. A connected-background flood fill can be a useful candidate for a sufficiently uniform field: seed only inspected background regions, select a suitable color-distance rule and compare a small range of tolerances. Connectivity prevents some enclosed subject regions from being removed; it does not protect a similarly colored edge touching the background.

Compare candidates against the same original at the same scale. A loose tolerance may erase pale cloth, highlights or thin straps; a strict tolerance may leave halos and islands. Do not choose the largest removed area or a universal threshold as success. Establish a protected subject region when possible, and inspect all likely leakage paths before accepting a mask.

Treat enclosed openings separately. The hole inside a handle may need removal even though it is disconnected from the outer background; a similarly colored panel inside a garment may be real subject. Use actual boundaries and material evidence, not a global replacement of every near-background pixel.

Refine difficult regions in the alpha mask with local selection or manual correction through an authorized workflow. Work on the smallest evidenced defect and compare the rest of the subject afterward. Preserve thin elements, frayed edges and natural soft transitions. Global erosion, dilation, blur or feathering can destroy detail or create a soft ring; use such operations only when their local effect supports the image. Keep separate candidate masks so a failed cleanup is reversible without repeating extraction.

## Treat translucency as an estimation problem

Hair, fabric fringe, glass and lenses are not all binary opaque shapes. Partial alpha may preserve soft coverage, but a photographed transparent region also contains the old background, tint, reflection and sometimes refraction. One RGB photograph generally does not uniquely determine foreground color and alpha: the composite combines foreground and background under unknown coverage and material behavior.

For a lens, preserve the frame and meaningful reflections while checking whether the old background remains trapped inside. An approximate alpha adjustment can look convincing on one background and wrong on another. Label approximated transparency honestly; do not claim recovered physical transmission or exact material color. Do not erase reflections merely because they are bright. If convincing reuse across backgrounds needs reconstruction beyond the approved task, retain the original and report the limitation rather than silently generating new product evidence.

## Inspect composites, not only the checkerboard

Review the exported file on light, dark and contrasting backgrounds, plus its actual destination when available. Choose backgrounds that expose the subject's real risks rather than a mandatory color list. Check outline halos, clipped fabric, detached islands, transparent holes, retained old background, over-soft edges, missing shadows and unexpected changes to the protected details. An attractive checkerboard preview can hide a light fringe or a partially erased subject.

Inspect whole-object silhouette, native intended-use size and edge/detail views. Zoom helps find defects but does not replace use-size judgment. Compare source and result with matching orientation, scale and coordinates; if a crop is required, record the offset. Use an RGB comparison for an alpha-only claim and visual review for edge quality: neither substitutes for the other. A few unchanged central pixels do not prove the whole garment or product survived.

Reopen the actual exported PNG with a trusted decoder, confirm dimensions and an alpha channel with the intended transparent/partial/opaque regions, and check its color handling. A checkerboard baked into RGB is not transparency. Inspect the receiving app too when integration is in scope: premultiplied/straight-alpha mismatches, scaling or compression can create fringes absent in the editor. Do not claim app verification from a file preview alone.

Check supported file type, reasonable byte/pixel bounds and export integrity before processing or delivery. Do not execute embedded content, follow untrusted external asset references or bypass access failures. Preserve necessary attribution while excluding unnecessary private metadata from distributed derivatives according to the task. Keep the private original intact; metadata removal does not establish permission to reuse an image.

## Deliver with the remaining uncertainty visible

Deliver the real transparent asset and retain the original and editable mask when useful and authorized. State whether it is alpha-only extraction, bounded edge-color repair or approximate transparency, what was inspected and which regions remain imperfect. Keep failed candidates out of the delivered-success set without deleting the source. If detail preservation, transparency or export checks fail, keep that requirement open and repair within scope or report the exact blocker.

Use [image production](image-production.md) for source-layer preservation and composition, and [generation contract](generation-contract.md) only when generation is part of the authorized route. A cleaner-looking cutout is not automatically a more faithful product image.

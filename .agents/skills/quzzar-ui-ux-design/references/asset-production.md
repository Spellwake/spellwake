# From approved image to implementable assets

Inspect and measure the approved image before recreating it. Map important regions with stable
IDs and pixel or normalized bounds: semantic text, controls, layout containers, icons, images,
illustrations, textures, frames, and independently moving parts. Keep the original dimensions
and retain surrounding context for region comparisons. A region crop is reference evidence,
not automatically a production-ready asset.

Use the active host's supported image tools. When the host provides an image-generation skill,
follow its tool instructions for generating/editing mockups, extracting isolated artwork, and
transparent backgrounds. Do not require Impeccable's asset producer, command names, or image API.
Inspect every local reference before editing it. Preserve approved inputs non-destructively.

Reconstruct semantic UI as real elements: readable/selectable text, controls, borders, simple
geometry, layout, and icons that fit the project's icon system. Do not flatten the screenshot
into a page background with invisible hit areas. That is appropriate only for the explicitly
labeled image walkthrough, never the product implementation.

Produce actual artwork assets for image-native regions such as photos, illustrations, painted
materials, or decorative frames when the design needs them. Use the approved region as a visual
reference and isolate the artwork without baked-in UI text, buttons, or unrelated neighbors.
For cutouts, request and preserve real alpha; inspect edges on both light and dark backgrounds.
Do not replace a meaningful illustration with generic CSS shapes merely because extraction is
hard. If fidelity cannot be maintained, show the discrepancy and ask for the specific change.

Separate layers according to independent behavior. For example, a decorative frame, its
replaceable content, and a moving ornament are separate assets/elements when they need to vary
independently. Preserve scale, composition, material, and palette relative to the approved image.
Verify assets inside the assembled screen, not just in isolation. Respect existing editable
SVG/vector/icon sources instead of rasterizing them unnecessarily; use an available vector
workflow only when the requested asset actually needs one.

Keep prompts, source/reference paths, edit lineage, dimensions, alpha requirements, and rights
or source provenance for external artwork with the design record. Label generated content as
illustrative when it might otherwise look like a factual product claim. Project-bound final
assets belong in the repo's asset location, not only in a tool cache or temporary directory.
Do not modify an approved asset in place and carry forward its old approval without review.

import { describe, expect, test } from "bun:test";
import {
  ALL_PLAYMATS,
  ART_PLAYMATS,
  BRAND_PLAYMATS,
  MATERIAL_PLAYMATS,
  PLAYMAT_ART_ASSET_PATHS,
  buildBrandPlaymatSvg,
  resolvePlaymat,
} from "./playmats";

describe("playmats catalogue", () => {
  test("contains all 53 approved playmats across brand, material, and art groups", () => {
    expect(BRAND_PLAYMATS).toHaveLength(18);
    expect(MATERIAL_PLAYMATS).toHaveLength(14);
    expect(ART_PLAYMATS).toHaveLength(21);
    expect(ALL_PLAYMATS).toHaveLength(53);
    expect(PLAYMAT_ART_ASSET_PATHS).toHaveLength(21);
  });

  test("has unique slugs, canonical UUIDs, preview IDs, and asset refs", () => {
    const slugs = new Set(ALL_PLAYMATS.map((m) => m.slug));
    const ids = new Set(ALL_PLAYMATS.map((m) => m.canonicalId));
    const previews = new Set(ALL_PLAYMATS.map((m) => m.previewId));
    const refs = new Set(ALL_PLAYMATS.map((m) => m.assetRef));
    expect(slugs.size).toBe(53);
    expect(ids.size).toBe(53);
    expect(previews.size).toBe(53);
    expect(refs.size).toBe(53);
  });

  test("prices every playmat between $0.99 and $3.99 with zero free starter playmats", () => {
    for (const mat of ALL_PLAYMATS) {
      expect(mat.priceCents).toBeGreaterThanOrEqual(99);
      expect(mat.priceCents).toBeLessThanOrEqual(399);
      expect(mat.priceLabel).toMatch(/^\$[0-3]\.99$/);
    }
  });

  test("resolves playmats by canonical UUID, preview ID, asset ref, or slug", () => {
    const castle = resolvePlaymat("windswept-castle");
    expect(castle).not.toBeNull();
    expect(castle?.edge.kind).toBe("merrowed");
    expect(resolvePlaymat(castle!.canonicalId)).toBe(castle);
    expect(resolvePlaymat(castle!.previewId)).toBe(castle);
    expect(resolvePlaymat(castle!.assetRef)).toBe(castle);
    expect(resolvePlaymat(null)).toBeNull();
    expect(resolvePlaymat("nonexistent")).toBeNull();
  });

  test("generates official Spellwake SVG markup without wordmarks for all 18 brand playmats", () => {
    for (const brand of BRAND_PLAYMATS) {
      const svg = buildBrandPlaymatSvg(brand.slug);
      expect(svg).not.toBeNull();
      expect(svg).toContain("<svg");
      expect(svg).not.toMatch(/SPELLWAKE/i);
    }
  });

  test("applies selective material edge treatments (stitchless stone/felt/paper, saddle leather, merrowed cloth)", () => {
    expect(resolvePlaymat("classic-casino-felt")?.edge.kind).toBe("none");
    expect(resolvePlaymat("obsidian-and-gold-marble")?.edge.kind).toBe("none");
    expect(resolvePlaymat("cartographers-vellum")?.edge.kind).toBe("none");
    expect(resolvePlaymat("obsidian-grain-leather")?.edge.kind).toBe("saddle");
    expect(resolvePlaymat("saddle-espresso-leather")?.edge.kind).toBe("saddle");
    expect(resolvePlaymat("oxblood-library-leather")?.edge.kind).toBe("saddle");
    expect(resolvePlaymat("woven-natural-linen")?.edge.kind).toBe("merrowed");
    expect(resolvePlaymat("raw-indigo-canvas")?.edge.kind).toBe("merrowed");
  });

  test("preserves exact approved stitching on flagship artworks", () => {
    const edgeKey = (slug: string) => {
      const e = resolvePlaymat(slug)?.edge;
      return e && e.kind === "merrowed" ? e.key : e?.kind;
    };
    expect(edgeKey("windswept-castle")).toBe("dark");
    expect(edgeKey("dragon-and-tiger")).toBe("dark");
    expect(edgeKey("twilight-in-the-wilderness")).toBe("dark");
    expect(edgeKey("rocky-wooded-landscape")).toBe("light");
    expect(edgeKey("landscape-with-castle-and-shepherds")).toBe("light");
    expect(edgeKey("under-the-wave-off-kanagawa")).toBe("light");
    expect(edgeKey("burning-of-houses-of-parliament")).toBe("gold");
    expect(edgeKey("tiger-in-bamboo")).toBe("light");
    expect(edgeKey("cliff-walk-at-pourville")).toBe("light");
    expect(edgeKey("the-petite-creuse-river")).toBe("light");
  });
});

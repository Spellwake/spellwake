import { describe, expect, test } from "bun:test";
import type { CustomCardFileV1 } from "@spellwake/application-contracts";
import {
  crc32,
  decodeSwcardJson,
  embedMetadataInPng,
  encodeSwcardJson,
  extractMetadataFromPng,
  SPELLWAKE_PNG_ITXT_KEYWORD,
} from "./codec";

/** Construct a minimal valid 1x1 PNG buffer with proper IHDR + IDAT + IEND chunks. */
function makeMinimalPng(): Uint8Array {
  const sig = new Uint8Array([137, 80, 78, 71, 13, 10, 26, 10]);
  const makeChunk = (type: string, data: Uint8Array) => {
    const out = new Uint8Array(12 + data.length);
    const dv = new DataView(out.buffer);
    dv.setUint32(0, data.length, false);
    for (let i = 0; i < 4; i += 1) out[4 + i] = type.charCodeAt(i);
    out.set(data, 8);
    dv.setUint32(8 + data.length, crc32(out.subarray(4, 8 + data.length)), false);
    return out;
  };
  const ihdr = new Uint8Array(13);
  const ihdrView = new DataView(ihdr.buffer);
  ihdrView.setUint32(0, 1, false);
  ihdrView.setUint32(4, 1, false);
  ihdr[8] = 8; // bit depth
  ihdr[9] = 6; // RGBA
  const ihdrChunk = makeChunk("IHDR", ihdr);
  const iendChunk = makeChunk("IEND", new Uint8Array(0));

  const out = new Uint8Array(sig.length + ihdrChunk.length + iendChunk.length);
  out.set(sig, 0);
  out.set(ihdrChunk, sig.length);
  out.set(iendChunk, sig.length + ihdrChunk.length);
  return out;
}

const SAMPLE_CARD: CustomCardFileV1 = {
  schema: "spellwake-custom-card-v1",
  layout: "normal",
  rarity: "rare",
  collectorNote: "014/250",
  faces: [
    {
      name: "Aetherblade Vanguard",
      manaCost: "{2}{U}{R}",
      typeLine: "Creature \u2014 Human Artificer",
      oracleText: "Flying, haste\nWhen Aetherblade Vanguard enters, draw a card.",
      flavorText: "Forged where lightning meets silence.",
      power: "3",
      toughness: "2",
      loyalty: null,
      defense: null,
      colorIndicator: null,
      frameAccent: "auto",
      art: {
        dataUrl: "data:image/png;base64,iVBORw0KGgo=",
        cropX: 0,
        cropY: -0.1,
        zoom: 1.15,
        artist: "Elena Vance",
      },
    },
  ],
};

describe("PNG iTXt and .swcard.json codec", () => {
  test("round-trips .swcard.json cleanly and rejects invalid schemas", () => {
    const json = encodeSwcardJson(SAMPLE_CARD);
    const decoded = decodeSwcardJson(json);
    expect(decoded.ok).toBe(true);
    if (decoded.ok) {
      expect(decoded.card.faces[0]?.name).toBe("Aetherblade Vanguard");
      expect(decoded.card.faces[0]?.art?.artist).toBe("Elena Vance");
    }

    const invalid = decodeSwcardJson('{"schema":"wrong-v0"}');
    expect(invalid.ok).toBe(false);
  });

  test("round-trips lossless uncompressed PNG iTXt metadata and detects CRC tampering", () => {
    const basePng = makeMinimalPng();
    expect(extractMetadataFromPng(basePng)).toEqual({ kind: "none" });

    const embedded = embedMetadataInPng(basePng, SAMPLE_CARD);
    const extracted = extractMetadataFromPng(embedded);
    expect(extracted.kind).toBe("embedded");
    if (extracted.kind === "embedded") {
      expect(extracted.card.faces[0]?.name).toBe("Aetherblade Vanguard");
      expect(extracted.card.collectorNote).toBe("014/250");
    }

    // Re-embedding replaces any prior spellwake-card-v1 chunk instead of duplicating it.
    const updatedCard: CustomCardFileV1 = {
      ...SAMPLE_CARD,
      collectorNote: "015/250",
    };
    const reEmbedded = embedMetadataInPng(embedded, updatedCard);
    const reExtracted = extractMetadataFromPng(reEmbedded);
    expect(reExtracted.kind).toBe("embedded");
    if (reExtracted.kind === "embedded") {
      expect(reExtracted.card.collectorNote).toBe("015/250");
    }

    // Flip a byte inside the iTXt text payload to confirm CRC32 tamper detection.
    const tampered = new Uint8Array(embedded);
    const kwBytes = new TextEncoder().encode(SPELLWAKE_PNG_ITXT_KEYWORD);
    const kwIdx = tampered.findIndex((_, idx) =>
      kwBytes.every((b, j) => tampered[idx + j] === b),
    );
    expect(kwIdx).toBeGreaterThan(8);
    const targetIdx = kwIdx + kwBytes.length + 10;
    tampered[targetIdx] = (tampered[targetIdx] ?? 0) ^ 0x01;
    const tamperedResult = extractMetadataFromPng(tampered);
    expect(tamperedResult.kind).toBe("invalid");
  });
});

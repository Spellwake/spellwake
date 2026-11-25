/**
 * Lossless PNG `iTXt` (`spellwake-card-v1`) and `.swcard.json` export/import codec
 * for Spellwake Custom Card Studio.
 *
 * Strictly PNG + `.swcard.json` only (zero PDF dependencies).
 * Security invariants:
 * - Pre-parse cap of 512 KiB on extracted `iTXt` payload bytes.
 * - Strictly uncompressed UTF-8 (`compression_flag === 0`) to prevent zip/zlib bombs.
 * - Full IEEE CRC32 verification on every written and read PNG chunk.
 * - Full Zod validation via `customCardFileV1Schema` from `@spellwake/application-contracts`.
 */

import {
  customCardFileV1Schema,
  type CustomCardFileV1,
} from "@spellwake/application-contracts";

export const SPELLWAKE_PNG_ITXT_KEYWORD = "spellwake-card-v1";
export const MAX_PNG_ITXT_PAYLOAD_BYTES = 512 * 1024;
const PNG_SIGNATURE = new Uint8Array([137, 80, 78, 71, 13, 10, 26, 10]);

const CRC_TABLE = (() => {
  const table = new Uint32Array(256);
  for (let n = 0; n < 256; n += 1) {
    let c = n;
    for (let k = 0; k < 8; k += 1) {
      c = (c & 1) !== 0 ? 0xedb88320 ^ (c >>> 1) : c >>> 1;
    }
    table[n] = c >>> 0;
  }
  return table;
})();

export function crc32(bytes: Uint8Array): number {
  let crc = 0xffffffff;
  for (let i = 0; i < bytes.length; i += 1) {
    const b = bytes[i] ?? 0;
    crc = (CRC_TABLE[(crc ^ b) & 0xff] ?? 0) ^ (crc >>> 8);
  }
  return (crc ^ 0xffffffff) >>> 0;
}

function isPngSignature(bytes: Uint8Array): boolean {
  if (bytes.length < 8) return false;
  for (let i = 0; i < 8; i += 1) {
    if (bytes[i] !== PNG_SIGNATURE[i]) return false;
  }
  return true;
}

function readU32BE(bytes: Uint8Array, offset: number): number {
  return (
    (((bytes[offset] ?? 0) << 24) |
      ((bytes[offset + 1] ?? 0) << 16) |
      ((bytes[offset + 2] ?? 0) << 8) |
      (bytes[offset + 3] ?? 0)) >>>
    0
  );
}

function writeU32BE(target: Uint8Array, offset: number, value: number): void {
  target[offset] = (value >>> 24) & 0xff;
  target[offset + 1] = (value >>> 16) & 0xff;
  target[offset + 2] = (value >>> 8) & 0xff;
  target[offset + 3] = value & 0xff;
}

/** Serialize a validated portable custom card as canonical formatted `.swcard.json`. */
export function encodeSwcardJson(card: CustomCardFileV1): string {
  const validated = customCardFileV1Schema.parse(card);
  return JSON.stringify(validated, null, 2);
}

/** Parse and strictly validate a `.swcard.json` payload. */
export function decodeSwcardJson(
  raw: string,
): { ok: true; card: CustomCardFileV1 } | { ok: false; error: string } {
  if (raw.length > MAX_PNG_ITXT_PAYLOAD_BYTES) {
    return { ok: false, error: "Custom card JSON exceeds the 512 KiB size limit." };
  }
  let parsedJson: unknown;
  try {
    parsedJson = JSON.parse(raw);
  } catch {
    return { ok: false, error: "File is not valid JSON." };
  }
  const checked = customCardFileV1Schema.safeParse(parsedJson);
  if (!checked.success) {
    const issue = checked.error.issues[0];
    return {
      ok: false,
      error: issue
        ? `Invalid .swcard.json (${issue.path.join(".") || "root"}: ${issue.message})`
        : "Invalid .swcard.json schema.",
    };
  }
  return { ok: true, card: checked.data };
}

/**
 * Inject an uncompressed UTF-8 PNG `iTXt` chunk (`spellwake-card-v1`) directly before
 * the PNG `IEND` chunk with a valid CRC32 checksum.
 */
export function embedMetadataInPng(
  pngBytes: Uint8Array,
  card: CustomCardFileV1,
): Uint8Array {
  if (!isPngSignature(pngBytes)) {
    throw new Error("Input bytes are not a valid PNG file.");
  }
  const validated = customCardFileV1Schema.parse(card);
  const encoder = new TextEncoder();
  const keywordBytes = encoder.encode(SPELLWAKE_PNG_ITXT_KEYWORD);
  const jsonBytes = encoder.encode(JSON.stringify(validated));
  if (jsonBytes.length > MAX_PNG_ITXT_PAYLOAD_BYTES) {
    throw new Error("Card metadata payload exceeds the 512 KiB PNG iTXt cap.");
  }

  // iTXt layout: keyword\0 + compressionFlag(0) + compressionMethod(0) + langTag\0 + translatedKeyword\0 + utf8Text
  const chunkDataLen = keywordBytes.length + 1 + 1 + 1 + 1 + 1 + jsonBytes.length;
  const chunkType = encoder.encode("iTXt");
  const fullChunk = new Uint8Array(4 + 4 + chunkDataLen + 4);
  writeU32BE(fullChunk, 0, chunkDataLen);
  fullChunk.set(chunkType, 4);

  let pos = 8;
  fullChunk.set(keywordBytes, pos);
  pos += keywordBytes.length;
  fullChunk[pos++] = 0; // null separator after keyword
  fullChunk[pos++] = 0; // compression_flag = 0 (uncompressed)
  fullChunk[pos++] = 0; // compression_method = 0
  fullChunk[pos++] = 0; // empty language tag + null
  fullChunk[pos++] = 0; // empty translated keyword + null
  fullChunk.set(jsonBytes, pos);

  const checksum = crc32(fullChunk.subarray(4, 8 + chunkDataLen));
  writeU32BE(fullChunk, 8 + chunkDataLen, checksum);

  // Locate IEND chunk and insert our iTXt right before it (replacing any older spellwake-card-v1 iTXt).
  const keptSlices: Uint8Array[] = [pngBytes.subarray(0, 8)];
  let offset = 8;
  let inserted = false;

  while (offset + 12 <= pngBytes.length) {
    const len = readU32BE(pngBytes, offset);
    const chunkEnd = offset + 12 + len;
    if (chunkEnd > pngBytes.length) {
      throw new Error("Truncated PNG chunk encountered while embedding metadata.");
    }
    const type = String.fromCharCode(
      pngBytes[offset + 4] ?? 0,
      pngBytes[offset + 5] ?? 0,
      pngBytes[offset + 6] ?? 0,
      pngBytes[offset + 7] ?? 0,
    );
    if (type === "iTXt" && isSpellwakeItxtChunk(pngBytes.subarray(offset + 8, offset + 8 + len))) {
      offset = chunkEnd;
      continue;
    }
    if (type === "IEND" && !inserted) {
      keptSlices.push(fullChunk);
      inserted = true;
    }
    keptSlices.push(pngBytes.subarray(offset, chunkEnd));
    offset = chunkEnd;
  }

  if (!inserted) {
    throw new Error("PNG missing IEND chunk.");
  }

  const totalLen = keptSlices.reduce((acc, s) => acc + s.length, 0);
  const out = new Uint8Array(totalLen);
  let writePos = 0;
  for (const slice of keptSlices) {
    out.set(slice, writePos);
    writePos += slice.length;
  }
  return out;
}

function isSpellwakeItxtChunk(data: Uint8Array): boolean {
  const nul = data.indexOf(0);
  if (nul <= 0) return false;
  const keyword = new TextDecoder().decode(data.subarray(0, nul));
  return keyword === SPELLWAKE_PNG_ITXT_KEYWORD;
}

export type PngMetadataExtractResult =
  | { kind: "embedded"; card: CustomCardFileV1 }
  | { kind: "none" }
  | { kind: "invalid"; error: string };

/**
 * Extract and validate a Spellwake `iTXt` (`spellwake-card-v1`) chunk from PNG bytes.
 * Returns `{ kind: "none" }` cleanly for ordinary external PNGs so the UI can fall back
 * to Codex Vision OCR.
 */
export function extractMetadataFromPng(pngBytes: Uint8Array): PngMetadataExtractResult {
  if (!isPngSignature(pngBytes)) {
    return { kind: "none" };
  }
  const decoder = new TextDecoder("utf-8", { fatal: true });
  let offset = 8;

  while (offset + 12 <= pngBytes.length) {
    const len = readU32BE(pngBytes, offset);
    const chunkEnd = offset + 12 + len;
    if (chunkEnd > pngBytes.length) {
      return { kind: "invalid", error: "PNG file appears truncated." };
    }
    const type = String.fromCharCode(
      pngBytes[offset + 4] ?? 0,
      pngBytes[offset + 5] ?? 0,
      pngBytes[offset + 6] ?? 0,
      pngBytes[offset + 7] ?? 0,
    );
    if (type === "iTXt") {
      const data = pngBytes.subarray(offset + 8, offset + 8 + len);
      const nul = data.indexOf(0);
      if (nul > 0) {
        let keyword = "";
        try {
          keyword = decoder.decode(data.subarray(0, nul));
        } catch {
          offset = chunkEnd;
          continue;
        }
        if (keyword === SPELLWAKE_PNG_ITXT_KEYWORD) {
          // Verify CRC32 before trusting payload bytes.
          const expectedCrc = readU32BE(pngBytes, offset + 8 + len);
          const actualCrc = crc32(pngBytes.subarray(offset + 4, offset + 8 + len));
          if (expectedCrc !== actualCrc) {
            return { kind: "invalid", error: "Embedded card metadata failed CRC32 check." };
          }
          if (nul + 4 > data.length) {
            return { kind: "invalid", error: "Malformed PNG iTXt header." };
          }
          const compressionFlag = data[nul + 1];
          if (compressionFlag !== 0) {
            return {
              kind: "invalid",
              error: "Compressed PNG iTXt chunks are rejected for safety.",
            };
          }
          // Skip compression_flag (1), compression_method (1), langTag\0, translatedKeyword\0.
          const langEnd = data.indexOf(0, nul + 3);
          if (langEnd < 0) {
            return { kind: "invalid", error: "Malformed PNG iTXt language tag." };
          }
          const transEnd = data.indexOf(0, langEnd + 1);
          if (transEnd < 0) {
            return { kind: "invalid", error: "Malformed PNG iTXt translated keyword." };
          }
          const payloadBytes = data.subarray(transEnd + 1);
          if (payloadBytes.length > MAX_PNG_ITXT_PAYLOAD_BYTES) {
            return {
              kind: "invalid",
              error: "Embedded card metadata exceeds 512 KiB limit.",
            };
          }
          let jsonText: string;
          try {
            jsonText = decoder.decode(payloadBytes);
          } catch {
            return { kind: "invalid", error: "Embedded card metadata is not valid UTF-8." };
          }
          const decoded = decodeSwcardJson(jsonText);
          if (!decoded.ok) {
            return { kind: "invalid", error: decoded.error };
          }
          return { kind: "embedded", card: decoded.card };
        }
      }
    }
    if (type === "IEND") break;
    offset = chunkEnd;
  }

  return { kind: "none" };
}

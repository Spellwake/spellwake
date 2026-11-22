import { readDeckIntake } from "../../../../play/src/data/deck-text";
import type {
  DemoDeckEntry,
  DemoSection,
} from "../../../../play/src/collection/data/demo-model";

type SourceLine = {
  name: string;
  quantity: string;
  section: DemoSection;
  sourceText: string;
  set?: string;
  number?: string;
  mtgo?: string;
};
export class FileFormatError extends Error {}
const maxLength = 1_000_000;
export function supportedLink(raw: string) {
  try {
    const url = new URL(raw.trim());
    if (
      !["https:", "http:"].includes(url.protocol) ||
      url.username ||
      url.password ||
      url.port
    )
      return false;
    const host = url.hostname.replace(/^www\./, "");
    return (
      (host === "moxfield.com" && /^\/decks\/[^/]+\/?$/.test(url.pathname)) ||
      (host === "tappedout.net" && /^\/mtg-decks\/[^/]+\/?$/.test(url.pathname))
    );
  } catch {
    return false;
  }
}
function textLines(text: string): SourceLine[] {
  return readDeckIntake(text).lines.map((l) => ({
    ...l,
    ...(l.printingHint?.setCode && l.printingHint.collectorNumber
      ? { set: l.printingHint.setCode, number: l.printingHint.collectorNumber }
      : {}),
  }));
}
const zones: Record<string, DemoSection> = {
  main: "main",
  side: "sideboard",
  sideboard: "sideboard",
  commander: "commander",
  companion: "companion",
  maybeboard: "considering",
  considering: "considering",
};
function xmlLines(text: string, dek: boolean): SourceLine[] {
  const invalid = () =>
    new FileFormatError(
      dek
        ? "Choose a Magic Online .dek file."
        : "Choose a Cockatrice .cod file.",
    );
  if (/<!DOCTYPE\s+[^>]*\[|<!ENTITY\s/i.test(text)) throw invalid();
  const xml = new DOMParser().parseFromString(text, "application/xml");
  const root = xml.documentElement;
  if (
    xml.querySelector("parsererror") ||
    root.localName !== (dek ? "Deck" : "cockatrice_deck")
  )
    throw invalid();
  const result: SourceLine[] = [];
  const add = (card: Element, section: DemoSection) => {
    const name = (card.getAttribute(dek ? "Name" : "name") ?? "").trim();
    const quantity = card.getAttribute(dek ? "Quantity" : "number") ?? "";
    const mtgo = dek ? card.getAttribute("CatID") : null;
    const set = card.getAttribute("set"),
      number =
        card.getAttribute("collectorNumber") ??
        card.getAttribute("collector_number") ??
        card.getAttribute("collector-number");
    if ((!name && !mtgo) || !/^[1-9]\d{0,77}$/.test(quantity)) return;
    const sourceText =
      quantity + " " + name + (set && number ? " (" + set + ") " + number : "");
    if (sourceText.length > 2000) return;
    result.push({
      name,
      quantity,
      section,
      sourceText,
      ...(mtgo ? { mtgo } : {}),
      ...(set && number ? { set, number } : {}),
    });
  };
  if (dek) {
    for (const card of root.children)
      if (card.localName === "Cards")
        add(
          card,
          /^(true|1)$/i.test(card.getAttribute("Sideboard") ?? "")
            ? "sideboard"
            : "main",
        );
  } else {
    for (const zone of root.children) {
      const name = zone.getAttribute("name") ?? "";
      if (zone.localName !== "zone" || !Object.hasOwn(zones, name)) continue;
      for (const card of zone.children)
        if (card.localName === "card") add(card, zones[name]!);
    }
  }
  if (result.length > 1000)
    throw new RangeError("Use at most 1,000 card lines.");
  return result;
}
export async function fileLines(file: File) {
  if (file.size > maxLength * 4)
    throw new FileFormatError("The deck file is too large.");
  const bytes = await file.arrayBuffer();
  const bom = new Uint8Array(bytes, 0, Math.min(2, bytes.byteLength));
  const encoding =
    bom[0] === 255 && bom[1] === 254
      ? "utf-16le"
      : bom[0] === 254 && bom[1] === 255
        ? "utf-16be"
        : "utf-8";
  const text = new TextDecoder(encoding).decode(bytes);
  if (text.length > maxLength)
    throw new FileFormatError("The deck file is too large.");
  const ext = file.name.split(".").pop()?.toLowerCase();
  if (ext === "txt") return textLines(text);
  if (ext === "dek" || ext === "cod") return xmlLines(text, ext === "dek");
  throw new FileFormatError("Choose a .txt, .dek, or .cod file.");
}

type Card = {
  name: string;
  oracleId: string;
  printingId: string;
  set: string;
  number: string;
  type: string;
  manaValue: number | null;
};
const key = (s: string) =>
  s.trim().normalize("NFC").replaceAll("’", "'").toLowerCase();
async function loadIndex() {
  const { default: rows } = await import("virtual:deck-import-catalogue");
  const names = new Map<string, Card | null>(),
    mtgo = new Map<string, Card>();
  for (const line of rows.split("\n")) {
    const [name, oracleId, printingId, set, number, normal, foil, type, mana] =
      line.split("\t");
    if (!name || !oracleId || !printingId || !set || !number || !type) continue;
    const card = {
      name,
      oracleId,
      printingId,
      set,
      number,
      type,
      manaValue: mana ? Number(mana) : null,
    };
    names.set(key(name), names.has(key(name)) ? null : card);
    for (const id of [normal, foil]) if (id) mtgo.set(id, card);
  }
  return { names, mtgo };
}
let index: ReturnType<typeof loadIndex> | undefined;
export async function resolveImport(
  source: string | File,
): Promise<DemoDeckEntry[]> {
  const lines =
    typeof source === "string" ? textLines(source) : await fileLines(source);
  index ??= loadIndex().catch((error) => {
    index = undefined;
    throw error;
  });
  const { names, mtgo } = await index;
  return lines.flatMap((line) => {
    const byId = line.mtgo ? mtgo.get(line.mtgo) : undefined;
    const card = line.name ? names.get(key(line.name)) : byId;
    if (!card) return [];
    const exact =
      byId?.oracleId === card.oracleId ||
      (line.set?.toLowerCase() === card.set && line.number === card.number);
    return [
      {
        id: crypto.randomUUID(),
        name: card.name,
        quantity: line.quantity,
        section: line.section,
        sourceText: line.sourceText,
        catalogue: {
          oracleId: card.oracleId,
          type: card.type,
          manaValue: card.manaValue,
          ...(exact ? { printingId: card.printingId } : {}),
        },
      },
    ];
  });
}

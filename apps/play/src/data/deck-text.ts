import { z } from "zod";
import {
  cardSummarySchema,
  deckEntrySchema,
  namedPrintingQuerySchema,
} from "@spellwake/application-contracts";

export const DECK_TEXT_SECTIONS = [
  "main",
  "sideboard",
  "commander",
  "companion",
  "considering",
] as const;
const sections: Record<string, (typeof DECK_TEXT_SECTIONS)[number]> = {
  deck: "main",
  main: "main",
  mainboard: "main",
  "main deck": "main",
  sideboard: "sideboard",
  commander: "commander",
  companion: "companion",
  considering: "considering",
  maybeboard: "considering",
};
const lineSchema = z.strictObject({
  line: z.number().int().positive(),
  name: cardSummarySchema.shape.name,
  quantity: deckEntrySchema.shape.quantity,
  section: z.enum(DECK_TEXT_SECTIONS),
  sourceText: z.string().max(2000),
  printingHint: namedPrintingQuerySchema.omit({ name: true }).nullable(),
});
export type DeckTextLine = z.infer<typeof lineSchema>;
export type DeckTextError = { line: number; message: string };

/** Parse source-preserving intake without creating local or canonical card identities. */
export function readDeckIntake(text: string) {
  if (text.length > 1_000_000)
    throw new RangeError("The deck list is too large.");
  const lines: DeckTextLine[] = [];
  const errors: DeckTextError[] = [];
  let section: (typeof DECK_TEXT_SECTIONS)[number] = "main";
  for (const [index, sourceText] of text.split(/\r?\n/).entries()) {
    const trimmed = sourceText.trim();
    if (!trimmed) continue;
    const header = trimmed.replace(/:$/, "").toLowerCase();
    const next = Object.hasOwn(sections, header) ? sections[header] : undefined;
    if (next) {
      section = next;
      continue;
    }
    const match = /^(\d+)(?:x)?\s+(.+)$/i.exec(trimmed);
    if (!match) {
      errors.push({
        line: index + 1,
        message:
          "Use a quantity and card name, or a supported section heading.",
      });
      continue;
    }
    const suppliedName = (match[2] ?? "").trim();
    const hint = /^(.*\S)\s+\(([A-Za-z0-9]{1,16})\)\s+(\S{1,128})$/u.exec(
      suppliedName,
    );
    const parsed = lineSchema.safeParse({
      line: index + 1,
      sourceText,
      quantity: match[1],
      section,
      name: hint?.[1] ?? suppliedName,
      printingHint: hint
        ? { setCode: hint[2], collectorNumber: hint[3] }
        : null,
    });
    if (!parsed.success)
      errors.push({
        line: index + 1,
        message: "Invalid card line, quantity or printing hint.",
      });
    else lines.push(parsed.data);
  }
  if (lines.length > 1000)
    throw new RangeError("Use at most 1,000 card lines.");
  return { lines, errors };
}

export function parseDeckIntake(
  text: string,
):
  | { status: "success"; lines: DeckTextLine[] }
  | { status: "fail"; errors: DeckTextError[] } {
  let result;
  try {
    result = readDeckIntake(text);
  } catch (error) {
    if (!(error instanceof RangeError)) throw error;
    return { status: "fail", errors: [{ line: 1, message: error.message }] };
  }
  const { lines, errors } = result;
  if (!lines.length && !errors.length)
    errors.push({ line: 1, message: "Paste at least one card line." });
  return errors.length
    ? { status: "fail", errors }
    : { status: "success", lines };
}

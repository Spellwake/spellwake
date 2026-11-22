import { parseDeckIntake, type DeckTextError } from "../../data/deck-text";
import { demoDeckEntrySchema, type DemoDeckEntry } from "./demo-model";

export type DeckImportResult =
  | { status: "success"; entries: DemoDeckEntry[] }
  | { status: "fail"; errors: DeckTextError[] };

/** Demo identities are allocated only after the neutral text intake is valid. */
export function parseDeckText(text: string): DeckImportResult {
  const intake = parseDeckIntake(text);
  if (intake.status === "fail") return intake;
  const entries: DemoDeckEntry[] = [];
  const errors: DeckTextError[] = [];
  for (const line of intake.lines) {
    const parsed = demoDeckEntrySchema.safeParse({
      id: crypto.randomUUID(),
      name: line.name,
      quantity: line.quantity,
      section: line.section,
      sourceText: line.sourceText,
    });
    if (parsed.success) entries.push(parsed.data);
    else errors.push({ line: line.line, message: "Invalid demo card line." });
  }
  return errors.length
    ? { status: "fail", errors }
    : { status: "success", entries };
}

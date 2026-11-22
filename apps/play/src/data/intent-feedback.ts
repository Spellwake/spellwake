import type { z } from "zod";
import type { DurableState } from "./durable-cell";

type Slot = { kind: string; targetId: string };
/** Feedback follows an immutable attempt in the selected slot, never a previous target's mutation. */
export function intentMutationFeedback<Intent>(
  intent: Intent | undefined,
  slot: Slot | null,
  journal: DurableState<{ state: string; command: { intent: Intent } }>,
  pending: boolean,
  error: Error | null,
  schema: z.ZodType<Intent>,
  slotFor: (intent: Intent) => Slot,
): { isPending: boolean; error: Error | null } {
  const parsed = schema.safeParse(intent);
  let current = false;
  if (parsed.success && slot && journal.status !== "unavailable") {
    const target = slotFor(parsed.data);
    current =
      target.kind === slot.kind &&
      target.targetId === slot.targetId &&
      (journal.status === "empty" ||
        JSON.stringify(parsed.data) ===
          JSON.stringify(journal.value.command.intent));
  }
  const settled =
    journal.status === "saved" && journal.value.state !== "unresolved";
  return {
    isPending: current && pending,
    error: current && !settled ? error : null,
  };
}

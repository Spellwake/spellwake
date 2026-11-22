import { atom } from "jotai";
import { z } from "zod";
import { identifierSchema } from "@spellwake/application-contracts";
import { createDurableCell } from "./durable-cell";

const intentSchema = z.strictObject({
  accountId: identifierSchema,
  cosmeticId: identifierSchema,
  requestId: identifierSchema,
  purchaseId: identifierSchema.nullable(),
});
type Intent = z.infer<typeof intentSchema>;
type IntentAction =
  | { type: "prepare" }
  | { type: "acknowledge"; requestId: string; purchaseId: string }
  | { type: "replace_rejected"; requestId: string }
  | { type: "replace_settled"; requestId: string; purchaseId: string };

/** Product lifetime stays payment-specific; shared storage never resets corrupt identities. */
export function createCheckoutIntentStore(
  origin: string,
  accountId: string,
  cosmeticId: string,
  getStorage: () => Pick<Storage, "getItem" | "setItem">,
) {
  const key = `spellwake.checkout.v1:${origin}:${accountId}:${cosmeticId}`;
  const cell = createDurableCell(
    key,
    intentSchema.refine(
      (intent) =>
        intent.accountId === accountId && intent.cosmeticId === cosmeticId,
    ),
    getStorage,
  );
  const actionAtom = atom(
    null,
    (_get, set, action: IntentAction): Intent | null => {
      const prior = cell.read();
      if (prior.status === "unavailable") {
        set(cell.refreshAtom, prior);
        return null;
      }
      if (
        action.type !== "prepare" &&
        (prior.status !== "saved" ||
          prior.value.requestId !== action.requestId ||
          (action.type !== "replace_rejected" &&
            !identifierSchema.safeParse(action.purchaseId).success))
      )
        return null;
      const intent: Intent =
        prior.status === "saved"
          ? { ...prior.value }
          : {
              accountId,
              cosmeticId,
              requestId: crypto.randomUUID(),
              purchaseId: null,
            };
      if (action.type === "acknowledge") {
        if (intent.purchaseId && intent.purchaseId !== action.purchaseId)
          return null;
        intent.purchaseId = action.purchaseId;
      }
      if (action.type === "replace_settled") {
        if (intent.purchaseId !== action.purchaseId) return null;
        intent.requestId = crypto.randomUUID();
        intent.purchaseId = null;
      }
      if (action.type === "replace_rejected") {
        if (intent.purchaseId !== null) return null;
        intent.requestId = crypto.randomUUID();
      }
      return set(cell.writeAtom, intent);
    },
  );
  return { key, stateAtom: cell.stateAtom, actionAtom };
}

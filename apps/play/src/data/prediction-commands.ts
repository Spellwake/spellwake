/** Immutable prediction attempts, kept like economic ones; reload alone never sends. */
import { useMemo } from "react";
import { atom, useAtomValue, useStore } from "jotai";
import { z } from "zod";
import { useMutation, useQueryClient } from "@tanstack/react-query";
import {
  commandAckSchema,
  identifierSchema,
  predictionAckMatches,
  predictionCommandDispositionSchema,
  predictionCommandErrorsSchema,
  predictionCommandSchema,
  type AccountSession,
  type PredictionCommand,
} from "@spellwake/application-contracts";
import { useAccount } from "../auth/AccountProvider";
import type { ApplicationServices } from "../auth/services";
import { ownerBearer } from "./account";
import { withAccountStorageLock } from "./account-storage";
import { publicOrigin } from "./application-api";
import type { DurableState } from "./durable-cell";
import {
  createIntentJournal,
  type IntentJournal,
  type IntentJournalAction,
} from "./intent-journal";
import { recoverIntent, type RecoverableAction } from "./intent-recovery";

export const predictionSlotSchema = z.strictObject({
  kind: z.enum(["publish_market", "place_order"]),
  targetId: identifierSchema,
});
export type PredictionSlot = z.infer<typeof predictionSlotSchema>;
/** One outstanding publication per printing; one outstanding order per market. */
export function predictionIntentSlot(
  intent: PredictionCommand,
): PredictionSlot | null {
  switch (intent.kind) {
    case "publish_market":
      return { kind: intent.kind, targetId: intent.command.printingId };
    case "place_order":
      return { kind: intent.kind, targetId: intent.command.marketId };
    case "cancel_order":
      return null;
  }
}
const scopedSchema = z.strictObject({
  subjectId: identifierSchema,
  intent: predictionCommandSchema,
});
type Scoped = z.infer<typeof scopedSchema>;
type Errors = z.infer<typeof predictionCommandErrorsSchema>;
export type PredictionJournal = IntentJournal<Scoped, Errors>;

/** Bind provider subject, API, account, operation and target before storing any attempt. */
export function createPredictionJournal(
  origin: string,
  accountId: string,
  subjectId: string,
  slotInput: PredictionSlot,
  getStorage: () => Pick<Storage, "getItem" | "setItem">,
) {
  if (publicOrigin(origin) !== origin)
    throw new Error("Prediction recovery scope is unavailable.");
  identifierSchema.parse(accountId);
  identifierSchema.parse(subjectId);
  const slot = predictionSlotSchema.parse(slotInput);
  const key = `spellwake.prediction-command.v1:${origin}:${accountId}:${slot.kind}:${slot.targetId}`;
  const commandSchema = scopedSchema.refine((command) => {
    const target = predictionIntentSlot(command.intent);
    return (
      command.subjectId === subjectId &&
      target?.kind === slot.kind &&
      target.targetId === slot.targetId
    );
  });
  const source = {
    version: z.literal(1),
    apiOrigin: z.literal(origin),
    accountId: z.literal(accountId),
    command: commandSchema,
  };
  const schema = z
    .discriminatedUnion("state", [
      z.strictObject({ ...source, state: z.literal("unresolved") }),
      z.strictObject({
        ...source,
        state: z.literal("accepted"),
        ack: commandAckSchema,
      }),
      z.strictObject({
        ...source,
        state: z.literal("rejected"),
        errors: predictionCommandErrorsSchema,
      }),
    ])
    .refine(
      (entry) =>
        entry.state !== "accepted" ||
        predictionAckMatches(entry.command.intent, entry.ack.resourceId),
    );
  return {
    ...createIntentJournal<Scoped, Errors>(
      key,
      origin,
      accountId,
      schema,
      commandSchema,
      predictionCommandDispositionSchema,
      (command) => command.intent.command.requestId,
      getStorage,
    ),
    origin,
    accountId,
    subjectId,
    slot,
  };
}

type Owner = Exclude<AccountSession, { state: "needs_registration" }>;
/** The same immutable request is sent or looked up again; a changed draft cannot replace it. */
async function recoverPredictionIntent(
  input: RecoverableAction<PredictionCommand, Scoped, Errors>,
  options: {
    journal: ReturnType<typeof createPredictionJournal>;
    owner: Owner;
    services: Pick<ApplicationServices, "api"> & {
      auth: Pick<ApplicationServices["auth"], "getSession">;
    };
    act: (
      action: IntentJournalAction<Scoped, Errors>,
    ) => PredictionJournal | null;
    isCurrent: () => boolean;
    refresh: () => Promise<void>;
  },
): Promise<PredictionJournal> {
  const { journal, owner, services } = options;
  const changed = () => new Error("Your account changed. Reopen this.");
  if (
    journal.origin !== services.api.origin ||
    journal.accountId !== owner.accountId ||
    journal.subjectId !== owner.subjectId ||
    !options.isCurrent()
  )
    throw changed();
  const bearer = async () => {
    if (!options.isCurrent()) throw changed();
    const token = await ownerBearer(services, owner);
    if (!options.isCurrent()) throw changed();
    return token;
  };
  if (input.type === "submit" && owner.state !== "active")
    throw new Error("This account cannot start another prediction.");
  return recoverIntent(input, {
    key: journal.key,
    build: (intent) => ({ subjectId: owner.subjectId, intent }),
    act: (action) =>
      withAccountStorageLock(journal.origin, journal.accountId, () => {
        if (!options.isCurrent()) throw changed();
        return options.act(action);
      }),
    send: async (command) => {
      if (command.subjectId !== owner.subjectId) throw changed();
      return services.api.predictionCommand(await bearer(), command.intent);
    },
    lookup: async (command) => {
      if (command.subjectId !== owner.subjectId) throw changed();
      return services.api.predictionCommandDisposition(
        await bearer(),
        command.intent,
      );
    },
    refreshCurrent: async () => {
      await bearer();
      await options.refresh();
      if (!options.isCurrent()) throw changed();
    },
  });
}

const emptyJournalAtom = atom<DurableState<PredictionJournal>>({
  status: "empty",
});
/**
 * A prediction command for one slot. Pressing the action again after a
 * failure looks up or resends the original unresolved request first, so a
 * second publication cannot be created by accident.
 */
export function usePredictionCommand(slot: PredictionSlot | null) {
  const { state, services, isCurrentSession } = useAccount();
  const owner =
    state.status === "ready" && state.session.state !== "needs_registration"
      ? state.session
      : null;
  const atoms = useStore();
  const queries = useQueryClient();
  const cell = useMemo(
    () =>
      owner && services && slot
        ? createPredictionJournal(
            services.api.origin,
            owner.accountId,
            owner.subjectId,
            slot,
            () => window.localStorage,
          )
        : null,
    [services, owner?.accountId, owner?.subjectId, slot?.kind, slot?.targetId],
  );
  const journal = useAtomValue<DurableState<PredictionJournal>>(
    cell?.stateAtom ?? emptyJournalAtom,
  );
  const run = useMutation({
    mutationFn: async (
      input: RecoverableAction<PredictionCommand, Scoped, Errors>,
    ) => {
      if (!owner || !services || !cell)
        throw new Error("Choose a prediction first.");
      return recoverPredictionIntent(input, {
        journal: cell,
        owner,
        services,
        act: (action) => atoms.set(cell.actionAtom, action),
        isCurrent: () => isCurrentSession(owner),
        refresh: () =>
          queries.invalidateQueries({
            queryKey: ["predictions", owner.accountId, services.api.origin],
          }),
      });
    },
    onError: () => console.warn("prediction_command code=outcome_unconfirmed"),
  });
  return { journal, run };
}

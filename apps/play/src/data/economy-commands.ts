import { useMemo } from "react";
import { atom, useAtomValue, useStore } from "jotai";
import { useMutation, useQueryClient } from "@tanstack/react-query";
import { economyCommandSchema } from "@spellwake/application-contracts";
import { useAccount } from "../auth/AccountProvider";
import {
  createEconomyJournal,
  economyIntentSlot,
  type EconomySlot,
} from "./economy-journal";
import { recoverEconomyIntent, type EconomyJournal } from "./economy-recovery";
import { refreshEconomyQueries } from "./economy-cache";
import { intentMutationFeedback } from "./intent-feedback";
import type { DurableState } from "./durable-cell";

const emptyJournalAtom = atom<DurableState<EconomyJournal>>({
  status: "empty",
});
/** Durable command adapter for approved controls; loading or observing the hook never sends. */
export function useEconomyCommand(slot: EconomySlot | null) {
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
        ? createEconomyJournal(
            services.api.origin,
            owner.accountId,
            owner.subjectId,
            slot,
            () => window.localStorage,
          )
        : null,
    [services, owner?.accountId, owner?.subjectId, slot?.kind, slot?.targetId],
  );
  const journal = useAtomValue<DurableState<EconomyJournal>>(
    cell?.stateAtom ?? emptyJournalAtom,
  );
  const run = useMutation({
    mutationFn: async (input: Parameters<typeof recoverEconomyIntent>[0]) => {
      if (!owner || !services || !cell)
        throw new Error("Choose a transaction first.");
      return recoverEconomyIntent(input, {
        journal: cell,
        owner,
        services,
        act: (action) => atoms.set(cell.actionAtom, action),
        isCurrent: () => isCurrentSession(owner),
        refresh: () =>
          refreshEconomyQueries(queries, owner.accountId, services.api.origin),
      });
    },
    onError: () => console.warn("economy_command code=outcome_unconfirmed"),
  });
  const intent =
    run.variables?.type === "submit"
      ? run.variables.draft
      : run.variables?.expected.command.intent;
  const feedback = intentMutationFeedback(
    intent,
    cell?.slot ?? null,
    journal,
    run.isPending,
    run.error,
    economyCommandSchema,
    economyIntentSlot,
  );
  return { journal, run: { mutate: run.mutate, ...feedback } };
}

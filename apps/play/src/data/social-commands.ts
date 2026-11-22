import { useMemo } from "react";
import { atom, useAtomValue, useStore } from "jotai";
import { useMutation, useQueryClient } from "@tanstack/react-query";
import { useAccount } from "../auth/AccountProvider";
import {
  createSocialJournal,
  socialIntentSchema,
  socialIntentSlot,
  type SocialSlot,
} from "./social-journal";
import { recoverSocialIntent, type SocialJournal } from "./social-recovery";
import type { DurableState } from "./durable-cell";
import { intentMutationFeedback } from "./intent-feedback";

const emptyJournalAtom = atom<DurableState<SocialJournal>>({ status: "empty" });

/** Mutation observations belong to one slot and immutable attempt, never the next selected peer. */
export function socialMutationFeedback(
  input: Parameters<typeof recoverSocialIntent>[0] | undefined,
  slot: SocialSlot | null,
  journal: DurableState<SocialJournal>,
  pending: boolean,
  error: Error | null,
): { isPending: boolean; error: Error | null } {
  return intentMutationFeedback(
    input?.type === "submit" ? input.draft : input?.expected.command.intent,
    slot,
    journal,
    pending,
    error,
    socialIntentSchema,
    socialIntentSlot,
  );
}

/** One immutable operation slot; saved commands are recovered only by an explicit action. */
export function useSocialCommand(slot: SocialSlot | null) {
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
        ? createSocialJournal(
            services.api.origin,
            owner.accountId,
            owner.subjectId,
            slot,
            () => window.localStorage,
          )
        : null,
    [services, owner?.accountId, owner?.subjectId, slot?.kind, slot?.targetId],
  );
  const journal = useAtomValue<DurableState<SocialJournal>>(
    cell?.stateAtom ?? emptyJournalAtom,
  );
  const run = useMutation({
    mutationFn: async (input: Parameters<typeof recoverSocialIntent>[0]) => {
      if (!owner || !services || !cell)
        throw new Error("Choose a player first.");
      return recoverSocialIntent(input, {
        journal: cell,
        owner,
        services,
        act: (action) => atoms.set(cell.actionAtom, action),
        isCurrent: () => isCurrentSession(owner),
        refresh: async () => {
          const intent =
            input.type === "submit"
              ? input.draft
              : input.expected.command.intent;
          // Read acknowledgments change counts, never history. Canceling a concurrent
          // history page load here would discard the user's Earlier messages action.
          const families =
            intent.kind === "read"
              ? ["social-directory", "social-threads", "social-status"]
              : [
                  "social-directory",
                  "social-threads",
                  "direct-messages",
                  "social-status",
                ];
          await Promise.all(
            families.map((family) =>
              queries.invalidateQueries({
                queryKey: [family, owner.accountId, services.api.origin],
              }),
            ),
          );
        },
      });
    },
    onError: () => console.warn("social_command code=outcome_unconfirmed"),
  });
  const feedback = useMemo(
    () =>
      socialMutationFeedback(
        run.variables,
        cell?.slot ?? null,
        journal,
        run.isPending,
        run.error,
      ),
    [run.variables, cell, journal, run.isPending, run.error],
  );
  return { journal, run: { mutate: run.mutate, ...feedback } };
}

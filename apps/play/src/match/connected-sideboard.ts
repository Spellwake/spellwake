import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { useQuery } from "@tanstack/react-query";
import { useAtom } from "jotai";
import { atomWithStorage } from "jotai/utils";
import type { DeckDetail } from "@spellwake/application-contracts";
import { useAccount } from "../auth/AccountProvider";
import { ownerBearer, useDeckLibrary } from "../data/account";
import { useDeckLabels } from "../data/catalogue";
import type { Sideboard } from "./match-sources";
import {
  changed,
  counts,
  samePool,
  sideboardCards,
  withSideboard,
  type SideboardCard,
} from "./sideboard";

type Entries = NonNullable<DeckDetail["revision"]>["entries"];

/**
 * A deck sideboarded during a match: the list it began with, so the deck can
 * be put back once the match is over, and the version that was saved.
 */
type Sideboarded = {
  accountId: string;
  matchId: string;
  deckId: string;
  startEntries: Entries;
  startMain: number;
  savedRevisionId: string | null;
};

/** Kept on this device so a reload mid-match still puts the deck back. */
const sideboarded = atomWithStorage<Sideboarded | null>(
  "spellwake:match:sideboarded:v1",
  null,
  undefined,
  { getOnInit: true },
);

/**
 * Sideboarding for the account's own deck between the games of a match. The
 * server takes a changed list only as a saved version of the deck with the
 * same cards, which also becomes the deck's current list, so the list the
 * deck began the match with is saved back once the match is over.
 */
export function useConnectedSideboard({
  matchId,
  revisionId,
  deciding,
  over,
  answer,
}: {
  matchId: string | null;
  /** The list the next game would use if nothing changes. */
  revisionId: string | null;
  deciding: boolean;
  over: boolean;
  /** Starts the next game with the given list. Resolves to whether it was taken. */
  answer: (revisionId: string) => Promise<boolean>;
}): Sideboard | null {
  const { state, services } = useAccount();
  const session = state.status === "ready" ? state.session : null;
  const owner = useMemo(
    () =>
      session?.accountId ? { ...session, accountId: session.accountId } : null,
    [session],
  );
  const library = useDeckLibrary();
  const { hasNextPage, isFetchingNextPage, fetchNextPage } = library;
  useEffect(() => {
    if (deciding && hasNextPage && !isFetchingNextPage) void fetchNextPage();
  }, [deciding, hasNextPage, isFetchingNextPage, fetchNextPage]);
  const deckId =
    (library.data?.pages ?? [])
      .flatMap((page) => page.items)
      .find((deck) => deck.currentRevisionId === revisionId)?.id ?? null;

  const detail = useQuery({
    queryKey: ["match-sideboard-deck", owner?.accountId, deckId, revisionId],
    enabled: deciding && !!owner && !!services && !!deckId,
    queryFn: async ({ signal }) => {
      if (!owner || !services || !deckId) return null;
      const read = await services.api.deck(
        await ownerBearer(services, owner),
        deckId,
        signal,
      );
      return read.status === "success" ? read.data : null;
    },
  });
  const entries = useMemo(
    () =>
      detail.data?.revision?.id === revisionId
        ? (detail.data?.revision?.entries ?? null)
        : null,
    [detail.data, revisionId],
  );
  const labels = useDeckLabels(entries ?? []);
  const cards = useMemo(() => {
    if (!entries || !labels.data) return null;
    const names = new Map(
      labels.data.cards.map((card) => [card.id, card.name]),
    );
    return sideboardCards(entries, (id) => names.get(id));
  }, [entries, labels.data]);

  const [record, setRecord] = useAtom(sideboarded);
  const own =
    record &&
    record.accountId === owner?.accountId &&
    record.matchId === matchId
      ? record
      : null;
  const [pending, setPending] = useState(false);
  const [failure, setFailure] = useState<string | null>(null);

  /** Saves a list to the deck and reads back the version it became. */
  const save = useCallback(
    async (
      id: string,
      expectedRevisionId: string,
      list: Entries,
    ): Promise<string | null> => {
      if (!owner || !services) return null;
      const bearer = await ownerBearer(services, owner);
      const sent = await services.api.saveDeck(bearer, {
        requestId: crypto.randomUUID(),
        deckId: id,
        expectedRevisionId,
        entries: list,
      });
      if (sent.status !== "success") return null;
      const read = await services.api.deck(bearer, id);
      return read.status === "success"
        ? read.data.deck.currentRevisionId
        : null;
    },
    [owner, services],
  );

  const use = useCallback(
    (chosen: readonly SideboardCard[]) => {
      if (!owner || !deckId || !entries || !cards || !revisionId || !matchId)
        return;
      setPending(true);
      setFailure(null);
      void (async () => {
        try {
          // Nothing moved: the next game starts with the list as it is.
          if (!changed(cards, chosen)) {
            if (!(await answer(revisionId)))
              setFailure("Couldn’t start the next game. Try again.");
            return;
          }
          const list = withSideboard(entries, chosen) as Entries;
          if (!samePool(entries, list)) {
            setFailure("Only cards already in this deck can be moved.");
            return;
          }
          const start = own ?? {
            accountId: owner.accountId,
            matchId,
            deckId,
            startEntries: entries,
            startMain: counts(cards).main,
            savedRevisionId: null,
          };
          const saved = await save(deckId, revisionId, list);
          if (!saved) {
            setFailure("Couldn’t save your sideboard. Try again.");
            return;
          }
          setRecord({ ...start, savedRevisionId: saved });
          if (!(await answer(saved)))
            setFailure(
              "Your sideboard is saved, but the next game didn’t start. Try again.",
            );
        } catch {
          setFailure("Couldn’t save your sideboard. Try again.");
        } finally {
          setPending(false);
        }
      })();
    },
    [
      owner,
      deckId,
      entries,
      cards,
      revisionId,
      matchId,
      own,
      answer,
      save,
      setRecord,
    ],
  );

  // Once the match is over, the deck gets back the list it began with. A deck
  // changed elsewhere since is left as it now is.
  const restoring = useRef(false);
  useEffect(() => {
    if (!over || !own?.savedRevisionId || restoring.current) return;
    restoring.current = true;
    void (async () => {
      try {
        const back = await save(
          own.deckId,
          own.savedRevisionId!,
          own.startEntries,
        );
        if (back || !services || !owner) setRecord(null);
        else {
          // Refused because the deck moved on: there is nothing left to restore.
          const read = await services.api.deck(
            await ownerBearer(services, owner),
            own.deckId,
          );
          if (
            read.status === "success" &&
            read.data.deck.currentRevisionId !== own.savedRevisionId
          )
            setRecord(null);
        }
      } catch {
        // Tried again the next time the match's end is shown.
      } finally {
        restoring.current = false;
      }
    })();
  }, [over, own, save, setRecord, services, owner]);

  if (!deciding || !revisionId || !deckId) return null;
  return {
    cards,
    minimumMain: own?.startMain ?? (cards ? counts(cards).main : 0),
    use,
    pending,
    failure,
  };
}

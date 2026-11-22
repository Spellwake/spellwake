import { useEffect, useMemo, useState } from "react";
import { useQueryClient } from "@tanstack/react-query";
import { useAtom } from "jotai";
import {
  identifierSchema,
  type DeckDetail,
} from "@spellwake/application-contracts";
import { accountLastFormatAtoms } from "../atoms/deck-shelf";
import { useAccount } from "../auth/AccountProvider";
import { ownerBearer, useDeckLibrary } from "../data/account";
import { useDeckImportCommand } from "../data/deck-import-command";
import { importEntries } from "../data/deck-import-flow";
import { useCardSearch, useDeckLabels } from "../data/catalogue";
import {
  useDeckCommand,
  useDeckFormats,
  useOwnedDeck,
} from "../data/deck-commands";
import {
  appearanceOf,
  sameAppearance,
  selectionOf,
  useConnectedAppearanceOptions,
} from "../cosmetics/connected-cosmetics";
import {
  entriesFrom,
  linesFrom,
  printingLabel,
  refusalMessage,
  sameEntries,
  settingsOf,
} from "./connected-mapping";
import {
  failed,
  succeeded,
  type DeckBuilderSource,
  type DeckShelfSource,
  type FormatChoice,
  type Outcome,
  type PlayMode,
  type SourceGap,
} from "./deck-sources";

/**
 * The signed-in account's decks, through the application API. These hooks
 * only compose the existing reads and journaled commands. They add no request
 * of their own, and they offer nothing the server has no command for.
 */
type Command = ReturnType<typeof useDeckCommand>;
type CommandDraft = Extract<
  Parameters<Command["run"]["mutateAsync"]>[0],
  { type: "submit" }
>["draft"];
/** The box shown for an account deck. Chosen cosmetics do not map to box art yet. */

const SEARCH_PAUSE_MS = 250;
const IMPORT_FAILED = "Couldn’t import deck.";
const IMPORT_UNCONFIRMED = "Couldn’t confirm import.";
/** Total tries at settling one import, the first included. */
const IMPORT_ATTEMPTS = 3;
const pause = (ms: number): Promise<void> =>
  new Promise((resolve) => window.setTimeout(resolve, ms));

/** Why the account has nothing to show, or null when it can be read. */
function useAccountGap(): SourceGap | null {
  const { state } = useAccount();
  if (state.status === "loading") return "loading";
  if (state.status === "signed_out") return "signed-out";
  if (state.status !== "ready") return "unavailable";
  return state.session.accountId ? null : "unregistered";
}

/**
 * Sends one command through its journal. An earlier attempt that never got an
 * answer is settled first and reported, never silently replaced.
 */
async function submit(
  command: Command,
  draft: CommandDraft,
): Promise<Outcome<string>> {
  // The journal is the durable record of the last attempt. Without it a retry could repeat a save.
  if (command.journal.status === "unavailable")
    return failed("This device can’t keep a record of the save. Retry later.");
  const last =
    command.journal.status === "saved" ? command.journal.value : null;
  const waiting = last?.state === "unresolved" ? last : null;
  try {
    const journal = await command.run.mutateAsync(
      waiting
        ? { type: "recover", expected: waiting }
        : { type: "submit", expected: last, draft },
    );
    if (waiting)
      return failed(
        "An earlier save just finished. Check the deck, then retry.",
      );
    if (journal.state === "accepted") return succeeded(journal.ack.resourceId);
    return failed(
      journal.state === "rejected"
        ? refusalMessage(journal.errors)
        : "The save is not confirmed yet. Retry.",
    );
  } catch (error) {
    return failed(
      error instanceof Error ? error.message : "Couldn’t reach the server.",
    );
  }
}

/** Every format, read page by page. The list is short and the screen filters by it. */
function useFormats(): {
  formats: readonly FormatChoice[];
  gap: SourceGap | null;
} {
  const query = useDeckFormats();
  const { hasNextPage, isFetchingNextPage, fetchNextPage } = query;
  useEffect(() => {
    if (hasNextPage && !isFetchingNextPage) void fetchNextPage();
  }, [hasNextPage, isFetchingNextPage, fetchNextPage]);
  const formats = useMemo(
    () =>
      (query.data?.pages ?? []).flatMap((page) =>
        page.items.map(({ id, name }) => ({ id, name })),
      ),
    [query.data],
  );
  return {
    formats,
    gap: query.isError
      ? "unavailable"
      : query.isPending || hasNextPage
        ? "loading"
        : null,
  };
}

export function useConnectedDeckShelf(mode: PlayMode): DeckShelfSource {
  const accountGap = useAccountGap();
  const library = useDeckLibrary();
  const { formats, gap: formatGap } = useFormats();
  const create = useDeckCommand({ kind: "create" });
  const importCommand = useDeckImportCommand();
  const { state, services } = useAccount();
  const [lastFormatId, rememberFormat] = useAtom(accountLastFormatAtoms[mode]);
  // Search and the format filter work on the whole shelf, so the whole shelf is read.
  const { hasNextPage, isFetchingNextPage, fetchNextPage } = library;
  useEffect(() => {
    if (hasNextPage && !isFetchingNextPage) void fetchNextPage();
  }, [hasNextPage, isFetchingNextPage, fetchNextPage]);
  const decks = useMemo(
    () =>
      (library.data?.pages ?? [])
        .flatMap((page) => page.items)
        .filter((deck) => deck.mode === mode)
        .map((deck) => ({
          id: deck.id,
          name: deck.name,
          format: { id: deck.format.id, name: deck.format.name },
          // The server keeps no colour identity; the face card's art is its own.
          appearance: { colors: [], faceArt: deck.faceCard?.artUrl ?? null },
        })),
    [library.data, mode],
  );
  if (accountGap) return { status: accountGap };
  if (library.isError) return { status: "unavailable" };
  if (library.isPending || hasNextPage) return { status: "loading" };
  if (formatGap) return { status: formatGap };
  return {
    status: "ready",
    decks,
    formats,
    lastFormatId,
    rememberFormat,
    createDeck: async (formatId) => {
      const result = await submit(create, {
        kind: "create",
        command: { name: "Untitled", mode, formatId },
      });
      return result.status === "success"
        ? succeeded({ deckId: result.value })
        : result;
    },
    importDeck: async (formatId, text) => {
      const owner =
        state.status === "ready" && state.session.accountId
          ? state.session
          : null;
      if (!owner || !services) return failed(IMPORT_FAILED);
      // Without its record, a retry could make the deck twice.
      if (importCommand.journal.status === "unavailable")
        return failed(IMPORT_FAILED);
      const last =
        importCommand.journal.status === "saved"
          ? importCommand.journal.value
          : null;
      let expected = last;
      try {
        if (last?.state === "unresolved") {
          // An earlier import never got its answer. It is settled before
          // anything else is sent, and opened if it did land.
          const settled = await importCommand.run.mutateAsync({
            type: "recover",
            expected: last,
          });
          if (settled.state === "accepted")
            return succeeded({ deckId: settled.ack.resourceId });
          if (settled.state === "unresolved") return failed(IMPORT_UNCONFIRMED);
          expected = settled;
        }
        const bearer = await ownerBearer(services, owner);
        const found = await importEntries(services.api, bearer, text, () =>
          pause(600),
        );
        if (found.status !== "found") return failed(IMPORT_FAILED);
        let journal = await importCommand.run.mutateAsync({
          type: "submit",
          expected,
          draft: { name: "Untitled", mode, formatId, entries: found.entries },
        });
        for (
          let attempt = 1;
          journal.state === "unresolved" && attempt < IMPORT_ATTEMPTS;
          attempt += 1
        ) {
          await pause(800);
          journal = await importCommand.run.mutateAsync({
            type: "recover",
            expected: journal,
          });
        }
        if (journal.state === "accepted")
          return succeeded({ deckId: journal.ack.resourceId });
        return failed(
          journal.state === "rejected" ? IMPORT_FAILED : IMPORT_UNCONFIRMED,
        );
      } catch {
        // No answer is not a refusal: the deck may have been made.
        return failed(IMPORT_UNCONFIRMED);
      }
    },
  };
}

export function useConnectedDeckBuilder(
  deckId: string | undefined,
): DeckBuilderSource {
  const id = deckId ?? "";
  const isValidId = identifierSchema.safeParse(id).success;
  const accountGap = useAccountGap();
  const { state: accountState, services } = useAccount();
  const queries = useQueryClient();
  const appearanceOptions = useConnectedAppearanceOptions();
  const owned = useOwnedDeck(id);
  const { formats, gap: formatGap } = useFormats();
  const changeSettings = useDeckCommand({ kind: "settings", deckId: id });
  const saveCards = useDeckCommand({ kind: "save", deckId: id });
  /**
   * `opened` is the deck as this editing session first read it. The builder
   * takes its rows from it once, so later reads never replace what is on screen.
   * `basis` is what the server last confirmed. A save is compared with that
   * exact revision, so a change made elsewhere is refused, not overwritten.
   */
  const [opened, setOpened] = useState<DeckDetail | null>(null);
  const [basis, setBasis] = useState<DeckDetail | null>(null);
  if (owned.data && opened?.deck.id !== owned.data.deck.id) {
    setOpened(owned.data);
    setBasis(owned.data);
  }
  const labels = useDeckLabels(opened?.revision?.entries ?? []);
  const [query, setQuery] = useState("");
  const [sent, setSent] = useState("");
  useEffect(() => {
    const timer = window.setTimeout(
      () => setSent(query.trim()),
      SEARCH_PAUSE_MS,
    );
    return () => window.clearTimeout(timer);
  }, [query]);
  const found = useCardSearch(sent);
  const results = useMemo(
    () =>
      (found.data?.pages ?? []).flatMap((page) =>
        page.items.map((card) => ({
          name: card.name,
          card: { definitionId: card.id, printingId: null },
        })),
      ),
    [found.data],
  );
  const lines = useMemo(
    () => (opened && labels.data ? linesFrom(opened, labels.data).lines : null),
    [opened, labels.data],
  );

  if (accountGap) return { status: accountGap };
  if (!isValidId) return { status: "missing" };
  if (owned.isError || labels.isError) return { status: "unavailable" };
  if (!opened || !basis || !lines || !labels.data) return { status: "loading" };
  if (formatGap) return { status: formatGap };
  const names = labels.data;
  /** Adopts what the server now holds as the basis for the next save. */
  const rebase = async (): Promise<DeckDetail | null> => {
    const fresh = await owned.refetch();
    if (fresh.data) setBasis(fresh.data);
    return fresh.data ?? null;
  };
  const nameOf = (definitionId: string | undefined): string | null =>
    names.cards.find((card) => card.id === definitionId)?.name ?? null;
  return {
    status: "ready",
    deck: {
      id: opened.deck.id,
      name: opened.deck.name,
      formatId: opened.deck.format.id,
      mode: opened.deck.mode,
      lines,
      appearance: selectionOf(opened.deck.appearance),
      faceCard: nameOf(opened.deck.faceCard?.definitionId),
    },
    formats,
    appearanceOptions,
    canChooseFaceCard: true,
    // Every published printing carries its official art.
    showsOnBox: () => true,
    // The catalogue names cards. It does not yet supply faces, costs, types or legality.
    facts: () => undefined,
    legality: null,
    printingOf: (line) => printingLabel(line, names),
    search: {
      query,
      setQuery,
      filters: null,
      state:
        query.trim() === ""
          ? "idle"
          : found.isError
            ? "failed"
            : found.isPending || sent !== query.trim()
              ? "loading"
              : "ready",
      results,
    },
    hasCardsFailed: found.isError,
    save: async (draft) => {
      // Entries in a section the builder does not show are saved back as the server holds them.
      const entries = entriesFrom(draft.lines, linesFrom(basis, names).kept);
      if (entries.status === "fail")
        return failed(`Couldn’t match ${entries.name}. Remove it, then save.`);
      /** What the server holds, refreshed after each step of this save. */
      let current = basis;
      const held = settingsOf(basis.deck);
      const chosen = draft.appearance ? appearanceOf(draft.appearance) : null;
      const wanted = {
        ...held,
        name: draft.name.trim(),
        formatId: draft.formatId,
        appearance: chosen ?? held.appearance,
      };
      if (
        wanted.name !== held.name ||
        wanted.formatId !== held.formatId ||
        !sameAppearance(wanted.appearance, held.appearance)
      ) {
        const changed = await submit(changeSettings, {
          kind: "settings",
          command: { deckId: id, expected: held, settings: wanted },
        });
        if (changed.status === "fail") return changed;
        // The name and format are saved even if the cards are then refused.
        current = (await rebase()) ?? current;
      }
      if (!sameEntries(entries.entries, basis.revision?.entries ?? [])) {
        const saved = await submit(saveCards, {
          kind: "save",
          command: {
            deckId: id,
            expectedRevisionId: basis.deck.currentRevisionId,
            entries: entries.entries,
          },
        });
        if (saved.status === "fail") return saved;
        current = (await rebase()) ?? current;
      }
      // The face card is set last, once the cards it names are saved.
      const face = draft.lines.find(
        (line) => line.name === draft.faceCard,
      )?.card;
      if (face && face.definitionId !== current.deck.faceCard?.definitionId) {
        const expected = settingsOf(current.deck);
        const faced = await submit(changeSettings, {
          kind: "settings",
          command: {
            deckId: id,
            expected,
            settings: { ...expected, faceCardId: face.definitionId },
          },
        });
        if (faced.status === "fail") return faced;
        await rebase();
      }
      return succeeded(undefined);
    },
    duplicate:
      accountState.status === "ready" &&
      accountState.session.accountId &&
      services
        ? async (draft) => {
            const owner = accountState.session;
            if (!owner.accountId) return failed("Sign in to duplicate decks.");
            try {
              const bearer = await ownerBearer(services, {
                ...owner,
                accountId: owner.accountId,
              });
              const copyName = `${draft.name.trim().slice(0, 56)} (Copy)`;
              const res = await services.api.duplicateDeck(bearer, id, {
                requestId: crypto.randomUUID(),
                name: copyName,
              });
              if (res.status !== "success") {
                return failed("Couldn’t duplicate deck.");
              }
              await queries.invalidateQueries({
                queryKey: ["deck-library", owner.accountId],
              });
              return succeeded(undefined);
            } catch {
              return failed("Couldn’t reach the server.");
            }
          }
        : null,
    remove:
      accountState.status === "ready" &&
      accountState.session.accountId &&
      services
        ? async () => {
            const owner = accountState.session;
            if (!owner.accountId) return failed("Sign in to delete decks.");
            try {
              const bearer = await ownerBearer(services, {
                ...owner,
                accountId: owner.accountId,
              });
              const res = await services.api.deleteDeck(
                bearer,
                id,
                crypto.randomUUID(),
              );
              if (res.status !== "success") {
                return failed("Couldn’t delete deck.");
              }
              await queries.invalidateQueries({
                queryKey: ["deck-library", owner.accountId],
              });
              return succeeded(undefined);
            } catch {
              return failed("Couldn’t reach the server.");
            }
          }
        : null,
  };
}

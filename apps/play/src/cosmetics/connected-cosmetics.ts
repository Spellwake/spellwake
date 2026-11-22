import { useEffect, useMemo, useRef, useState } from "react";
import { isTauri } from "@tauri-apps/api/core";
import type {
  AppearanceSelection,
  CosmeticSummary,
} from "@spellwake/application-contracts";
import { useAccount } from "../auth/AccountProvider";
import { useAccountSummary } from "../data/account";
import { useCheckout } from "../data/checkout";
import { useCosmeticCatalogue } from "../data/cosmetics";
import { useProfileDefaultsCommand } from "../data/profile-defaults";
import {
  failed,
  succeeded,
  type Outcome,
  type SourceGap,
} from "../decks/deck-sources";
import { openNativeCheckout } from "../lib/checkout";
import type {
  AppearanceOptions,
  CheckoutNotice,
  CosmeticKind,
  CosmeticsSource,
  DefaultSelection,
  DefaultSettingsSource,
  ShopItem,
} from "./cosmetic-sources";

/**
 * The signed-in account's cosmetics, through the application API. These hooks
 * only compose the existing reads and journaled commands. What the account's
 * catalogue does not carry is left out, never invented:
 *
 * - Artwork: the catalogue names each item and its kind, with no picture.
 * - Prices: the catalogue lists none. The price is shown by hosted checkout,
 *   before anything is paid.
 * - Pets: the account has four appearance slots and none for a pet.
 */
const CATEGORIES: readonly CosmeticKind[] = [
  "sleeve",
  "back",
  "dice",
  "playmat",
];
const SAVE_FAILED = "Couldn’t save defaults.";
const SAVE_UNCONFIRMED = "Couldn’t confirm defaults.";
/** Total tries at settling one save, the first included. */
const SAVE_ATTEMPTS = 3;
const pause = (ms: number): Promise<void> =>
  new Promise((resolve) => window.setTimeout(resolve, ms));

function useAccountGap(): SourceGap | null {
  const { state } = useAccount();
  if (state.status === "loading") return "loading";
  if (state.status === "signed_out") return "signed-out";
  if (state.status !== "ready") return "unavailable";
  return state.session.accountId ? null : "unregistered";
}

/** Every cosmetic the catalogue lists, read page by page. */
function useCatalogue(): {
  items: readonly CosmeticSummary[];
  state: "loading" | "ready" | "failed";
} {
  const query = useCosmeticCatalogue();
  const { hasNextPage, isFetchingNextPage, fetchNextPage } = query;
  useEffect(() => {
    if (hasNextPage && !isFetchingNextPage) void fetchNextPage();
  }, [hasNextPage, isFetchingNextPage, fetchNextPage]);
  const items = useMemo(
    () => (query.data?.pages ?? []).flatMap((page) => page.items),
    [query.data],
  );
  return {
    items,
    // A list that is still arriving is not yet proof of what is owned.
    state: query.isError
      ? "failed"
      : query.data && !hasNextPage
        ? "ready"
        : "loading",
  };
}

/** An account appearance as the picker holds it. The account has no pet slot. */
export const selectionOf = (
  appearance: AppearanceSelection,
): DefaultSelection => ({
  sleeve: appearance.sleeveId,
  back: appearance.backId,
  dice: appearance.diceId,
  playmat: appearance.playmatId,
  pet: null,
});
/** The picker's choices as the account stores them, or null when a required one is missing. */
export function appearanceOf(
  selection: DefaultSelection,
): AppearanceSelection | null {
  const { sleeve, back, dice, playmat } = selection;
  return sleeve && back && dice
    ? { sleeveId: sleeve, backId: back, diceId: dice, playmatId: playmat }
    : null;
}

/** Whether two stored appearances choose the same items, whatever their key order. */
export const sameAppearance = (
  a: AppearanceSelection,
  b: AppearanceSelection,
): boolean =>
  a.sleeveId === b.sleeveId &&
  a.backId === b.backId &&
  a.diceId === b.diceId &&
  a.playmatId === b.playmatId;
/** What an account deck's appearance may be chosen from: the cosmetics it owns. */
export function useConnectedAppearanceOptions(): AppearanceOptions {
  const catalogue = useCatalogue();
  return useMemo(
    () => ({
      categories: CATEGORIES,
      optional: ["playmat"],
      items: {
        state: catalogue.state,
        items: catalogue.items
          .filter((item) => item.isOwned)
          .map(({ id, name, kind }) => ({ id, name, kind, look: null })),
      },
    }),
    [catalogue.state, catalogue.items],
  );
}

export function useConnectedDefaultSettings(): DefaultSettingsSource {
  const gap = useAccountGap();
  const summary = useAccountSummary();
  const catalogue = useCatalogue();
  const command = useProfileDefaultsCommand();
  if (gap) return { status: gap };
  if (!summary.data)
    return { status: summary.isError ? "unavailable" : "loading" };
  const current = summary.data.defaults;
  return {
    status: "ready",
    categories: CATEGORIES,
    optional: ["playmat"],
    options: {
      state: catalogue.state,
      items: catalogue.items
        .filter((item) => item.isOwned)
        .map(({ id, name, kind }) => ({ id, name, kind, look: null })),
    },
    saved: selectionOf(current),
    save: async (selection: DefaultSelection): Promise<Outcome> => {
      // The server requires sleeve, back and dice. The screen never offers None for them.
      const appearance = appearanceOf(selection);
      if (!appearance) return failed(SAVE_FAILED);
      // Without its record, a retry could write the defaults twice.
      if (command.journal.status === "unavailable") return failed(SAVE_FAILED);
      const last =
        command.journal.status === "saved" ? command.journal.value : null;
      try {
        if (last?.state === "unresolved") {
          // An earlier save never got its answer. It is settled first, and
          // these new choices are not written over it.
          await command.run.mutateAsync({ type: "recover", expected: last });
          return failed(SAVE_UNCONFIRMED);
        }
        let journal = await command.run.mutateAsync({
          type: "submit",
          expected: last,
          draft: { expected: current, appearance },
        });
        for (
          let attempt = 1;
          journal.state === "unresolved" && attempt < SAVE_ATTEMPTS;
          attempt += 1
        ) {
          await pause(800);
          journal = await command.run.mutateAsync({
            type: "recover",
            expected: journal,
          });
        }
        if (journal.state === "accepted") return succeeded(undefined);
        return failed(
          journal.state === "rejected" ? SAVE_FAILED : SAVE_UNCONFIRMED,
        );
      } catch {
        // No answer is not a refusal: the save may have been received.
        return failed(SAVE_UNCONFIRMED);
      }
    },
  };
}

const ENDED = ["canceled", "refunded", "reversed"];
/** How long to wait for hosted checkout to be ready before giving up on opening it. */
const OPEN_WAIT_MS = 30_000;
/** How long to look for the payment after the person comes back. */
const CONFIRM_WAIT_MS = 20_000;

/**
 * Hosted checkout for one chosen item. The reservation and its recovery
 * belong to `useCheckout`. This adds only what the person sees: opening the
 * hosted page, and passive status. Ownership is read from the account, never
 * taken from the return.
 */
function useHostedCheckout(
  chosen: ShopItem | null,
  setChosen: (itemId: string | null) => void,
): {
  checkout: NonNullable<
    Extract<CosmeticsSource, { status: "ready" }>["checkout"]
  >;
  isPaid: boolean;
} {
  const itemId = chosen?.id ?? "";
  const { intent, purchase, reserve } = useCheckout(itemId);
  const [notice, setNotice] = useState<CheckoutNotice | null>(null);
  const [isBusy, setIsBusy] = useState(false);
  /** The browser tab opened on the click, waiting for the hosted address. */
  const tab = useRef<Window | null>(null);
  /** The purchase whose hosted page is wanted, with when the wait began. */
  const [wanted, setWanted] = useState<{ id: string; since: number } | null>(
    null,
  );
  /** Set once the hosted page is open, and again each time the person returns. */
  const [watching, setWatching] = useState<{ since: number | null } | null>(
    null,
  );
  const closeTab = (): void => {
    tab.current?.close();
    tab.current = null;
  };
  const giveUpOpening = (): void => {
    closeTab();
    setWanted(null);
    setIsBusy(false);
    setNotice({ text: "Couldn’t open checkout.", kind: "result" });
  };

  // Another item, or the dialog closing: nothing may open afterwards unasked.
  useEffect(
    () => () => {
      closeTab();
      setWanted(null);
      setWatching(null);
      setIsBusy(false);
      setNotice(null);
    },
    [itemId],
  );

  const data = purchase.data;
  const state = data?.state;
  const hosted = data?.checkout;
  // Open the hosted page once the reserved purchase offers one.
  useEffect(() => {
    if (!wanted || !data || data.purchaseId !== wanted.id) return;
    if (state === "paid") {
      closeTab();
      setWanted(null);
      return;
    }
    if (hosted?.status === "unavailable" || (state && ENDED.includes(state))) {
      giveUpOpening();
      return;
    }
    if (hosted?.status !== "ready") return;
    setWanted(null);
    if (isTauri()) {
      void openNativeCheckout(hosted.url, hosted.expiresAt).then((isOpen) => {
        if (!isOpen) return giveUpOpening();
        setIsBusy(false);
        setWatching({ since: null });
        setNotice({ text: "Checkout opened.", kind: "working" });
      });
      return;
    }
    if (!tab.current || tab.current.closed) return giveUpOpening();
    tab.current.location.replace(hosted.url);
    tab.current = null;
    setIsBusy(false);
    setWatching({ since: null });
    setNotice({ text: "Checkout opened.", kind: "working" });
  }, [wanted, data?.purchaseId, state, hosted?.status]);
  useEffect(() => {
    if (!wanted) return;
    const timer = window.setTimeout(
      giveUpOpening,
      Math.max(0, wanted.since + OPEN_WAIT_MS - Date.now()),
    );
    return () => window.clearTimeout(timer);
  }, [wanted]);

  // Coming back to the app: look for the payment.
  const { refetch } = purchase;
  useEffect(() => {
    if (!watching) return;
    const look = (): void => {
      if (document.visibilityState !== "visible") return;
      setWatching({ since: Date.now() });
      setNotice({ text: "Checking purchase…", kind: "working" });
      void refetch();
    };
    window.addEventListener("focus", look);
    document.addEventListener("visibilitychange", look);
    return () => {
      window.removeEventListener("focus", look);
      document.removeEventListener("visibilitychange", look);
    };
  }, [watching !== null, refetch]);
  useEffect(() => {
    if (!watching) return;
    if (state === "paid") {
      setWatching(null);
      setNotice({ text: "Cosmetic added.", kind: "result" });
      return;
    }
    if (state && ENDED.includes(state)) {
      setWatching(null);
      setNotice(null);
      return;
    }
    if (watching.since === null) return;
    // Still watching afterwards: a late payment is reported when it arrives.
    const timer = window.setTimeout(
      () =>
        setNotice((current) =>
          current?.text === "Checking purchase…"
            ? { text: "Couldn’t confirm purchase.", kind: "result" }
            : current,
        ),
      Math.max(0, watching.since + CONFIRM_WAIT_MS - Date.now()),
    );
    return () => window.clearTimeout(timer);
  }, [watching, state]);

  const buy = (): void => {
    if (!chosen || chosen.isOwned || !chosen.isSold || isBusy) return;
    if (intent.status === "unavailable") {
      setNotice({ text: "Couldn’t open checkout.", kind: "result" });
      return;
    }
    setIsBusy(true);
    setWatching(null);
    setNotice({ text: "Opening checkout…", kind: "working" });
    if (!isTauri()) {
      // Opened on the click itself, which a browser allows, and pointed at
      // hosted checkout once its address is known. It cannot reach this page.
      const opened = window.open("", "_blank");
      if (!opened) return giveUpOpening();
      opened.opener = null;
      tab.current = opened;
    }
    // A checkout that has ended may be started afresh. Any other is resumed.
    const isEnded = state !== undefined && ENDED.includes(state);
    reserve
      .mutateAsync(isEnded)
      .then((purchaseId) => {
        if (purchaseId) setWanted({ id: purchaseId, since: Date.now() });
        else giveUpOpening();
      })
      .catch(giveUpOpening);
  };
  return {
    checkout: {
      choose: setChosen,
      isBusy,
      notice,
      buy,
    },
    isPaid: state === "paid" && data?.cosmeticId === itemId,
  };
}

export function useConnectedCosmetics(): CosmeticsSource {
  const gap = useAccountGap();
  const catalogue = useCatalogue();
  const [chosenId, setChosenId] = useState<string | null>(null);
  const items = useMemo(
    () =>
      catalogue.items.map(({ id, name, kind, isOwned, isFree }): ShopItem => ({
        id,
        name,
        kind,
        look: null,
        isOwned,
        isSold: !isFree,
        price: null,
      })),
    [catalogue.items],
  );
  const chosen = items.find((item) => item.id === chosenId) ?? null;
  const { checkout, isPaid } = useHostedCheckout(chosen, setChosenId);
  if (gap) return { status: gap };
  return {
    status: "ready",
    categories: CATEGORIES,
    state: catalogue.state,
    // A paid purchase counts at once, before the catalogue is read again.
    items: isPaid
      ? items.map((item) =>
          item.id === chosenId ? { ...item, isOwned: true } : item,
        )
      : items,
    checkout,
  };
}

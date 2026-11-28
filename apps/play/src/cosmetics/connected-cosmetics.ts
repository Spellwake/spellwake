import { useEffect, useMemo, useRef, useState } from "react";
import { useAtom } from "jotai";
import { useQuery } from "@tanstack/react-query";
import { isTauri } from "@tauri-apps/api/core";
import type {
  AppearanceSelection,
  CosmeticSummary,
} from "@spellwake/application-contracts";
import { resolvePlaymat } from "spellwake-renderer-benchmark/playmats";
import { useAccount } from "../auth/AccountProvider";
import { ownerBearer, useAccountSummary } from "../data/account";
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
import {
  MAGIC_CARD_BACK_ID,
  MAGIC_CARD_BACK_URL,
  SPELLWAKE_CARD_BACK_URL,
} from "./card-backs";
import {
  PLAYMAT_EFFECT_CANONICAL_IDS,
  connectedPlaymatEffectDefaultAtom,
  resolvePlaymatEffect,
} from "./playmat-effects";
import {
  CARD_EFFECT_CANONICAL_IDS,
  SLEEVE_EFFECT_CANONICAL_IDS,
  connectedWearDefaultsAtom,
  resolveWearEffect,
  type ConnectedWearDefaults,
} from "./wear-effects";
import type {
  AppearanceOptions,
  CheckoutNotice,
  CosmeticItem,
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
 * - Artwork: built-in backs and wear effects have known visuals. Other picker
 *   choices use the catalogue name and kind.
 * - Prices: read from the rich catalogue when available; hosted checkout shows
 *   the authoritative price before payment.
 * - Pets: shown when present in the catalogue.
 */
const CATEGORIES: readonly CosmeticKind[] = [
  "sleeve",
  "back",
  "dice",
  "playmat",
];
const CATEGORIES_WITH_EFFECTS: readonly CosmeticKind[] = [
  "sleeve",
  "back",
  "dice",
  "playmat",
  "card_effect",
  "sleeve_effect",
  "playmat_effect",
];
const SAVE_FAILED = "Couldn’t save defaults.";
const SAVE_UNCONFIRMED = "Couldn’t confirm defaults.";
/** Total tries at settling one save, the first included. */
const SAVE_ATTEMPTS = 3;
const pause = (ms: number): Promise<void> =>
  new Promise((resolve) => window.setTimeout(resolve, ms));

/** Attach art to the two built-in backs in the existing catalogue. */
function builtInBacks(
  items: readonly CosmeticSummary[],
): ReadonlyMap<string, CosmeticItem> {
  const backs = new Map<string, CosmeticItem>();
  const starter = items.find(
    (item) =>
      item.kind === "back" && item.isFree && item.id !== MAGIC_CARD_BACK_ID,
  );
  if (starter)
    backs.set(starter.id, {
      id: starter.id,
      kind: "back",
      name: "Spellwake",
      look: { imageUrl: SPELLWAKE_CARD_BACK_URL },
    });
  backs.set(MAGIC_CARD_BACK_ID, {
    id: MAGIC_CARD_BACK_ID,
    kind: "back",
    name: "Magic",
    look: { imageUrl: MAGIC_CARD_BACK_URL },
  });
  return backs;
}

/** Build the appearance picker from owned catalogue entries. */
function ownedAppearances(items: readonly CosmeticSummary[]): CosmeticItem[] {
  const backs = builtInBacks(items);
  return items
    .filter((item) => item.isOwned)
    .map(({ id, name, kind }) => {
      const back = backs.get(id);
      if (back) return back;
      const wear = resolveWearEffect(id);
      if (wear) {
        return {
          id,
          name,
          kind,
          look: { wear: { target: wear.target, tier: wear.tier } },
        };
      }
      if (kind === "playmat") {
        const mat = resolvePlaymat(id);
        if (mat) {
          return {
            id,
            name: mat.name,
            kind,
            look: { playmat: { id: mat.slug } },
          };
        }
      }
      if (kind === "playmat_effect") {
        const fx = resolvePlaymatEffect(id);
        if (fx) {
          return {
            id,
            name: fx.name,
            kind,
            look: { playmatEffect: { id: fx.id } },
          };
        }
      }
      return { id, name, kind, look: null };
    });
}

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

/** An account appearance as the picker holds it. */
export const selectionOf = (
  appearance: AppearanceSelection,
  wearDefaults?: ConnectedWearDefaults,
  playmatEffectId?: string,
): DefaultSelection => ({
  sleeve: appearance.sleeveId,
  back: appearance.backId,
  dice: appearance.diceId,
  playmat: appearance.playmatId,
  card_effect: wearDefaults?.cardEffectId ?? CARD_EFFECT_CANONICAL_IDS.nm,
  sleeve_effect: wearDefaults?.sleeveEffectId ?? SLEEVE_EFFECT_CANONICAL_IDS.nm,
  playmat_effect: playmatEffectId ?? PLAYMAT_EFFECT_CANONICAL_IDS.clean,
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
        items: ownedAppearances(catalogue.items),
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
  const [wearDefaults, setWearDefaults] = useAtom(connectedWearDefaultsAtom);
  const [playmatEffectDefault, setPlaymatEffectDefault] = useAtom(
    connectedPlaymatEffectDefaultAtom,
  );
  if (gap) return { status: gap };
  if (!summary.data)
    return { status: summary.isError ? "unavailable" : "loading" };
  const current = summary.data.defaults;
  const owned = ownedAppearances(catalogue.items);
  const hasEffects = owned.some(
    (item) =>
      item.kind === "card_effect" ||
      item.kind === "sleeve_effect" ||
      item.kind === "playmat_effect",
  );
  const ownedIds = new Set(owned.map((item) => item.id));
  const resolvedWear: ConnectedWearDefaults = {
    cardEffectId: ownedIds.has(wearDefaults.cardEffectId)
      ? wearDefaults.cardEffectId
      : CARD_EFFECT_CANONICAL_IDS.nm,
    sleeveEffectId: ownedIds.has(wearDefaults.sleeveEffectId)
      ? wearDefaults.sleeveEffectId
      : SLEEVE_EFFECT_CANONICAL_IDS.nm,
  };
  const resolvedPlaymatEffect = ownedIds.has(playmatEffectDefault)
    ? playmatEffectDefault
    : PLAYMAT_EFFECT_CANONICAL_IDS.clean;
  return {
    status: "ready",
    categories: hasEffects ? CATEGORIES_WITH_EFFECTS : CATEGORIES,
    optional: ["playmat"],
    options: {
      state: catalogue.state,
      items: owned,
    },
    saved: selectionOf(current, resolvedWear, resolvedPlaymatEffect),
    save: async (selection: DefaultSelection): Promise<Outcome> => {
      // The server requires sleeve, back and dice. The screen never offers None for them.
      const appearance = appearanceOf(selection);
      if (!appearance) return failed(SAVE_FAILED);
      const nextWear: ConnectedWearDefaults = {
        cardEffectId: selection.card_effect ?? CARD_EFFECT_CANONICAL_IDS.nm,
        sleeveEffectId:
          selection.sleeve_effect ?? SLEEVE_EFFECT_CANONICAL_IDS.nm,
      };
      const nextPlaymatEffect =
        selection.playmat_effect ?? PLAYMAT_EFFECT_CANONICAL_IDS.clean;
      if (sameAppearance(current, appearance)) {
        setWearDefaults(nextWear);
        setPlaymatEffectDefault(nextPlaymatEffect);
        return succeeded(undefined);
      }
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
        if (journal.state === "accepted") {
          setWearDefaults(nextWear);
          setPlaymatEffectDefault(nextPlaymatEffect);
          return succeeded(undefined);
        }
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

function formatUsdCents(cents: number | null): string | null {
  if (cents === null || !Number.isFinite(cents)) return null;
  const dollars = cents / 100;
  return dollars % 1 === 0
    ? `$${dollars.toFixed(0)}`
    : `$${dollars.toFixed(2)}`;
}

export function useConnectedCosmetics(): CosmeticsSource {
  const gap = useAccountGap();
  const { state, services } = useAccount();
  const owner =
    state.status === "ready" && state.session.accountId
      ? { ...state.session, accountId: state.session.accountId }
      : null;
  const catalogue = useCatalogue();
  const richQuery = useQuery({
    queryKey: ["cosmetics-rich", owner?.accountId, owner?.sessionId],
    enabled: !!owner && !!services,
    queryFn: async ({ signal }) => {
      if (!owner || !services) return null;
      const bearer = await ownerBearer(services, owner);
      const collected = [];
      let after: string | undefined;
      for (let page = 0; page < 10; page += 1) {
        const res = await services.api.cosmeticsRich(
          bearer,
          { limit: 100, ...(after ? { after } : {}) },
          signal,
        );
        if (res.status !== "success") {
          return collected.length > 0 ? collected : null;
        }
        collected.push(...res.data.items);
        if (!res.data.nextCursor || res.data.items.length === 0) break;
        after = res.data.nextCursor;
      }
      return collected;
    },
  });
  const [chosenId, setChosenId] = useState<string | null>(null);
  const richById = useMemo(
    () =>
      new Map((richQuery.data ?? []).map((item) => [item.id, item] as const)),
    [richQuery.data],
  );
  const items = useMemo((): ShopItem[] => {
    const backs = builtInBacks(catalogue.items);
    const base: ShopItem[] = catalogue.items.map(
      ({ id, name, kind, isOwned, isFree }): ShopItem => {
        const rich = richById.get(id);
        const back = backs.get(id);
        const wear = resolveWearEffect(id) ?? resolveWearEffect(rich?.assetRef);
        const playmat =
          kind === "playmat"
            ? (resolvePlaymat(id) ?? resolvePlaymat(rich?.assetRef))
            : null;
        const playmatEffect =
          kind === "playmat_effect"
            ? (resolvePlaymatEffect(id) ?? resolvePlaymatEffect(rich?.assetRef))
            : null;
        return {
          id,
          name:
            back?.name ??
            playmat?.name ??
            playmatEffect?.name ??
            (rich?.name || name),
          kind,
          look:
            back?.look ??
            (wear
              ? { wear: { target: wear.target, tier: wear.tier } }
              : playmat
                ? { playmat: { id: playmat.slug } }
                : playmatEffect
                  ? { playmatEffect: { id: playmatEffect.id } }
                  : rich?.previewUrl
                    ? { imageUrl: rich.previewUrl }
                    : null),
          isOwned: rich ? rich.owned : isOwned,
          isSold: rich
            ? rich.priceCents !== null && rich.priceCents > 0
            : !isFree,
          price: rich ? formatUsdCents(rich.priceCents) : null,
        };
      },
    );
    const seen = new Set(base.map((i) => i.id));
    for (const rich of richQuery.data ?? []) {
      if (seen.has(rich.id)) continue;
      const wear =
        resolveWearEffect(rich.id) ?? resolveWearEffect(rich.assetRef);
      const playmat =
        rich.kind === "playmat"
          ? (resolvePlaymat(rich.id) ?? resolvePlaymat(rich.assetRef))
          : null;
      const playmatEffect =
        rich.kind === "playmat_effect"
          ? (resolvePlaymatEffect(rich.id) ??
            resolvePlaymatEffect(rich.assetRef))
          : null;
      base.push({
        id: rich.id,
        name: playmat?.name ?? playmatEffect?.name ?? rich.name,
        kind: rich.kind,
        look: wear
          ? { wear: { target: wear.target, tier: wear.tier } }
          : playmat
            ? { playmat: { id: playmat.slug } }
            : playmatEffect
              ? { playmatEffect: { id: playmatEffect.id } }
              : rich.previewUrl
                ? { imageUrl: rich.previewUrl }
                : null,
        isOwned: rich.owned,
        isSold: rich.priceCents !== null && rich.priceCents > 0,
        price: formatUsdCents(rich.priceCents),
      });
    }
    return base;
  }, [catalogue.items, richById, richQuery.data]);
  const hasEffects = items.some(
    (item) =>
      item.kind === "card_effect" ||
      item.kind === "sleeve_effect" ||
      item.kind === "playmat_effect",
  );
  const hasPets = items.some((item) => item.kind === "pet");
  const baseCategories = hasEffects ? CATEGORIES_WITH_EFFECTS : CATEGORIES;
  const categories = hasPets
    ? ([...baseCategories, "pet"] as const)
    : baseCategories;
  const chosen = items.find((item) => item.id === chosenId) ?? null;
  const { checkout, isPaid } = useHostedCheckout(chosen, setChosenId);
  if (gap) return { status: gap };
  return {
    status: "ready",
    categories,
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

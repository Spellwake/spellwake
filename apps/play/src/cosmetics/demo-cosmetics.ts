import { useEffect, useRef, useState } from "react";
import { useAtom, useAtomValue } from "jotai";
import { atomWithStorage } from "jotai/utils";
import { succeeded } from "../decks/deck-sources";
import { MAGIC_CARD_BACK_URL, SPELLWAKE_CARD_BACK_URL } from "./card-backs";
import {
  COSMETIC_KINDS,
  type AppearanceOptions,
  type CheckoutNotice,
  type CosmeticKind,
  type CosmeticsSource,
  type DefaultSelection,
  type DefaultSettingsSource,
  type ShopItem,
} from "./cosmetic-sources";

/**
 * The local preview of cosmetics. Its items are stand-ins: their names follow
 * the approved walkthrough, and their looks are patterns painted in code, not
 * artwork. Prices are examples. Nothing here reaches an account or a payment
 * provider, and "buying" only marks the item owned in this browser.
 */
const paint = (...layers: string[]): { paint: string } => ({
  paint: layers.join(","),
});
const sky = (top: string, mid: string, low: string, glow: string) =>
  paint(
    `radial-gradient(120% 70% at 30% 18%, ${glow}, transparent 60%)`,
    `radial-gradient(90% 60% at 78% 72%, ${mid}cc, transparent 62%)`,
    `linear-gradient(160deg, ${top}, ${mid} 52%, ${low})`,
  );
const BOUGHT: ReadonlySet<string> = new Set([
  "sleeve:Moonstone",
  "sleeve:Forest",
  "sleeve:Storm",
  "back:Moon",
  "dice:Midnight",
  "playmat:Moonlit",
]);

const item = (
  kind: CosmeticKind,
  name: string,
  look: ShopItem["look"],
  price: string | null,
): ShopItem => ({
  id: `preview-${kind}-${name.toLowerCase()}`,
  name,
  kind,
  look,
  // An item with no price is one every account has. A few others start bought.
  isOwned: price === null || BOUGHT.has(`${kind}:${name}`),
  isSold: price !== null,
  price,
});
/** What the preview person already owns, as in the approved walkthrough. */
const ITEMS: readonly ShopItem[] = [
  item(
    "sleeve",
    "Classic",
    sky("#274a7a", "#16305a", "#0c1c3a", "#4f7fc466"),
    null,
  ),
  item(
    "sleeve",
    "Midnight",
    sky("#1b2a4d", "#0e1830", "#070c1a", "#dfe6ff55"),
    "$4.99",
  ),
  item(
    "sleeve",
    "Ember",
    sky("#a8321c", "#5e1a16", "#1d0c12", "#ffb45e88"),
    "$2.99",
  ),
  item(
    "sleeve",
    "Moonstone",
    sky("#8fa9d6", "#4f6ea8", "#253a66", "#ffffff88"),
    "$4.99",
  ),
  item(
    "sleeve",
    "Forest",
    sky("#4f7d3a", "#2a4a25", "#0f1f12", "#d9e9a055"),
    "$2.99",
  ),
  item(
    "sleeve",
    "Storm",
    sky("#3a5a8c", "#1c2c4e", "#0a1226", "#bfe0ff77"),
    "$4.99",
  ),
  item(
    "sleeve",
    "Twilight",
    sky("#7a4a9c", "#3c2a66", "#16122e", "#ff9e7a77"),
    "$2.99",
  ),
  // Preserve the saved ID for the original preview back.
  {
    ...item("back", "Classic", { imageUrl: SPELLWAKE_CARD_BACK_URL }, null),
    name: "Spellwake",
  },
  item("back", "Magic", { imageUrl: MAGIC_CARD_BACK_URL }, null),
  item(
    "back",
    "Moon",
    sky("#1b2a4d", "#0e1830", "#070c1a", "#f3f0dd66"),
    "$2.99",
  ),
  item(
    "back",
    "Ember",
    sky("#a8321c", "#5e1a16", "#1d0c12", "#ffb45e88"),
    "$2.99",
  ),
  item("dice", "Ivory", paint("linear-gradient(145deg,#f4ead2,#cdbf9f)"), null),
  item(
    "dice",
    "Midnight",
    paint("linear-gradient(145deg,#27375e,#10182e)"),
    "$3.00",
  ),
  item(
    "dice",
    "Ember",
    paint("linear-gradient(145deg,#a02a22,#4c1212)"),
    "$3.00",
  ),
  item(
    "playmat",
    "Moonlit",
    sky("#1b2a4d", "#0e1830", "#070c1a", "#dfe6ff44"),
    "$4.99",
  ),
  item(
    "playmat",
    "Forest",
    sky("#4f7d3a", "#2a4a25", "#0f1f12", "#d9e9a044"),
    "$4.99",
  ),
  // The preview has no pets to show yet, so the category holds only None.
];
const OPTIONAL: readonly CosmeticKind[] = ["playmat", "pet"];
const id = (kind: CosmeticKind, name: string): string =>
  `preview-${kind}-${name.toLowerCase()}`;
const FIRST_DEFAULTS: DefaultSelection = {
  sleeve: id("sleeve", "Classic"),
  back: id("back", "Classic"),
  dice: id("dice", "Ivory"),
  playmat: null,
  pet: null,
};

const ownedAtom = atomWithStorage<string[]>(
  "spellwake:cosmetics:demo:owned:v1",
  [],
);
const defaultsAtom = atomWithStorage<DefaultSelection>(
  "spellwake:cosmetics:demo:defaults:v1",
  FIRST_DEFAULTS,
);

/** Read the preview's saved appearance without loading its cosmetic shop. */
export function useDemoDefaults(): DefaultSelection {
  return useAtomValue(defaultsAtom);
}
const pause = (ms: number): Promise<void> =>
  new Promise((resolve) => window.setTimeout(resolve, ms));

function useItems(): { items: ShopItem[]; own: (itemId: string) => void } {
  const [bought, setBought] = useAtom(ownedAtom);
  return {
    items: ITEMS.map((entry) =>
      bought.includes(entry.id) ? { ...entry, isOwned: true } : entry,
    ),
    own: (itemId) =>
      setBought((current) =>
        current.includes(itemId) ? current : [...current, itemId],
      ),
  };
}

export function useDemoDefaultSettings(): DefaultSettingsSource {
  const { items } = useItems();
  const [saved, setSaved] = useAtom(defaultsAtom);
  return {
    status: "ready",
    categories: COSMETIC_KINDS,
    optional: OPTIONAL,
    options: { state: "ready", items: items.filter((entry) => entry.isOwned) },
    saved,
    save: async (selection) => {
      await pause(600);
      setSaved(selection);
      return succeeded(undefined);
    },
  };
}

export function useDemoCosmetics(): CosmeticsSource {
  const { items, own } = useItems();
  const [chosen, setChosen] = useState<string | null>(null);
  const [notice, setNotice] = useState<CheckoutNotice | null>(null);
  const [isBusy, setIsBusy] = useState(false);
  const isOpen = useRef(true);
  useEffect(() => {
    isOpen.current = true;
    return () => {
      isOpen.current = false;
    };
  }, []);
  const say = (text: string, kind: CheckoutNotice["kind"]): void => {
    if (isOpen.current) setNotice({ text, kind });
  };
  return {
    status: "ready",
    categories: COSMETIC_KINDS,
    state: "ready",
    items,
    checkout: {
      choose: (itemId) => {
        setChosen(itemId);
        if (!isBusy) setNotice(null);
      },
      isBusy,
      notice,
      // Walks the approved steps with nothing behind them: no page opens and no money moves.
      buy: () => {
        const target = items.find((entry) => entry.id === chosen);
        if (!target || target.isOwned || !target.isSold || isBusy) return;
        setIsBusy(true);
        void (async () => {
          say("Opening checkout…", "working");
          await pause(900);
          say("Checkout opened.", "working");
          await pause(1400);
          say("Checking purchase…", "working");
          await pause(1200);
          own(target.id);
          say("Cosmetic added.", "result");
          if (isOpen.current) setIsBusy(false);
        })();
      },
    },
  };
}

/**
 * What a preview deck's appearance is chosen from, and the saved defaults a
 * new deck, or one saved before decks had an appearance, starts with.
 */
export function useDemoAppearance(): {
  options: AppearanceOptions;
  defaults: DefaultSelection;
} {
  const { items } = useItems();
  const [defaults] = useAtom(defaultsAtom);
  return {
    options: {
      categories: COSMETIC_KINDS,
      optional: OPTIONAL,
      items: { state: "ready", items: items.filter((entry) => entry.isOwned) },
    },
    defaults,
  };
}

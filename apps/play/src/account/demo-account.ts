import { useMemo, useState } from "react";
import { atom, useAtom, useAtomValue } from "jotai";
import { atomWithStorage } from "jotai/utils";
import { useBuilderCatalogue } from "../data/builder-catalogue";
import { failed, succeeded, type Outcome } from "../decks/deck-sources";
import type {
  AccountSettingsSource,
  ArtChoice,
  BadgeChoice,
  OwnIdentity,
  PatreonLink,
  PublicProfileSource,
  PurchaseRow,
  RankRow,
} from "./account-sources";
import { CENTRED, type PortraitCrop } from "./portrait-crop";

/**
 * The local preview of an account: a stand-in person whose picture, badge and
 * Patreon link are kept in this browser. Nothing here reaches an account, a
 * payment provider or Patreon's records.
 */
export const DEMO_USERNAME = "Planeswalker";

type DemoProfile = {
  art: ArtChoice | null;
  crop: PortraitCrop;
  badge: BadgeChoice;
  patreon: Exclude<PatreonLink, "unknown">;
};
const demoProfileAtom = atomWithStorage<DemoProfile>(
  "spellwake:account:demo:v1",
  { art: null, crop: CENTRED, badge: "none", patreon: "unlinked" },
);
/** Kept only until the page reloads, so the preview cannot lock itself out. */
const demoDeletionAtom = atom<"none" | "requested">("none");

const rank = (name: string): { name: string; emblemUrl: string } => ({
  name,
  emblemUrl: `${import.meta.env.BASE_URL}ranks/${name.toLowerCase()}.svg`,
});
const record = (
  wins: number,
  losses: number,
  draws = 0,
): RankRow["record"] => ({ wins, losses, draws });

const PURCHASES: readonly PurchaseRow[] = [
  ["Midnight sleeve", "2026-11-01", "$2.99 USD", "Paid"],
  ["Stormglass dice", "2026-11-02", "$3.00 USD", "Pending"],
  ["Forest sleeve", "2026-11-02", "$1.00 USD", "Refunded"],
  ["Moonstone back", "2026-11-03", "$1.00 USD", "Reversed"],
  ["Ember sleeve", "2026-11-05", "$2.99 USD", "Paid"],
  ["Twilight sleeve", "2026-11-08", "$4.99 USD", "Canceled"],
  ["Ivory dice", "2026-11-11", "$3.00 USD", "Paid"],
  ["Moonlit playmat", "2026-11-14", "$4.99 USD", "Paid"],
  ["Storm sleeve", "2026-11-17", "$2.99 USD", "Paid"],
].map(([item, date, amount, status], index) => ({
  id: `preview-purchase-${index + 1}`,
  item,
  date: `${date}T12:00:00Z`,
  amount,
  status,
})) as PurchaseRow[];
const PURCHASE_PAGE = 4;

const done = (): Promise<Outcome> => Promise.resolve(succeeded(undefined));
/** Long enough for a working notice to be read. */
const pause = (): Promise<void> =>
  new Promise((resolve) => window.setTimeout(resolve, 600));

/** The preview person's picture and badge, for the header. */
export function useDemoIdentity(): OwnIdentity {
  const profile = useAtomValue(demoProfileAtom);
  return {
    portrait: profile.art
      ? { artUrl: profile.art.artUrl, crop: profile.crop }
      : null,
    badge: profile.badge,
  };
}

export function useDemoAccountSettings(): AccountSettingsSource {
  const [profile, setProfile] = useAtom(demoProfileAtom);
  const [deletion, setDeletion] = useAtom(demoDeletionAtom);
  const { catalogue, hasFailed } = useBuilderCatalogue();
  const [query, setQuery] = useState("");
  const [pages, setPages] = useState(1);
  const results = useMemo(() => {
    const needle = query.trim().toLowerCase();
    return (catalogue?.cards ?? [])
      .filter((card) => card.name.toLowerCase().includes(needle))
      .sort((a, b) => a.name.localeCompare(b.name))
      .map((card) => ({
        id: card.name,
        name: card.name,
        artist: card.artist,
        artUrl: card.artUrl,
      }));
  }, [catalogue, query]);
  const shown = PURCHASES.slice(0, pages * PURCHASE_PAGE);
  return {
    status: "ready",
    username: DEMO_USERNAME,
    portrait: profile.art
      ? { artUrl: profile.art.artUrl, crop: profile.crop }
      : null,
    badge: profile.badge,
    picture: {
      search: {
        query,
        setQuery,
        state: hasFailed ? "failed" : catalogue ? "ready" : "loading",
        results,
      },
      save: async (art, crop) => {
        await pause();
        setProfile((current) => ({ ...current, art, crop }));
        return succeeded(undefined);
      },
    },
    badges: {
      canUseSupporter: profile.patreon === "active",
      save: async (badge) => {
        await pause();
        if (badge === "supporter" && profile.patreon !== "active")
          return failed("Couldn’t save badge.");
        setProfile((current) => ({ ...current, badge }));
        return succeeded(undefined);
      },
    },
    patreon: {
      link: profile.patreon,
      // Stands in for the round trip through Patreon's own authorisation.
      connect: async () => {
        await pause();
        setProfile((current) => ({ ...current, patreon: "active" }));
        return succeeded(undefined);
      },
      manage: () => {
        window.open(
          "https://www.patreon.com/settings/memberships",
          "_blank",
          "noopener",
        );
        return done();
      },
      isLinking: false,
      problem: null,
      dismissProblem: () => undefined,
    },
    billing: {
      // There is no hosted billing page to open from a preview.
      manage: null,
      purchases: {
        state: "ready",
        rows: shown,
        loadMore:
          shown.length < PURCHASES.length
            ? () => setPages((count) => count + 1)
            : null,
      },
    },
    deletion: {
      state: deletion,
      request: async () => {
        await pause();
        setDeletion("requested");
        return succeeded(undefined);
      },
    },
  };
}

const SEASON = "Oct–Dec 2026";
/** Stand-in players the preview can show a profile for. */
const PLAYERS: Readonly<
  Record<
    string,
    {
      username: string;
      art: string | null;
      canMessage: boolean;
      ranks: RankRow[];
    }
  >
> = {
  emberfox: {
    username: "EmberFox",
    art: "Goblin Guide",
    canMessage: true,
    ranks: [
      {
        format: "Modern",
        tier: rank("Mythic"),
        rating: 2216,
        record: record(40, 12),
      },
      {
        format: "Standard",
        tier: rank("Gold"),
        rating: 1420,
        record: record(18, 12),
      },
      {
        format: "Commander",
        tier: rank("Silver"),
        rating: 1240,
        record: record(12, 8),
      },
    ],
  },
  mosswitch: {
    username: "Mosswitch",
    art: "Aether Vial",
    canMessage: false,
    ranks: [
      {
        format: "Modern",
        tier: rank("Mythic"),
        rating: 2132,
        record: record(35, 15),
      },
      {
        format: "Standard",
        tier: rank("Diamond"),
        rating: 1688,
        record: record(27, 14, 1),
      },
      {
        format: "Commander",
        tier: rank("Platinum"),
        rating: 1502,
        record: record(22, 16),
      },
      {
        format: "Legacy",
        tier: rank("Gold"),
        rating: 1384,
        record: record(14, 11),
      },
      {
        format: "Pioneer",
        tier: rank("Gold"),
        rating: 1310,
        record: record(11, 9, 2),
      },
      {
        format: "Pauper",
        tier: rank("Silver"),
        rating: 1205,
        record: record(9, 8),
      },
      {
        format: "Vintage",
        tier: rank("Bronze"),
        rating: 1040,
        record: record(3, 6),
      },
      { format: "Premodern", tier: null, rating: 1000, record: record(0, 0) },
    ],
  },
  moonwatcher: {
    username: "Moonwatcher",
    art: "Conspicuous Snoop",
    canMessage: false,
    ranks: [],
  },
  // A long name, to hold the layout to its reserved column.
  archivist: {
    username: "Grand_Archivist_of_the_Ninth",
    art: null,
    canMessage: true,
    ranks: [
      {
        format: "Modern",
        tier: rank("Platinum"),
        rating: 1544,
        record: record(21, 17),
      },
    ],
  },
};

/**
 * A preview profile by ID. `me` is the preview person's own public profile.
 * Any other unknown ID is an unavailable player.
 */
export function useDemoPublicProfile(playerId: string): PublicProfileSource {
  const own = useAtomValue(demoProfileAtom);
  const { catalogue, hasFailed } = useBuilderCatalogue();
  if (playerId === "me")
    return {
      status: "ready",
      username: DEMO_USERNAME,
      portrait: own.art ? { artUrl: own.art.artUrl, crop: own.crop } : null,
      badge: own.badge,
      season: SEASON,
      ranks: [
        {
          format: "Modern",
          tier: rank("Silver"),
          rating: 1240,
          record: record(12, 8),
        },
      ],
      isStale: false,
      message: null,
    };
  const player = PLAYERS[playerId];
  if (!player) return { status: "unavailable" };
  if (!catalogue && !hasFailed) return { status: "loading" };
  const art = catalogue?.cards.find((card) => card.name === player.art);
  return {
    status: "ready",
    username: player.username,
    portrait: art ? { artUrl: art.artUrl, crop: CENTRED } : null,
    badge: playerId === "emberfox" ? "supporter" : "none",
    season: SEASON,
    ranks: player.ranks,
    isStale: false,
    // Messaging is not built, so the conversation cannot open yet.
    message: player.canMessage
      ? () => Promise.resolve(failed("Couldn’t open conversation."))
      : null,
  };
}

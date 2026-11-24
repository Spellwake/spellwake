/**
 * Stress qualification for the application frame against a running dev server.
 *
 *   bun apps/play/scripts/qualify-frame.ts <origin> <output-dir> <dark-art> <bright-art> [filter]
 *
 * The origin is the Play dev server base, for example http://127.0.0.1:6204/play.
 * The two artwork files stand in for whatever backdrop art is supplied later:
 * requests for the backdrop descriptor and its image are answered from here, so
 * the product source needs no test switch. Every case is captured and checked
 * for overflow, see-through surfaces, target sizes, overlap and focus cover.
 * An optional filter runs only the cases whose name contains it.
 */
import {
  chromium,
  expect,
  type Browser,
  type Locator,
  type Page,
} from "@playwright/test";
import { mkdir, readFile, writeFile } from "node:fs/promises";
import { resolve } from "node:path";
import {
  createDemoSeed,
  createStressDecks,
} from "../src/collection/data/demo-seed";

const [origin, outputArgument, darkArt, brightArt, filter] =
  process.argv.slice(2);
if (!origin || !outputArgument || !darkArt || !brightArt)
  throw new Error(
    "Usage: qualify-frame.ts <origin> <output-dir> <dark-art> <bright-art>",
  );
if (new URL(origin).hostname !== "127.0.0.1")
  throw new Error("Owned loopback dev server URL required.");
const output = resolve(outputArgument);
const base = origin.replace(/\/$/, "");
const basePath = new URL(base).pathname;

type Art = "none" | "dark" | "bright";
type Decks = "six" | "many" | "none" | "builder";
type Viewport = {
  name: string;
  width: number;
  height: number;
  touch: boolean;
};
type Case = {
  viewport: Viewport;
  art: Art;
  decks: Decks;
  path: string;
  /** Extra condition exercised on top of the plain render. */
  state?:
    | "menu"
    | "end-of-list"
    | "reduced-transparency"
    | "forced-colors"
    /** Account settings editors, opened from the Profile and Account sections. */
    | "picture"
    | "crop"
    | "badge"
    | "delete"
    /** Import deck on its File tab, and a deck's Appearance from the builder. */
    | "import-file"
    | "appearance"
    /** Cards with its filters open, a card's details, and a pack opened and revealed. */
    | "card-filters"
    | "card-open"
    | "pack-open"
    /** The lobby's Choose Deck, Game rules and Invite players. */
    | "lobby-deck"
    | "lobby-rules"
    | "lobby-invite"
    /** Trade's Add items, its cancel confirmation, and Edit items on a ready offer. */
    | "trade-picker"
    | "trade-cancel"
    | "trade-edit"
    /** Market Listings' filters, Buy, both stages of Create listing, and Cancel listing. */
    | "market-filters"
    | "market-buy"
    | "market-create"
    | "market-create-terms"
    | "market-cancel"
    /** Supply's Buy packs (and with three typed), and its vote. */
    | "supply-buy"
    | "supply-buy-three"
    | "supply-vote"
    /** Predictions: a bet (and armed), a read-only view, Create, its picker and calendar. */
    | "prediction-bet"
    | "prediction-bet-armed"
    | "prediction-view"
    | "prediction-mine-view"
    | "prediction-create"
    | "prediction-picker"
    | "prediction-calendar"
    /** League: Standings scrolled to your row, and a player's profile from it. */
    | "league-scrolled"
    | "league-profile"
    /** The match: another seat's board, and the whole table. */
    | "match-opponent"
    | "match-overview"
    /** Declaring: an attacker sent at a player, and a blocker linked from the whole table. */
    | "match-attack"
    | "match-block"
    /** Combat damage: a point moved between targets, a second source, a player target, and Confirm. */
    | "match-damage"
    | "match-damage-two"
    | "match-trample"
    /** The stack: a spell read in place, the oldest of a long pile, and both resolved. */
    | "match-stack-read"
    | "match-stack-oldest"
    | "match-resolve"
    /** Table chat and settings: a message sent, one refused, settings changed, conceding and watching. */
    | "match-chat-send"
    | "match-chat-refused"
    | "match-settings"
    | "match-concede"
    | "match-watch-chat"
    | "match-end"
    | "match-end-view"
    | "match-end-chat"
    | "league-history"
    | "quests"
    | "play-choose"
    | "play-options"
    | "play-search"
    | "play-refused"
    | "play-found"
    | "match-series"
    | "match-series-next"
    | "match-series-leave"
    /** Casting: waiting for a target, a player targeted, a paused payment, and a creature. */
    | "match-cast-target"
    | "match-cast-player"
    | "match-cast-pay"
    | "match-cast-paid"
    | "match-cast-creature"
    /** A modal spell: its modes, one with no legal target, a look at the table, and a mode cast. */
    | "match-mode-choice"
    | "match-mode-unavailable"
    | "match-mode-peek"
    | "match-mode-cast"
    /** A spell that asks for two modes: marking them, and each way it goes on. */
    | "match-two-modes"
    | "match-two-modes-quiet"
    | "match-two-modes-cast"
    /** Two instructions with a target each: the second, going back, and the cast. */
    | "match-two-targets"
    | "match-two-targets-back"
    /** A spell on the stack, then a permanent. */
    | "match-mixed-targets"
    | "match-mixed-cast"
    /** A divided amount: three targets, adjusting and removing, and the cast. */
    | "match-divide"
    | "match-divide-adjust"
    | "match-divide-cast";
};

const viewports: Viewport[] = [
  { name: "phone-small", width: 320, height: 568, touch: true },
  { name: "phone", width: 390, height: 844, touch: true },
  { name: "phone-large", width: 430, height: 932, touch: true },
  { name: "phone-landscape", width: 844, height: 390, touch: true },
  { name: "tablet-portrait", width: 768, height: 1024, touch: true },
  { name: "tablet-landscape", width: 1024, height: 768, touch: true },
  { name: "laptop", width: 1280, height: 720, touch: false },
  { name: "desktop", width: 1440, height: 900, touch: false },
  { name: "desktop-large", width: 1920, height: 1080, touch: false },
  { name: "ultrawide", width: 2560, height: 1080, touch: false },
];
/**
 * A second press on an offered hand card casts it. A held card is fanned,
 * tilted and half below the edge, so the press goes where the card shows.
 */
async function pressHandCard(page: Page, name: string): Promise<void> {
  const card = page.getByRole("button", {
    name: `${name}, in your hand`,
    exact: true,
  });
  const point = await card.evaluate((node) => {
    const box = node.getBoundingClientRect();
    for (let row = 1; row < 12; row += 1)
      for (let column = 1; column < 12; column += 1) {
        const x = box.left + (box.width * column) / 12;
        const y = box.top + (box.height * row) / 12;
        if (y > window.innerHeight - 6 || x < 4 || x > window.innerWidth - 4)
          continue;
        const hit = document.elementFromPoint(x, y);
        if (hit && node.contains(hit)) return { x, y };
      }
    return null;
  });
  if (!point) throw new Error(`${name} shows no part that can be pressed.`);
  await page.mouse.dblclick(point.x, point.y);
}
const expectText = (locator: Locator, text: string): Promise<void> =>
  expect(locator).toHaveText(text);
const named = (name: string): Viewport => {
  const found = viewports.find((viewport) => viewport.name === name);
  if (!found) throw new Error(`Unknown viewport ${name}.`);
  return found;
};
const shelf = "/collection/decks";
/** The preview lobby, opened with the Open Play shelf's first deck. */
const LOBBY_PATH = "/open-play/lobby?deck=00000000-0000-4000-8000-000000000001";
/** Market Supply with the preview supply. */
const SUPPLY_PATH = "/market/supply";
/** Market Predictions with the preview questions. */
const PREDICTIONS_PATH = "/market/predictions";
/** Market Listings with the preview market. */
const LISTINGS_PATH = "/market/listings";
/** The preview trade room with Mira, as Messages opens it. */
const TRADE_PATH = "/collection/trade/mira";
/** The Collection shelf's first example deck, filled with the approved reference list. */
const BUILDER_DECK_ID = "00000000-0000-4000-8100-000000000001";
const BUILDER_PATH = `/decks/${BUILDER_DECK_ID}`;
const BUILDER_LIST: readonly (readonly [string, number])[] = [
  ["Skirk Prospector", 4],
  ["Mogg War Marshal", 4],
  ["Goblin Instigator", 4],
  ["Goblin Warchief", 4],
  ["Goblin Chieftain", 4],
  ["Goblin Matron", 4],
  ["Lightning Bolt", 4],
  ["Shock", 4],
  ["Krenko's Command", 4],
  ["Mountain", 20],
];
const cases: Case[] = [
  ...viewports.map((viewport): Case => ({
    viewport,
    art: "dark",
    decks: "six",
    path: shelf,
  })),
  // Messages on every screen, then its views and states where space is tightest.
  ...viewports.map((viewport): Case => ({
    viewport,
    art: "dark",
    decks: "six",
    path: `${shelf}?messages=mira`,
  })),
  ...(
    [
      "phone-small",
      "phone",
      "phone-landscape",
      "tablet-portrait",
      "desktop",
    ] as const
  ).flatMap((name): Case[] => [
    ...["requests", "find", "nia"].map((view): Case => ({
      viewport: named(name),
      art: "dark",
      decks: "six",
      path: `${shelf}?messages=${view}`,
    })),
    ...["unconfirmed", "no-contact", "deleted", "loading", "no-requests"].map(
      (probe): Case => ({
        viewport: named(name),
        art: "dark",
        decks: "six",
        path: `/probe/messages?state=${probe}`,
      }),
    ),
  ]),
  // The lobby on every screen, then its dialogs and states where space is tightest.
  ...viewports.map((viewport): Case => ({
    viewport,
    art: "dark",
    decks: "six",
    path: LOBBY_PATH,
  })),
  ...(
    [
      "phone-small",
      "phone",
      "phone-landscape",
      "tablet-portrait",
      "desktop",
    ] as const
  ).flatMap((name): Case[] => [
    ...(["lobby-deck", "lobby-rules", "lobby-invite"] as const).map(
      (state): Case => ({
        viewport: named(name),
        art: "dark",
        decks: "six",
        path: LOBBY_PATH,
        state,
      }),
    ),
    ...[
      "member",
      "countdown",
      "starting",
      "start-unconfirmed",
      "needs-deck",
      "close",
      "closure-unconfirmed",
      "closed",
      "invite-link",
    ].map((probe): Case => ({
      viewport: named(name),
      art: "dark",
      decks: "six",
      path: `/probe/lobby?state=${probe}`,
    })),
  ]),
  // Trade on every screen, then its overlays and states where space is tightest.
  ...viewports.map((viewport): Case => ({
    viewport,
    art: "dark",
    decks: "six",
    path: TRADE_PATH,
  })),
  ...(
    [
      "phone-small",
      "phone",
      "phone-landscape",
      "tablet-portrait",
      "desktop",
    ] as const
  ).flatMap((name): Case[] => [
    ...(["trade-picker", "trade-cancel"] as const).map((state): Case => ({
      viewport: named(name),
      art: "dark",
      decks: "six",
      path: TRADE_PATH,
      state,
    })),
    {
      viewport: named(name),
      art: "dark",
      decks: "six",
      path: "/probe/trade?state=both-ready",
      state: "trade-edit",
    },
    ...[
      "editing",
      "dense",
      "short",
      "reserving",
      "rejected",
      "waiting",
      "confirmed",
      "unconfirmed",
      "checking",
      "complete",
      "canceled",
      "unavailable",
      "loading",
    ].map((probe): Case => ({
      viewport: named(name),
      art: "dark",
      decks: "six",
      path: `/probe/trade?state=${probe}`,
    })),
  ]),
  // Market Supply on every screen, then its dialogs and states where space is tightest.
  ...viewports.map((viewport): Case => ({
    viewport,
    art: "dark",
    decks: "six",
    path: SUPPLY_PATH,
  })),
  ...(
    [
      "phone-small",
      "phone",
      "phone-landscape",
      "tablet-portrait",
      "desktop",
    ] as const
  ).flatMap((name): Case[] => {
    const at = (path: string, state?: Case["state"]): Case => ({
      viewport: named(name),
      art: "dark",
      decks: "six",
      path,
      ...(state ? { state } : {}),
    });
    const probe = (state: string) => `/probe/supply?state=${state}`;
    return [
      at(SUPPLY_PATH, "supply-buy"),
      at(SUPPLY_PATH, "supply-vote"),
      at(`${SUPPLY_PATH}?q=zzzz`),
      at(`${SUPPLY_PATH}?availability=sold-out`),
      ...[
        "loading",
        "read-failed",
        "empty",
        "checking",
        "appending",
        "buy-pending",
        "buy-unconfirmed",
        "buy-rejected",
        "vote-pending",
        "vote-unconfirmed",
        "no-ballot",
      ].map((state) => at(probe(state))),
      ...[
        "buy-quote-loading",
        "buy-quote-failed",
        "buy-changed",
        "buy-no-coins",
      ].map((state) => at(probe(state), "supply-buy")),
      at(probe("buy-short"), "supply-buy-three"),
      ...[
        "vote-loading",
        "vote-none",
        "vote-unavailable",
        "vote-not-eligible",
        "vote-read-failed",
        "vote-limit",
        "vote-closed",
        "vote-results",
      ].map((state) => at(probe(state), "supply-vote")),
    ];
  }),
  // The match table on every screen: your board, another seat's, and the whole table.
  ...viewports.flatMap((viewport): Case[] =>
    ([undefined, "match-opponent", "match-overview"] as const).map((state) => ({
      viewport,
      art: "none",
      decks: "six",
      path: "/match",
      ...(state ? { state } : {}),
    })),
  ),
  // Declaring attackers and blockers on the table, on every screen.
  ...viewports.flatMap((viewport): Case[] => [
    {
      viewport,
      art: "none",
      decks: "six",
      path: "/probe/table?state=attack",
    },
    {
      viewport,
      art: "none",
      decks: "six",
      path: "/probe/table?state=attack",
      state: "match-attack",
    },
    {
      viewport,
      art: "none",
      decks: "six",
      path: "/probe/table?state=block",
    },
    {
      viewport,
      art: "none",
      decks: "six",
      path: "/probe/table?state=block",
      state: "match-block",
    },
  ]),
  // Assigning combat damage and reading the stack, on every screen.
  ...viewports.flatMap((viewport): Case[] =>
    (
      [
        ["damage", undefined],
        ["damage", "match-damage"],
        ["damage-two", "match-damage-two"],
        ["trample", "match-trample"],
        ["stack", undefined],
        ["stack", "match-stack-read"],
        ["stack", "match-resolve"],
        ["stack-long", undefined],
        ["stack-long", "match-stack-oldest"],
        ["stack-waiting", undefined],
      ] as const
    ).map(([scene, state]) => ({
      viewport,
      art: "none",
      decks: "six",
      path: `/probe/table?state=${scene}`,
      ...(state ? { state } : {}),
    })),
  ),
  // Table chat, settings, conceding and watching, on every screen.
  ...viewports.flatMap((viewport): Case[] =>
    (
      [
        ["/probe/table", "match-chat-send"],
        ["/probe/table?chat=refused", "match-chat-refused"],
        ["/probe/table", "match-settings"],
        ["/probe/table", "match-concede"],
        ["/probe/table?state=watching", undefined],
        ["/probe/table?state=watching", "match-watch-chat"],
        // The end of a game: each way it can come out, the finished table
        // behind the lifted band, and a last word in chat.
        ["/probe/table?end=won", "match-end"],
        ["/probe/table?end=lost", "match-end"],
        ["/probe/table?end=draw", "match-end"],
        ["/probe/table?end=none", "match-end"],
        ["/probe/table?end=won", "match-end-view"],
        ["/probe/table?end=won", "match-end-chat"],
        // One game of a best of three: what the next game waits on, starting
        // it, leaving between games, and the match decided.
        ["/probe/table?end=won&series=deciding", "match-series"],
        ["/probe/table?end=lost&series=stuck", "match-series"],
        ["/probe/table?end=won&series=untimed", "match-series"],
        ["/probe/table?end=won&series=waiting", "match-series"],
        ["/probe/table?end=lost&series=over", "match-series"],
        ["/probe/table?end=won&series=deciding", "match-series-next"],
        ["/probe/table?end=won&series=deciding", "match-series-leave"],
      ] as const
    ).map(([path, state]) => ({
      viewport,
      art: "none",
      decks: "six",
      path,
      ...(state ? { state } : {}),
    })),
  ),
  // Casting from hand, on every screen.
  ...viewports.flatMap((viewport): Case[] =>
    (
      [
        ["/probe/table", "match-cast-target"],
        ["/probe/table", "match-cast-player"],
        ["/probe/table?pay=paused", "match-cast-pay"],
        ["/probe/table?pay=paused", "match-cast-paid"],
        ["/probe/table", "match-cast-creature"],
      ] as const
    ).map(([path, state]) => ({
      viewport,
      art: "none",
      decks: "six",
      path,
      state,
    })),
  ),
  // Two modes, two targets, a spell and a permanent, and a divided amount, on every screen.
  ...viewports.flatMap((viewport): Case[] =>
    (
      [
        ["/probe/table", "match-two-modes"],
        ["/probe/table", "match-two-modes-quiet"],
        ["/probe/table", "match-two-modes-cast"],
        ["/probe/table", "match-two-targets"],
        ["/probe/table", "match-two-targets-back"],
        ["/probe/table?state=stack", "match-mixed-targets"],
        ["/probe/table?state=stack", "match-mixed-cast"],
        ["/probe/table", "match-divide"],
        ["/probe/table", "match-divide-adjust"],
        ["/probe/table", "match-divide-cast"],
      ] as const
    ).map(([path, state]) => ({
      viewport,
      art: "none",
      decks: "six",
      path,
      state,
    })),
  ),
  // Choosing a modal spell's mode, on every screen.
  ...viewports.flatMap((viewport): Case[] =>
    (
      [
        ["/probe/table", "match-mode-choice"],
        ["/probe/table?state=no-artifacts", "match-mode-unavailable"],
        ["/probe/table", "match-mode-peek"],
        ["/probe/table", "match-mode-cast"],
      ] as const
    ).map(([path, state]) => ({
      viewport,
      art: "none",
      decks: "six",
      path,
      state,
    })),
  ),
  // League on every screen, both sections, then its formats and states.
  ...viewports.flatMap((viewport): Case[] =>
    ["/league/season", "/league/standings"].map((path) => ({
      viewport,
      art: "dark",
      decks: "six",
      path,
    })),
  ),
  // Daily quests under the decks, on every screen: three quests and a swap,
  // one quest with nothing won, a refused swap, and no quests at all.
  ...viewports.flatMap((viewport): Case[] =>
    (
      [
        ["/collection/decks", "quests"],
        ["/open-play", "quests"],
        ["/collection/decks?quests=one", "quests"],
        ["/collection/decks?quests=refused", "quests"],
        ["/collection/decks?quests=none", "quests"],
      ] as const
    ).map(([path, state]) => ({
      viewport,
      art: "none",
      decks: "six",
      path,
      state,
    })),
  ),
  // Play on a deck, on every screen: the two ways to play, the match options,
  // a search that waits and is cancelled, one that cannot start, a format
  // with one match length or none, Open Play without rank, and a found match.
  ...viewports.flatMap((viewport): Case[] =>
    (
      [
        ["/collection/decks", "play-choose"],
        ["/collection/decks?search=none", "play-choose"],
        ["/collection/decks", "play-options"],
        ["/collection/decks?search=single-only", "play-options"],
        ["/open-play", "play-options"],
        ["/collection/decks?search=waits", "play-search"],
        ["/open-play?search=waits", "play-search"],
        ["/collection/decks?search=refused", "play-refused"],
        ["/collection/decks", "play-found"],
      ] as const
    ).map(([path, state]) => ({
      viewport,
      art: "none",
      decks: "six",
      path,
      state,
    })),
  ),
  // League · History on every screen: the list read to its end, no matches,
  // slow reads and a failed read.
  ...viewports.flatMap((viewport): Case[] =>
    (
      [
        ["/league/history", "league-history"],
        ["/league/history?history=empty", "league-history"],
        ["/league/history?history=slow", "league-history"],
        ["/league/history?history=failed", "league-history"],
      ] as const
    ).map(([path, state]) => ({
      viewport,
      art: "dark",
      decks: "six",
      path,
      state,
    })),
  ),
  ...(
    [
      "phone-small",
      "phone",
      "phone-landscape",
      "tablet-portrait",
      "desktop",
    ] as const
  ).flatMap((name): Case[] => {
    const at = (path: string, state?: Case["state"]): Case => ({
      viewport: named(name),
      art: "dark",
      decks: "six",
      path,
      ...(state ? { state } : {}),
    });
    const standings = (format: string) =>
      `/league/standings?format=preview-format-${format}`;
    const probe = (state: string, page = "") =>
      `/probe/league?state=${state}${page ? `&page=${page}` : ""}`;
    return [
      ...["standard", "premodern", "pioneer"].map((format) =>
        at(standings(format)),
      ),
      at(standings("modern"), "league-scrolled"),
      at(standings("modern"), "league-profile"),
      ...[
        "loading",
        "signed-out",
        "unavailable",
        "ranks-loading",
        "ranks-failed",
        "ranks-refresh-failed",
        "all-new",
        "many-formats",
      ].map((state) => at(probe(state))),
      ...[
        "loading",
        "unavailable",
        "standings-loading",
        "standings-failed",
        "standings-refresh-failed",
        "standings-appending",
        "append-failed",
        "own-unavailable",
        "many-formats",
      ].map((state) => at(probe(state, "standings"))),
    ];
  }),
  // Market Predictions on every screen, both tabs, then its dialogs and states.
  ...viewports.flatMap((viewport): Case[] =>
    [PREDICTIONS_PATH, `${PREDICTIONS_PATH}?tab=mine`].map((path) => ({
      viewport,
      art: "dark",
      decks: "six",
      path,
    })),
  ),
  ...(
    [
      "phone-small",
      "phone",
      "phone-landscape",
      "tablet-portrait",
      "desktop",
    ] as const
  ).flatMap((name): Case[] => {
    const at = (path: string, state?: Case["state"]): Case => ({
      viewport: named(name),
      art: "dark",
      decks: "six",
      path,
      ...(state ? { state } : {}),
    });
    const probe = (state: string) => `/probe/predictions?state=${state}`;
    return [
      at(PREDICTIONS_PATH, "prediction-bet"),
      at(PREDICTIONS_PATH, "prediction-bet-armed"),
      at(PREDICTIONS_PATH, "prediction-view"),
      at(`${PREDICTIONS_PATH}?tab=mine`, "prediction-mine-view"),
      at(PREDICTIONS_PATH, "prediction-create"),
      at(PREDICTIONS_PATH, "prediction-picker"),
      at(PREDICTIONS_PATH, "prediction-calendar"),
      at(`${PREDICTIONS_PATH}?q=zzzz`),
      at(`${PREDICTIONS_PATH}?tab=mine&mq=ring`),
      at(`${PREDICTIONS_PATH}?tab=mine&mq=zzzz`),
      ...[
        "loading",
        "signed-out",
        "unavailable",
        "explore-loading",
        "explore-failed",
        "explore-empty",
        "explore-appending",
        "refresh-failed",
        "result-unknown",
      ].map((state) => at(probe(state))),
      ...[
        "mine-loading",
        "mine-failed",
        "mine-partial",
        "mine-empty",
        "mine-long",
      ].map((state) => at(`${probe(state)}&tab=mine`)),
      ...[
        "bet-checking",
        "bet-failed",
        "bet-expired",
        "bet-none",
        "bet-side",
        "bet-too-much",
        "bet-pending",
        "bet-closed",
        "bet-not-accepting",
        "no-coins",
      ].map((state) => at(probe(state), "prediction-bet")),
      at(probe("bet-changed"), "prediction-bet-armed"),
      at(probe("bet-failure"), "prediction-bet-armed"),
      at(probe("result-unverified"), "prediction-view"),
      ...["create-pending", "create-failed"].map((state) =>
        at(probe(state), "prediction-create"),
      ),
      ...["create-checking", "create-unavailable", "create-price-failed"].map(
        (state) => at(probe(state), "prediction-calendar"),
      ),
      ...["picker-loading", "picker-failed", "picker-appending"].map((state) =>
        at(probe(state), "prediction-picker"),
      ),
    ];
  }),
  // Market Listings on every screen, then its dialogs and states where space is tightest.
  ...viewports.map((viewport): Case => ({
    viewport,
    art: "dark",
    decks: "six",
    path: LISTINGS_PATH,
  })),
  ...(
    [
      "phone-small",
      "phone",
      "phone-landscape",
      "tablet-portrait",
      "desktop",
    ] as const
  ).flatMap((name): Case[] => [
    ...(
      [
        "market-filters",
        "market-buy",
        "market-create",
        "market-create-terms",
      ] as const
    ).map((state): Case => ({
      viewport: named(name),
      art: "dark",
      decks: "six",
      path: LISTINGS_PATH,
      state,
    })),
    {
      viewport: named(name),
      art: "dark",
      decks: "six",
      path: "/probe/market?state=mine",
      state: "market-cancel",
    },
    ...[`${LISTINGS_PATH}?q=zzzz`, `${LISTINGS_PATH}?mine=true`].map(
      (path): Case => ({
        viewport: named(name),
        art: "dark",
        decks: "six",
        path,
      }),
    ),
    ...[
      "loading",
      "buy-pending",
      "buy-unconfirmed",
      "buy-checking",
      "buy-rejected",
      "create-pending",
      "create-unconfirmed",
      "cancel-unconfirmed",
      "read-failed",
      "empty",
      "no-assets",
      "pack-selected",
      "not-allowed",
    ].map((probe): Case => ({
      viewport: named(name),
      art: "dark",
      decks: "six",
      path: `/probe/market?state=${probe}`,
    })),
  ]),
  // Cards and Packs on every screen, then their overlays and states where space is tightest.
  ...viewports.flatMap((viewport): Case[] => [
    { viewport, art: "dark", decks: "six", path: "/collection/cards" },
    { viewport, art: "bright", decks: "six", path: "/collection/packs" },
  ]),
  ...(
    [
      "phone-small",
      "phone",
      "phone-landscape",
      "tablet-portrait",
      "desktop",
    ] as const
  ).flatMap((name): Case[] => [
    ...(["card-filters", "card-open"] as const).map((state): Case => ({
      viewport: named(name),
      art: "dark",
      decks: "six",
      path: "/collection/cards",
      state,
    })),
    {
      viewport: named(name),
      art: "dark",
      decks: "six",
      path: "/collection/packs",
      state: "pack-open",
    },
    ...[
      "cards-empty",
      "cards-unavailable",
      "cards-no-artwork",
      "packs-empty",
      "packs-restricted",
      "opening-unconfirmed",
    ].map((probe): Case => ({
      viewport: named(name),
      art: "dark",
      decks: "six",
      path: `/probe/collection?state=${probe}`,
    })),
  ]),
  ...(["phone", "desktop"] as const).flatMap((name): Case[] => [
    { viewport: named(name), art: "bright", decks: "six", path: shelf },
    { viewport: named(name), art: "none", decks: "six", path: shelf },
    { viewport: named(name), art: "bright", decks: "none", path: shelf },
    {
      viewport: named(name),
      art: "dark",
      decks: "many",
      path: shelf,
      state: "end-of-list",
    },
    {
      viewport: named(name),
      art: "bright",
      decks: "six",
      path: shelf,
      state: "reduced-transparency",
    },
    {
      viewport: named(name),
      art: "bright",
      decks: "six",
      path: shelf,
      state: "forced-colors",
    },
    { viewport: named(name), art: "bright", decks: "six", path: "/open-play" },
    {
      viewport: named(name),
      art: "dark",
      decks: "six",
      path: "/market/listings",
    },
  ]),
  // The focus frame under the real deck builder, and the immersive frame under a six-seat match probe.
  ...viewports.flatMap((viewport): Case[] => [
    { viewport, art: "dark", decks: "builder", path: BUILDER_PATH },
    { viewport, art: "none", decks: "six", path: "/probe/match" },
  ]),
  // Account entry: the tallest card on every screen, and the code and refused-name states where space is tightest.
  ...viewports.map((viewport): Case => ({
    viewport,
    art: "bright",
    decks: "six",
    path: "/probe/entry?state=invalid-email",
  })),
  ...(["phone-small", "phone-landscape", "desktop"] as const).flatMap(
    (name): Case[] => [
      {
        viewport: named(name),
        art: "dark",
        decks: "six",
        path: "/probe/entry?state=checking",
      },
      {
        viewport: named(name),
        art: "dark",
        decks: "six",
        path: "/probe/entry?state=taken",
      },
    ],
  ),
  // The card that holds an account awaiting deletion.
  ...(["phone-small", "phone-landscape", "desktop"] as const).map(
    (name): Case => ({
      viewport: named(name),
      art: "bright",
      decks: "six",
      path: "/probe/entry?state=deletion-pending",
    }),
  ),
  // Account settings on every screen, then each section, editor and profile where space is tightest.
  ...viewports.map((viewport): Case => ({
    viewport,
    art: "bright",
    decks: "six",
    path: `${shelf}?account=profile`,
  })),
  ...(["phone-small", "phone", "phone-landscape", "desktop"] as const).flatMap(
    (name): Case[] => [
      ...["patreon", "billing", "account"].map((section): Case => ({
        viewport: named(name),
        art: "dark",
        decks: "six",
        path: `${shelf}?account=${section}`,
      })),
      ...(["picture", "crop", "badge"] as const).map((state): Case => ({
        viewport: named(name),
        art: "dark",
        decks: "six",
        path: `${shelf}?account=profile`,
        state,
      })),
      {
        viewport: named(name),
        art: "dark",
        decks: "six",
        path: `${shelf}?account=account`,
        state: "delete",
      },
      ...[
        "defaults=true",
        "cosmetics=owned",
        "cosmetics=shop",
        "import=true",
      ].map((query): Case => ({
        viewport: named(name),
        art: "dark",
        decks: "six",
        path: `${shelf}?${query}`,
      })),
      {
        viewport: named(name),
        art: "dark",
        decks: "six",
        path: `${shelf}?import=true`,
        state: "import-file",
      },
      {
        viewport: named(name),
        art: "dark",
        decks: "builder",
        path: BUILDER_PATH,
        state: "appearance",
      },
      ...["emberfox", "mosswitch", "archivist", "moonwatcher", "nobody"].map(
        (player): Case => ({
          viewport: named(name),
          art: "bright",
          decks: "six",
          path: `${shelf}?player=${player}`,
        }),
      ),
      ...[
        "billing-failed",
        "deletion-requested",
        "no-commands",
        "profile-loading",
        "profile-stale",
      ].map((probe): Case => ({
        viewport: named(name),
        art: "dark",
        decks: "six",
        path: `/probe/account?state=${probe}`,
      })),
    ],
  ),
  {
    viewport: named("desktop"),
    art: "bright",
    decks: "six",
    path: shelf,
    state: "menu",
  },
  {
    viewport: named("phone"),
    art: "bright",
    decks: "six",
    path: shelf,
    state: "menu",
  },
];

const library = (decks: Decks): string | null => {
  if (decks === "six") return null;
  if (decks === "builder")
    return JSON.stringify({
      version: 1,
      decks: createDemoSeed().decks.map((deck) => {
        const id = deck.id.replace("-8000-", "-8100-");
        return {
          ...deck,
          id,
          mode: "collection",
          ...(id === BUILDER_DECK_ID
            ? {
                format: "Modern",
                entries: BUILDER_LIST.map(([name, quantity]) => ({
                  id: crypto.randomUUID(),
                  name,
                  quantity: String(quantity),
                  section: "main",
                })),
              }
            : {}),
        };
      }),
    });
  const now = "2026-11-01T00:00:00.000Z";
  return JSON.stringify({
    version: 1,
    decks:
      decks === "none"
        ? []
        : createStressDecks(160).map((deck) => ({
            ...deck,
            mode: "collection",
            createdAt: now,
            updatedAt: now,
          })),
  });
};

type Finding = { check: string; detail: string };

/** Measurements taken inside the page. Kept free of closures so it can be serialised. */
function inspect({
  expectsGlass,
  minimumTarget,
}: {
  expectsGlass: boolean;
  minimumTarget: number;
}): Finding[] {
  const findings: Finding[] = [];
  const add = (check: string, detail: string): void => {
    findings.push({ check, detail });
  };
  const width = window.innerWidth;
  const height = window.innerHeight;
  const rounded = (rect: DOMRect): string =>
    [rect.left, rect.top, rect.right, rect.bottom].map(Math.round).join(",");
  if (document.documentElement.scrollWidth > width + 1)
    add("overflow", `page is ${document.documentElement.scrollWidth}px wide`);

  const frame = [...document.querySelectorAll<HTMLElement>("[data-frame]")];
  const mode =
    document.querySelector<HTMLElement>("[data-frame-mode]")?.dataset
      .frameMode ?? "browse";
  const names = frame.map((part) => part.dataset.frame);
  if (mode === "browse" && !names.includes("header"))
    add("frame", "header is missing");
  // A focused task brings its task header. Account entry is the one focused screen that is only a card.
  if (
    mode === "focus" &&
    !names.includes("task-header") &&
    !document.querySelector("[data-entry-card]")
  )
    add("frame", "task header is missing");
  if (mode !== "browse" && names.includes("dock"))
    add("frame", `${mode} frame still shows the dock`);
  // Surfaces that must be glass. Floating regions only position their own glass controls.
  const glassParts = new Set([
    "header",
    "rail",
    "strip",
    "dock",
    "task-header",
    "pane-switch",
  ]);
  for (const part of frame) {
    const name = part.dataset.frame ?? "?";
    const rect = part.getBoundingClientRect();
    if (rect.left < -1 || rect.right > width + 1 || rect.bottom > height + 1)
      add("bounds", `${name} leaves the viewport at ${rounded(rect)}`);
    const style = getComputedStyle(part);
    const filter =
      style.backdropFilter !== "none"
        ? style.backdropFilter
        : style.getPropertyValue("-webkit-backdrop-filter") || "none";
    const alpha = (() => {
      const match = /rgba?\(([^)]+)\)|color\(srgb ([^)]+)\)/.exec(
        style.backgroundColor,
      );
      const parts = (match?.[1] ?? match?.[2] ?? "")
        .split(/[\s,/]+/)
        .filter(Boolean);
      return parts.length > 3 ? Number(parts[3]) : 1;
    })();
    if (!glassParts.has(name)) continue;
    if (expectsGlass && (filter === "none" || alpha > 0.7))
      add("glass", `${name} is not see-through (alpha ${alpha}, ${filter})`);
    if (!expectsGlass && (filter !== "none" || alpha < 0.99))
      add("solid", `${name} stayed see-through (alpha ${alpha}, ${filter})`);
  }
  const overlaps = (a: DOMRect, b: DOMRect): boolean =>
    a.left < b.right - 1 &&
    b.left < a.right - 1 &&
    a.top < b.bottom - 1 &&
    b.top < a.bottom - 1;
  for (const [index, first] of frame.entries())
    for (const second of frame.slice(index + 1))
      if (
        overlaps(first.getBoundingClientRect(), second.getBoundingClientRect())
      )
        add(
          "overlap",
          `${first.dataset.frame} overlaps ${second.dataset.frame}`,
        );

  // A dialog must fit the screen it opens on, with room to see it is a dialog.
  // A dialog is a native dialog on wide screens and a drawer on a phone.
  const dialogs = [
    ...document.querySelectorAll<HTMLElement>(
      "dialog[open], [data-vaul-drawer][data-state=open]",
    ),
  ];
  for (const dialog of dialogs) {
    // A drawer with another open over it is pushed back on purpose.
    if (dialog !== dialogs.at(-1) && dialog.matches("[data-vaul-drawer]"))
      continue;
    const rect = dialog.getBoundingClientRect();
    if (
      rect.left < -1 ||
      rect.top < -1 ||
      rect.right > width + 1 ||
      rect.bottom > height + 1
    )
      add("dialog", `a dialog leaves the viewport at ${rounded(rect)}`);
  }
  // Only the dialog on top can be used, so only its controls are measured.
  const top = dialogs.at(-1);
  const controls = top
    ? top.querySelectorAll<HTMLElement>("a, button, input")
    : document.querySelectorAll<HTMLElement>(
        "[data-frame] a, [data-frame] button, main a, main button, main input",
      );
  for (const control of controls) {
    const rect = control.getBoundingClientRect();
    if (rect.width === 0 || rect.height === 0) continue;
    if (rect.bottom < 0 || rect.top > height) continue;
    // A control scrolled out of its own list is reached by scrolling, not measured here.
    const hiddenByScroller = (() => {
      for (
        let parent = control.parentElement;
        parent && parent !== document.body;
        parent = parent.parentElement
      ) {
        if (!/auto|scroll/.test(getComputedStyle(parent).overflowY)) continue;
        const box = parent.getBoundingClientRect();
        if (rect.bottom <= box.top + 1 || rect.top >= box.bottom - 1)
          return true;
      }
      return false;
    })();
    if (hiddenByScroller) continue;
    // A search input may be shorter than its framed field, which is the real target.
    const target =
      control.tagName === "INPUT"
        ? (control.parentElement?.getBoundingClientRect() ?? rect)
        : rect;
    const label =
      control.getAttribute("aria-label") ??
      control.textContent?.trim().slice(0, 24) ??
      control.tagName;
    if (
      target.width < minimumTarget - 0.5 ||
      target.height < minimumTarget - 0.5
    )
      add(
        "target",
        `${label} is ${Math.round(target.width)}x${Math.round(target.height)}, needs ${minimumTarget}`,
      );
    // Content in a sideways scroller, such as a tab strip, is expected to continue past the edge.
    const scrollsSideways = (() => {
      for (
        let parent = control.parentElement;
        parent && parent !== document.body;
        parent = parent.parentElement
      )
        if (/auto|scroll/.test(getComputedStyle(parent).overflowX)) return true;
      return false;
    })();
    if (
      !control.closest("[role=region]") &&
      !scrollsSideways &&
      (rect.left < -1 || rect.right > width + 1)
    )
      add("clipped", `${label} leaves the viewport at ${rounded(rect)}`);
    // Scrolling content may pass under the dock. Anything that cannot scroll away must stay clear of it.
    const dock = document.querySelector("[data-frame=dock]");
    if (
      dock &&
      // A dialog opens over the dock, which is then out of use beneath it.
      !top &&
      !dock.contains(control) &&
      !control.closest("[role=region]") &&
      overlaps(rect, dock.getBoundingClientRect())
    )
      add("covered", `${label} sits under the dock`);
  }
  /** The part of a control a scrolling ancestor leaves in view. */
  const visibleRect = (control: HTMLElement): DOMRect => {
    let { left, top, right, bottom } = control.getBoundingClientRect();
    for (
      let parent = control.parentElement;
      parent && parent !== document.body;
      parent = parent.parentElement
    ) {
      const style = getComputedStyle(parent);
      if (!/auto|scroll|hidden|clip/.test(style.overflowY + style.overflowX))
        continue;
      const box = parent.getBoundingClientRect();
      left = Math.max(left, box.left);
      top = Math.max(top, box.top);
      right = Math.min(right, box.right);
      bottom = Math.min(bottom, box.bottom);
    }
    return new DOMRect(
      left,
      top,
      Math.max(0, right - left),
      Math.max(0, bottom - top),
    );
  };
  /** A control scrolled wholly out of view has nothing left to crowd. */
  const isShown = (rect: DOMRect): boolean =>
    rect.width > 0.5 && rect.height > 0.5;
  // Fixed controls squeezed into each other read as one broken control.
  const fixed = [...controls].filter((control) => {
    const rect = control.getBoundingClientRect();
    return (
      rect.width > 0 &&
      // A control hidden under an overlay, or inert behind it, is not on screen to crowd.
      getComputedStyle(control).visibility !== "hidden" &&
      !control.closest("[inert]") &&
      control.tagName !== "INPUT" &&
      !control.closest("[role=region]") &&
      !control.closest("[data-frame=dock]") &&
      // Cards on a table overlap by design: a fanned hand, a squeezed row, a pile.
      !control.closest("[data-match-table]")
    );
  });
  for (const [index, first] of fixed.entries())
    for (const second of fixed.slice(index + 1))
      if (
        !first.contains(second) &&
        !second.contains(first) &&
        isShown(visibleRect(first)) &&
        isShown(visibleRect(second)) &&
        overlaps(visibleRect(first), visibleRect(second))
      )
        add(
          "crowded",
          `${first.getAttribute("aria-label") ?? first.textContent?.trim()} overlaps ${second.getAttribute("aria-label") ?? second.textContent?.trim()}`,
        );
  return findings;
}

async function run(browser: Browser, test: Case): Promise<Finding[]> {
  const { viewport, art, decks, state } = test;
  const context = await browser.newContext({
    viewport: { width: viewport.width, height: viewport.height },
    deviceScaleFactor: viewport.touch && viewport.width < 600 ? 2 : 1,
    hasTouch: viewport.touch,
    isMobile: viewport.touch,
    // Ambient and arrival motion is checked by eye; stills here must be repeatable.
    reducedMotion: "reduce",
    ...(state === "forced-colors" ? { forcedColors: "active" as const } : {}),
  });
  const page: Page = await context.newPage();
  const errors: string[] = [];
  page.on("pageerror", (error) => errors.push(String(error)));
  page.on("console", (message) => {
    // Headless WebGL may refuse the deck previews. That is outside the frame's contract.
    if (
      message.type() === "error" &&
      !/collection box|WebGL|Failed to load resource/i.test(message.text())
    )
      errors.push(message.text());
  });
  // Software-rendered deck previews take minutes per case and are not part of the frame.
  await page.route("**/models/*.glb*", (route) => route.abort());
  if (art !== "none") {
    const file = await readFile(art === "dark" ? darkArt! : brightArt!);
    await page.route("**/src/lib/backdrop-art.ts*", (route) =>
      route.fulfill({
        contentType: "text/javascript",
        body: `export const backdropArt = { url: "${basePath}/__frame-art__", focus: { x: 0.5, y: 0.3 } };`,
      }),
    );
    await page.route("**/__frame-art__", (route) =>
      route.fulfill({ contentType: "image/png", body: file }),
    );
  }
  // The preview person, with a picture and badge, so both are drawn on every surface.
  await page.addInitScript((root: string) => {
    window.localStorage.setItem(
      "spellwake:account:demo:v1",
      JSON.stringify({
        art: {
          id: "Goblin Guide",
          name: "Goblin Guide",
          artist: "Filip Burburan",
          artUrl: `${root}/builder-preview/cards/goblin-guide-art.jpg`,
        },
        crop: { x: 0.5, y: 0.5, scale: 1 },
        badge: "supporter",
        patreon: "active",
      }),
    );
  }, basePath);
  const saved = library(decks);
  if (saved !== null)
    await page.addInitScript((value: string) => {
      window.localStorage.setItem("spellwake:decks:demo:v1:collection", value);
    }, saved);
  if (state === "reduced-transparency") {
    const session = await context.newCDPSession(page);
    await session.send("Emulation.setEmulatedMedia", {
      features: [{ name: "prefers-reduced-transparency", value: "reduce" }],
    });
  }
  await page.goto(base + test.path, { waitUntil: "load" });
  await page.waitForSelector("[data-frame], [data-entry-card]", {
    timeout: 60_000,
  });
  if (art !== "none")
    // Forced colours hide the artwork on purpose, so it only has to be loaded.
    await page.waitForSelector("img[data-ready=true]", {
      state: "attached",
      timeout: 30_000,
    });
  await page.waitForTimeout(600);

  const findings: Finding[] = [];
  if (state === "menu") {
    await page.getByRole("combobox", { name: "Format" }).click();
    await page.waitForTimeout(400);
    const aligned = await page.evaluate(() => {
      const trigger = document
        .querySelector("[aria-label=Format]")
        ?.getBoundingClientRect();
      const menu = document
        .querySelector("[role=listbox]")
        ?.getBoundingClientRect();
      return trigger && menu
        ? Math.abs(trigger.left - menu.left) <= 2 &&
            menu.right <= window.innerWidth + 1 &&
            menu.bottom <= window.innerHeight + 1
        : false;
    });
    if (!aligned)
      findings.push({ check: "menu", detail: "format menu is misplaced" });
  }
  if (state === "picture" || state === "crop") {
    await page.getByRole("button", { name: "Change picture" }).click();
    await page
      .getByRole("searchbox", { name: "Search artwork by card name" })
      .fill(state === "crop" ? "goblin" : "");
    await page.getByRole("button", { name: "Goblin Guide" }).waitFor();
    if (state === "crop") {
      await page.getByRole("button", { name: "Goblin Guide" }).click();
      await page.getByRole("slider", { name: "Zoom" }).waitFor();
      const covered = await page.evaluate(() => {
        const stage = document.querySelector<HTMLElement>(
          "[data-picture-stage=crop] [role=group]",
        );
        const art = stage?.querySelector("img")?.getBoundingClientRect();
        if (!stage || !art) return "the crop stage is missing";
        const left = stage.getBoundingClientRect().left + stage.clientLeft;
        const top = stage.getBoundingClientRect().top + stage.clientTop;
        if (stage.clientWidth < 160)
          return "the crop stage is too small to use";
        return art.left <= left + 1 &&
          art.top <= top + 1 &&
          art.right >= left + stage.clientWidth - 1 &&
          art.bottom >= top + stage.clientHeight - 1
          ? null
          : "the artwork does not cover the crop";
      });
      if (covered) findings.push({ check: "crop", detail: covered });
    }
    // A drawer takes about half a second to finish rising.
    await page.waitForTimeout(800);
  }
  if (state === "badge") {
    await page.getByRole("button", { name: "Change badge" }).click();
    await page.getByRole("radio", { name: "Supporter" }).waitFor();
    await page.waitForTimeout(800);
  }
  if (state === "delete") {
    await page.getByRole("button", { name: "Delete account" }).click();
    await page.getByRole("heading", { name: "Delete your account?" }).waitFor();
    await page.waitForTimeout(800);
  }
  if (state === "import-file") {
    await page.getByRole("tab", { name: "File" }).click();
    await page.getByText("Drop a deck file here").waitFor();
    await page.waitForTimeout(400);
  }
  if (state === "appearance") {
    // A narrow builder carries Appearance in its actions menu instead.
    const button = page.getByRole("button", { name: "Appearance" });
    if (await button.isVisible()) await button.click();
    else {
      await page.getByRole("button", { name: "Deck actions" }).click();
      await page.getByRole("menuitem", { name: "Appearance" }).click();
    }
    await page.getByRole("heading", { name: "Deck appearance" }).waitFor();
    await page.waitForTimeout(800);
  }
  if (state === "lobby-deck") {
    await page.getByRole("button", { name: /^Your deck: / }).click();
    await page.getByRole("heading", { name: "Choose Deck" }).waitFor();
    await page.waitForTimeout(800);
  }
  if (state === "lobby-rules") {
    await page.getByRole("button", { name: "Edit rules" }).click();
    await page.getByRole("heading", { name: "Game rules" }).waitFor();
    await page.waitForTimeout(800);
  }
  if (state === "lobby-invite") {
    await page.getByRole("button", { name: "Invite players" }).click();
    await page.getByRole("heading", { name: "Invite players" }).waitFor();
    await page.waitForTimeout(1000);
  }
  if (state === "trade-picker") {
    await page.getByRole("button", { name: "Add items" }).click();
    await page.getByRole("heading", { name: "Add items" }).waitFor();
    await page.waitForTimeout(800);
  }
  if (state === "trade-cancel") {
    await page.getByRole("button", { name: "Cancel trade" }).click();
    await page.getByRole("heading", { name: "Cancel trade?" }).waitFor();
    await page.waitForTimeout(800);
  }
  if (state === "trade-edit") {
    await page.getByRole("button", { name: "Edit items" }).click();
    await page.getByRole("heading", { name: "Edit items?" }).waitFor();
    await page.waitForTimeout(800);
  }
  if (state === "supply-buy" || state === "supply-buy-three") {
    await page.getByRole("button", { name: /^Buy / }).first().click();
    await page.getByRole("heading", { name: "Buy packs" }).waitFor();
    if (state === "supply-buy-three")
      await page.getByRole("textbox", { name: "Quantity" }).fill("3");
    await page.waitForTimeout(1000);
  }
  if (state === "supply-vote") {
    await page
      .getByRole("button", {
        name: /Vote for next drop|Edit vote|See your vote|See results/,
      })
      .click();
    await page
      .getByRole("heading", { name: /Supply (vote|results)/ })
      .waitFor();
    await page.waitForTimeout(900);
  }
  if (state === "match-attack") {
    await page
      .getByRole("button", { name: "Goblin Guide", exact: true })
      .click();
    await page.getByRole("button", { name: "View Alex’s board" }).click();
    // The assignment must have been made: Attack is offered only then.
    await page
      .getByRole("button", { name: "Attack", exact: true })
      .and(page.locator(":enabled"))
      .waitFor();
    await page.waitForTimeout(1100);
  }
  if (state === "match-block") {
    await page.getByRole("button", { name: "View the whole table" }).click();
    await page.locator('[data-match-table="overview"]').waitFor();
    await page.waitForTimeout(900);
    const blocker = page.getByRole("button", {
      name: "Mogg War Marshal",
      exact: true,
    });
    // On a small screen the whole table is a map, and blocks are made from a board view.
    if (await blocker.isVisible()) {
      await blocker.click();
      await page
        .getByRole("button", { name: /Alex’s Krenko, Mob Boss/ })
        .click();
      // The link must have been made: Block is offered only then.
      await page
        .getByRole("button", { name: "Block", exact: true })
        .and(page.locator(":enabled"))
        .waitFor();
    }
    await page.waitForTimeout(1100);
  }
  if (
    state === "match-damage" ||
    state === "match-damage-two" ||
    state === "match-trample"
  ) {
    const confirm = page
      .getByRole("button", { name: "Confirm", exact: true })
      .and(page.locator(":enabled"));
    // The suggested split is complete, so Confirm is offered at once.
    await confirm.waitFor();
    // The targets are on Alex's board (Morgan's for the second source).
    if (state === "match-damage-two") {
      await page
        .getByRole("button", { name: /^Goblin Guide, 2 of 2 assigned$/ })
        .click();
      await page.getByRole("button", { name: "View Morgan’s board" }).click();
    } else {
      await page.getByRole("button", { name: "View Alex’s board" }).click();
    }
    await page.locator('[data-match-table="seat"]').waitFor();
    await page.waitForTimeout(900);
    const amount = (name: string) =>
      page
        .getByRole("group", { name: `Damage to ${name}`, exact: true })
        .getByRole("status");
    if (state === "match-damage") {
      await page
        .getByRole("button", { name: "More damage to Goblin Matron" })
        .click();
      await expectText(amount("Krenko, Mob Boss"), "3");
      await expectText(amount("Goblin Matron"), "1");
      // Taking a point away leaves it unassigned, and Confirm waits.
      await page
        .getByRole("button", { name: "Less damage to Goblin Matron" })
        .click();
      await page
        .getByRole("button", { name: "Confirm", exact: true })
        .and(page.locator(":disabled"))
        .waitFor();
      await page
        .getByRole("button", { name: "More damage to Goblin Matron" })
        .click();
      await confirm.click();
      // Accepted: the controls leave the table.
      await page
        .getByRole("group", { name: /^Damage to / })
        .first()
        .waitFor({ state: "detached" });
    }
    if (state === "match-damage-two") {
      await page
        .getByRole("button", { name: "More damage to Legion Loyalist" })
        .click();
      await expectText(amount("Goblin Piledriver"), "1");
      await expectText(amount("Legion Loyalist"), "1");
      await confirm.waitFor();
    }
    if (state === "match-trample") {
      await expectText(amount("Goblin Matron"), "1");
      await expectText(amount("Alex"), "5");
      // Lethal damage stays on the blocker: nothing more can go to the player.
      await page
        .getByRole("button", { name: "More damage to Alex" })
        .and(page.locator(":disabled"))
        .waitFor();
      await page
        .getByRole("button", { name: "More damage to Goblin Matron" })
        .click();
      await expectText(amount("Alex"), "4");
    }
    await page.waitForTimeout(700);
  }
  if (state === "match-chat-send" || state === "match-chat-refused") {
    const chat = page.getByRole("region", { name: "Table chat" });
    // Something unread waits, and opening the chat at its end clears it.
    await page
      .getByRole("button", { name: "Table chat, unread messages" })
      .click();
    await chat.waitFor();
    const field = chat.getByRole("textbox", { name: "Message the table" });
    await field.fill("Good game, everyone.");
    await field.press("Enter");
    if (state === "match-chat-send") {
      // Accepted: the message joins the history and the draft is cleared.
      await chat.getByText("Good game, everyone.").waitFor();
      await expect(field).toHaveValue("");
      await chat.getByRole("button", { name: "Close table chat" }).click();
      await chat.waitFor({ state: "detached" });
      await page
        .getByRole("button", { name: "Table chat", exact: true })
        .click();
      await chat.waitFor();
    } else {
      // Refused: the words stay, with one short line.
      await chat.getByText("Couldn’t send message.").waitFor();
      await expect(field).toHaveValue("Good game, everyone.");
      // Closed and reopened, the draft is still there.
      await field.press("Escape");
      await chat.waitFor({ state: "detached" });
      await page.getByRole("button", { name: /^Table chat/ }).click();
      await expect(
        chat.getByRole("textbox", { name: "Message the table" }),
      ).toHaveValue("Good game, everyone.");
    }
    await page.waitForTimeout(500);
  }
  if (state === "match-settings" || state === "match-concede") {
    await page.getByRole("button", { name: "Settings", exact: true }).click();
    const settings = page.getByRole("dialog", { name: "Settings" });
    await settings.waitFor();
    const music = settings.getByRole("slider", { name: "Music" });
    await music.focus();
    await music.press("Home");
    await expect(music).toHaveValue("0");
    const tap = settings.getByRole("switch", { name: "Auto-tap mana" });
    const was = await tap.isChecked();
    await tap.setChecked(!was);
    await expect(tap).toBeChecked({ checked: !was });
    if (state === "match-settings") {
      // Closed and reopened, the changes are still there: there is no Save.
      await page.keyboard.press("Escape");
      await settings.waitFor({ state: "hidden" });
      await page.getByRole("button", { name: "Settings", exact: true }).click();
      await expect(settings.getByRole("slider", { name: "Music" })).toHaveValue(
        "0",
      );
    } else {
      await settings
        .getByRole("button", { name: "Concede", exact: true })
        .click();
      const confirm = page.getByRole("dialog", { name: "Concede?" });
      await confirm.waitFor();
      // Cancel goes back to the settings it came from.
      await confirm.getByRole("button", { name: "Cancel" }).click();
      await settings
        .getByRole("button", { name: "Concede", exact: true })
        .click();
      await confirm
        .getByRole("button", { name: "Concede", exact: true })
        .click();
      // Accepted: still at the table, watching, with nothing to act on.
      await page.getByText("Watching", { exact: true }).waitFor();
      await expect(
        page.getByRole("button", { name: "Next", exact: true }),
      ).toHaveCount(0);
      await page.getByRole("button", { name: "Settings", exact: true }).click();
      await settings.waitFor();
      await expect(
        settings.getByRole("button", { name: "Concede" }),
      ).toHaveCount(0);
    }
    await page.waitForTimeout(600);
  }
  if (state === "match-watch-chat") {
    await page.getByRole("button", { name: /^Table chat/ }).click();
    const chat = page.getByRole("region", { name: "Table chat" });
    await chat.getByText("Chat is read-only.").waitFor();
    await expect(chat.getByRole("textbox")).toHaveCount(0);
    await page.waitForTimeout(500);
  }
  if (
    state === "match-series" ||
    state === "match-series-next" ||
    state === "match-series-leave"
  ) {
    const query = new URLSearchParams(test.path.split("?")[1]);
    const kind = query.get("series") ?? "";
    const band = page.getByRole("dialog", {
      name: query.get("end") === "won" ? "You won" : "You lost",
      exact: true,
    });
    await band.waitFor();
    const nextGame = band.getByRole("button", { name: "Next game" });
    const leave = band.getByRole("button", { name: "Leave table" });
    if (kind === "over") {
      // The match is decided: its score, and the way out.
      await band.getByText("Match over · 1–2").waitFor();
      await leave.waitFor();
      await expect(nextGame).toHaveCount(0);
    } else {
      await band
        .getByText(
          query.get("end") === "won"
            ? "Game 1 · You lead 1–0"
            : "Game 1 · You trail 0–1",
        )
        .waitFor();
      // Between games the band never offers to leave the table.
      await expect(leave).toHaveCount(0);
      if (kind === "waiting") {
        await band.getByText("Waiting for the next game…").waitFor();
        await expect(nextGame).toHaveCount(0);
      } else {
        await nextGame.waitFor();
        await band
          .getByText(
            kind === "untimed"
              ? "The next game starts when everyone is ready."
              : /Next game in \d:\d\d/,
          )
          .waitFor();
        if (kind === "stuck")
          await band.getByText(/That didn’t go through\./).waitFor();
      }
    }
    if (state === "match-series-next") {
      // Lifted, the same answer is offered where the turn's actions were.
      await band.getByRole("button", { name: "View table" }).click();
      await band.waitFor({ state: "detached" });
      const kept = page.getByRole("group", { name: "Game result" });
      await kept.getByText(/Next game in \d:\d\d/).waitFor();
      await kept.getByRole("button", { name: "Next game" }).click();
      await kept.getByText("Waiting for the next game…").waitFor();
      await expect(kept.getByRole("button")).toHaveCount(0);
    }
    if (state === "match-series-leave") {
      await page.getByRole("button", { name: "Leave table" }).click();
      const confirm = page.getByRole("dialog", { name: "Leave match?" });
      await confirm.waitFor();
      await confirm.getByRole("button", { name: "Stay at table" }).click();
      await confirm.waitFor({ state: "hidden" });
      await band.waitFor();
    }
    await page.waitForTimeout(1400);
  }
  if (
    state === "match-end" ||
    state === "match-end-view" ||
    state === "match-end-chat"
  ) {
    const title =
      { won: "You won", lost: "You lost", draw: "Draw", none: "No result" }[
        new URLSearchParams(test.path.split("?")[1]).get("end") ?? ""
      ] ?? "";
    const band = page.getByRole("dialog", { name: title, exact: true });
    await band.waitFor();
    // Nothing more is offered at a finished table, and no turn is anyone's.
    await expect(page.getByRole("button", { name: "Next" })).toHaveCount(0);
    await expect(page.getByText(/turn · T\d/)).toHaveCount(0);
    if (state === "match-end-view") {
      await band.getByRole("button", { name: "View table" }).click();
      await band.waitFor({ state: "detached" });
      const kept = page.getByRole("group", { name: "Game result" });
      await kept.getByText(title, { exact: true }).waitFor();
      await kept.getByRole("button", { name: "Leave table" }).waitFor();
      // The game is over: settings no longer offer to concede it.
      await page.getByRole("button", { name: "Settings", exact: true }).click();
      const settings = page.getByRole("dialog", { name: "Settings" });
      await settings.waitFor();
      await expect(
        settings.getByRole("button", { name: "Concede", exact: true }),
      ).toHaveCount(0);
      await page.keyboard.press("Escape");
      await settings.waitFor({ state: "hidden" });
    }
    if (state === "match-end-chat") {
      // Opening the chat lifts the band; the result waits in the turn panel.
      await page.getByRole("button", { name: /^Table chat/ }).click();
      await band.waitFor({ state: "detached" });
      await page
        .getByRole("group", { name: "Game result" })
        .getByRole("button", { name: "Leave table" })
        .waitFor();
      const chat = page.getByRole("region", { name: "Table chat" });
      const box = chat.getByRole("textbox");
      await box.fill("Good game");
      await box.press("Enter");
      await chat.getByText("Good game").waitFor();
    }
    // The band and what it holds have arrived.
    await page.waitForTimeout(1400);
  }
  if (
    state === "match-cast-target" ||
    state === "match-cast-player" ||
    state === "match-cast-pay" ||
    state === "match-cast-paid" ||
    state === "match-cast-creature"
  ) {
    const cast = (name: string) => pressHandCard(page, name);
    const prompt = page.locator('[data-cast-prompt="target"]');
    const pile = (count: number) =>
      page.locator(`[data-match-stack="${count}"]`);
    const resolveAction = page.getByRole("button", {
      name: "Resolve",
      exact: true,
    });
    if (state === "match-cast-creature") {
      // No target and no pause: the cast is sent at once.
      await cast("Goblin Piledriver");
      await pile(1).waitFor();
      await expect(prompt).toHaveCount(0);
      await resolveAction.click();
      await page.locator("[data-match-stack]").waitFor({ state: "detached" });
      // Resolved, it is a permanent on your board.
      await page
        .getByRole("button", { name: "Goblin Piledriver", exact: true })
        .or(page.getByRole("img", { name: "Goblin Piledriver", exact: true }))
        .waitFor();
    } else {
      const paying = state === "match-cast-pay" || state === "match-cast-paid";
      await cast(paying ? "Shock" : "Lightning Bolt");
      await prompt.waitFor();
      // The pending spell stands beside the table; it is not on the stack.
      await pile(1).waitFor();
      await expect(resolveAction).toHaveCount(0);
      if (state === "match-cast-target") {
        // Cancel puts everything back, and the card may be started again.
        await page.getByRole("button", { name: "Cancel", exact: true }).click();
        await prompt.waitFor({ state: "detached" });
        await page.getByRole("button", { name: "Next", exact: true }).waitFor();
        await cast("Lightning Bolt");
        await prompt.waitFor();
      }
      if (state === "match-cast-player") {
        // The portrait chooses the player, and the last choice sends the cast.
        await page.getByRole("button", { name: "Target Alex" }).click();
        await prompt.waitFor({ state: "detached" });
        await resolveAction.waitFor();
        await expect(
          page.getByRole("button", {
            name: "Lightning Bolt, in your hand",
            exact: true,
          }),
        ).toHaveCount(0);
      }
      if (paying) {
        // One of your own creatures is the target; then the game waits for payment.
        await page
          .getByRole("button", { name: "Goblin Guide", exact: true })
          .click();
        const pay = page.locator('[data-cast-prompt="payment"]');
        await pay.waitFor();
        await expect(resolveAction).toHaveCount(0);
        const autoPay = page.getByRole("button", {
          name: "Auto Pay",
          exact: true,
        });
        await autoPay.and(page.locator(":enabled")).waitFor();
        if (state === "match-cast-paid") {
          await autoPay.click();
          await pay.waitFor({ state: "detached" });
          await resolveAction.waitFor();
        }
      }
    }
    await page.waitForTimeout(700);
  }
  if (
    state === "match-mode-choice" ||
    state === "match-mode-unavailable" ||
    state === "match-mode-peek" ||
    state === "match-mode-cast"
  ) {
    await pressHandCard(page, "Abrade");
    const choice = page.getByRole("dialog", { name: "Choose one" });
    await choice.waitFor();
    const damage = choice.getByRole("button", {
      name: "Abrade deals 3 damage to target creature.",
    });
    const destroy = choice.getByRole("button", {
      name: "Destroy target artifact.",
    });
    await damage.and(page.locator(":enabled")).waitFor();
    const next = page.getByRole("button", { name: "Next", exact: true });
    if (state === "match-mode-choice")
      await destroy.and(page.locator(":enabled")).waitFor();
    if (state === "match-mode-unavailable") {
      // With no artifact on the table, that mode says so and cannot be chosen.
      await destroy.and(page.locator(":disabled")).waitFor();
      await choice.getByText("No legal targets").waitFor();
    }
    if (state === "match-mode-peek") {
      await choice.getByRole("button", { name: "View Battlefield" }).click();
      await choice.waitFor({ state: "hidden" });
      // Looking is not acting: there is no turn action while the choice waits.
      await expect(next).toHaveCount(0);
      await page.getByRole("button", { name: "Return to choice" }).click();
      await damage.waitFor();
      await choice.getByRole("button", { name: "View Battlefield" }).click();
      await page.getByRole("button", { name: "Cancel", exact: true }).click();
      await next.waitFor();
      await pressHandCard(page, "Abrade");
      await choice.getByRole("button", { name: "View Battlefield" }).click();
      await page.getByRole("button", { name: "Return to choice" }).waitFor();
    }
    if (state === "match-mode-cast") {
      // The mode goes straight on to its own targets: artifacts only.
      await destroy.click();
      await choice.waitFor({ state: "hidden" });
      await page.locator('[data-cast-prompt="target"]').waitFor();
      await expect(page.getByRole("button", { name: /^Target / })).toHaveCount(
        0,
      );
      await page
        .getByRole("button", { name: "Aether Vial", exact: true })
        .click();
      await page.getByRole("button", { name: "Resolve", exact: true }).click();
      await page.locator("[data-match-stack]").waitFor({ state: "detached" });
      // Destroyed: your Aether Vial has left the table.
      await expect(
        page.getByRole("button", { name: "Aether Vial", exact: true }),
      ).toHaveCount(0);
    }
    await page.waitForTimeout(700);
  }
  if (
    state === "match-two-modes" ||
    state === "match-two-modes-quiet" ||
    state === "match-two-modes-cast" ||
    state === "match-mixed-targets" ||
    state === "match-mixed-cast"
  ) {
    await pressHandCard(page, "Cryptic Command");
    const choice = page.getByRole("dialog", { name: "Choose two" });
    await choice.waitFor();
    const mode = (name: string) => choice.getByRole("button", { name });
    const counter = mode("Counter target spell.");
    const giveBack = mode("Return target permanent to its owner’s hand.");
    const tap = mode("Tap all creatures your opponents control.");
    const draw = mode("Draw a card.");
    const next = choice.getByRole("button", { name: "Next", exact: true });
    const prompt = page.locator('[data-cast-prompt="target"]');
    const face = page.locator('[data-cast-source="Cryptic Command"]');
    const resolveAction = page.getByRole("button", {
      name: "Resolve",
      exact: true,
    });
    await expect(next).toBeDisabled();
    const mixed =
      state === "match-mixed-targets" || state === "match-mixed-cast";
    if (!mixed) {
      // Nothing is on the stack, so there is no spell to counter.
      await expect(counter).toBeDisabled();
      await choice.getByText("No legal targets").waitFor();
    }
    if (state === "match-two-modes") {
      await giveBack.click();
      await expect(giveBack).toHaveAttribute("aria-pressed", "true");
      await expect(next).toBeDisabled();
      await draw.click();
      await expect(next).toBeEnabled();
      // With two marked, a third cannot be added until one is removed.
      await expect(tap).toBeDisabled();
      await draw.click();
      await expect(draw).toHaveAttribute("aria-pressed", "false");
      await expect(next).toBeDisabled();
      await tap.click();
      await expect(next).toBeEnabled();
    }
    if (state === "match-two-modes-quiet") {
      // Neither mode has a target: Next casts, with no other confirmation.
      await tap.click();
      await draw.click();
      await next.click();
      await choice.waitFor({ state: "hidden" });
      await expect(prompt).toHaveCount(0);
      await resolveAction.waitFor();
    }
    if (state === "match-two-modes-cast") {
      await giveBack.click();
      await draw.click();
      await next.click();
      await choice.waitFor({ state: "hidden" });
      await expectText(prompt, "Choose a permanent");
      // The spell stands on its own face with both chosen modes outlined.
      await face.waitFor();
      await expect(face.locator('[data-tone="selected"]')).toHaveCount(2);
      await expect(page.getByRole("button", { name: "Back" })).toHaveCount(0);
      await page
        .getByRole("button", { name: "Aether Vial", exact: true })
        .click();
      await resolveAction.click();
      await page.locator("[data-match-stack]").waitFor({ state: "detached" });
      // Returned: the Vial is back in your hand.
      await page
        .getByRole("button", { name: "Aether Vial, in your hand", exact: true })
        .waitFor();
    }
    if (mixed) {
      await counter.click();
      await giveBack.click();
      await next.click();
      await choice.waitFor({ state: "hidden" });
      // The spell is asked for first, and chosen on the pile itself.
      await expectText(prompt, "Choose a spell · 1/2");
      const offered = page.locator("[data-match-stack] [data-offered]");
      await expect(offered).toHaveCount(2);
      await expect(page.getByRole("button", { name: "Back" })).toHaveCount(0);
      await page.getByRole("option", { name: /^Shock, Morgan/ }).click();
      await expectText(prompt, "Choose a permanent · 2/2");
      await expect(offered).toHaveCount(0);
      await page.locator("[data-cast-link]").waitFor();
      await expect(face.locator('[data-tone="earlier"]')).toHaveCount(1);
      await expect(face.locator('[data-tone="available"]')).toHaveCount(1);
      if (state === "match-mixed-cast") {
        // Back keeps the old spell until another replaces it.
        await page.getByRole("button", { name: "Back", exact: true }).click();
        await expectText(prompt, "Choose a spell · 1/2");
        await expect(page.locator("[data-cast-link]")).toHaveCount(1);
        // The older spell is pressed on the strip of it that shows.
        const older = page.getByRole("option", {
          name: /^Lightning Bolt, Alex/,
        });
        const strip = await older.boundingBox();
        if (!strip) throw new Error("The older spell is not shown.");
        await older.click({
          position: { x: strip.width * 0.12, y: strip.height * 0.5 },
        });
        await expectText(prompt, "Choose a permanent · 2/2");
        await page
          .getByRole("button", { name: "Aether Vial", exact: true })
          .click();
        // Accepted, it is the newest of three; resolved, it takes the Bolt with it.
        await page.locator('[data-match-stack="3"]').waitFor();
        await resolveAction.click();
        await page.locator('[data-match-stack="1"]').waitFor();
        await page.getByRole("option", { name: /^Shock, Morgan/ }).waitFor();
      }
    }
    await page.waitForTimeout(700);
  }
  if (state === "match-two-targets" || state === "match-two-targets-back") {
    await pressHandCard(page, "Common Bond");
    const prompt = page.locator('[data-cast-prompt="target"]');
    const face = page.locator('[data-cast-source="Common Bond"]');
    const back = page.getByRole("button", { name: "Back", exact: true });
    const creature = (name: string) =>
      page.getByRole("button", { name, exact: true });
    await expectText(prompt, "Choose a creature · 1/2");
    await face.waitFor();
    await expect(face.locator('[data-tone="available"]')).toHaveCount(1);
    // The first has nothing before it: Cancel is the way out.
    await expect(back).toHaveCount(0);
    await creature("Goblin Guide").click();
    await expectText(prompt, "Choose a creature · 2/2");
    await page.locator("[data-cast-link]").waitFor();
    await expect(face.locator('[data-tone="earlier"]')).toHaveCount(1);
    await back.waitFor();
    if (state === "match-two-targets-back") {
      await back.click();
      await expectText(prompt, "Choose a creature · 1/2");
      // The old choice keeps its line until another replaces it.
      await expect(page.locator("[data-cast-link]")).toHaveCount(1);
      await expect(back).toHaveCount(0);
      await creature("Goblin Chieftain").click();
      await expectText(prompt, "Choose a creature · 2/2");
      // The same creature may be named twice; the second choice casts.
      await creature("Goblin Chieftain").click();
      await prompt.waitFor({ state: "detached" });
      await page.getByRole("button", { name: "Resolve", exact: true }).click();
      await page.locator("[data-match-stack]").waitFor({ state: "detached" });
    }
    await page.waitForTimeout(700);
  }
  if (
    state === "match-divide" ||
    state === "match-divide-adjust" ||
    state === "match-divide-cast"
  ) {
    await pressHandCard(page, "Arc Lightning");
    const prompt = page.locator('[data-cast-prompt="target"]');
    const confirm = page.getByRole("button", { name: "Confirm", exact: true });
    const creature = (name: string) =>
      page.getByRole("button", { name, exact: true });
    const amount = (name: string) =>
      page
        .getByRole("group", { name: `Damage to ${name}`, exact: true })
        .getByRole("status");
    await expectText(prompt, "Divide 3 damage · 0/3");
    await page.locator('[data-cast-source="Arc Lightning"]').waitFor();
    await expect(confirm).toBeDisabled();
    // The first target takes everything; the next takes one from it.
    await creature("Goblin Guide").click();
    await expectText(amount("Goblin Guide"), "3");
    await expectText(prompt, "Divide 3 damage · 3/3");
    if (state === "match-divide") {
      await creature("Goblin Chieftain").click();
      await expectText(amount("Goblin Guide"), "2");
      await expectText(amount("Goblin Chieftain"), "1");
      await page.getByRole("button", { name: "Target Alex" }).click();
      await expectText(amount("Goblin Guide"), "1");
      await expectText(amount("Alex"), "1");
      // Three is the most: nobody else is offered.
      await expect(page.getByRole("button", { name: /^Target / })).toHaveCount(
        0,
      );
      await expect(confirm).toBeEnabled();
    }
    if (state === "match-divide-adjust") {
      await creature("Goblin Chieftain").click();
      await page
        .getByRole("button", { name: "More damage to Goblin Chieftain" })
        .click();
      await expectText(amount("Goblin Guide"), "1");
      await expectText(amount("Goblin Chieftain"), "2");
      await page
        .getByRole("button", { name: "Less damage to Goblin Chieftain" })
        .click();
      await expectText(prompt, "Divide 3 damage · 2/3");
      await expect(confirm).toBeDisabled();
      // Pressing a chosen target removes it and leaves the other as it was.
      await creature("Goblin Guide").click();
      await expect(amount("Goblin Guide")).toHaveCount(0);
      await expectText(amount("Goblin Chieftain"), "1");
      await expectText(prompt, "Divide 3 damage · 1/3");
    }
    if (state === "match-divide-cast") {
      await page.getByRole("button", { name: "Target Alex" }).click();
      await expectText(amount("Alex"), "1");
      await confirm.click();
      await prompt.waitFor({ state: "detached" });
      await page.getByRole("button", { name: "Resolve", exact: true }).click();
      await page.locator("[data-match-stack]").waitFor({ state: "detached" });
      // Two of the three went to the Guide, which dies.
      await expect(creature("Goblin Guide")).toHaveCount(0);
    }
    await page.waitForTimeout(700);
  }
  if (state === "match-stack-read") {
    await page.getByRole("option", { name: /^Shock, Morgan/ }).click();
    await page
      .getByRole("option", { name: "Shock, Morgan, targeting You" })
      .and(page.locator('[aria-selected="true"]'))
      .waitFor();
    await page.waitForTimeout(700);
  }
  if (state === "match-stack-oldest") {
    const pile = page.getByRole("listbox", { name: "Inspect stack" });
    await pile.focus();
    await page.keyboard.press("Home");
    await page
      .getByRole("option", { name: /^Lightning Bolt, Alex, targeting You/ })
      .and(page.locator('[aria-selected="true"]'))
      .waitFor();
    await page.waitForTimeout(700);
  }
  if (state === "match-resolve") {
    const resolve = page.getByRole("button", { name: "Resolve", exact: true });
    await resolve.click();
    await page.locator('[data-match-stack="1"]').waitFor();
    await resolve.click();
    // Nothing waits: the pile is gone and the turn action is Next again.
    await page.locator("[data-match-stack]").waitFor({ state: "detached" });
    await page.getByRole("button", { name: "Next", exact: true }).waitFor();
    await page.waitForTimeout(500);
  }
  if (state === "match-opponent") {
    await page.getByRole("button", { name: "View Alex’s board" }).click();
    await page.locator('[data-match-table="seat"]').waitFor();
    await page.waitForTimeout(1200);
  }
  if (state === "match-overview") {
    await page.getByRole("button", { name: "View the whole table" }).click();
    await page.locator('[data-match-table="overview"]').waitFor();
    await page.waitForTimeout(1200);
  }
  if (state === "quests") {
    const kind = new URLSearchParams(test.path.split("?")[1]).get("quests");
    await page
      .getByRole("region", { name: "Deck library", exact: true })
      .waitFor();
    const row = page.getByRole("region", { name: "Daily quests" });
    if (kind === "none") {
      // With no quests there is no row, and the decks are as they were.
      await expect(row).toHaveCount(0);
    } else {
      // Where the dock floats over the page the row ends the deck list.
      await row.scrollIntoViewIfNeeded();
      await row.waitFor();
      const quests = row.getByRole("listitem");
      const swaps = row.getByRole("button", { name: /^Swap / });
      await expect(quests).toHaveCount(kind === "one" ? 1 : 3);
      await expect(quests.first()).toContainText("Cast 20 red or green spells");
      await expect(quests.first()).toContainText("12 / 20");
      // Today's one swap is offered on one quest only.
      await expect(swaps).toHaveCount(1);
      if (kind === "one") {
        await expect(row.getByText(/wins? today/)).toHaveCount(0);
      } else {
        await expect(quests.nth(1)).toContainText("Done");
        await row.getByText("wins today").waitFor();
        await swaps.click();
        if (kind === "refused") {
          await row.getByText("No other quest is available today.").waitFor();
          await expect(swaps).toHaveCount(1);
        } else {
          // Swapped: another quest takes its place and the swap is spent.
          await expect(quests.first()).toContainText(
            "Cast 20 black or green spells",
          );
          await expect(swaps).toHaveCount(0);
        }
      }
    }
    await page.waitForTimeout(900);
  }
  if (
    state === "play-choose" ||
    state === "play-options" ||
    state === "play-search" ||
    state === "play-refused" ||
    state === "play-found"
  ) {
    const kind = new URLSearchParams(test.path.split("?")[1]).get("search");
    const collection = test.path.startsWith("/collection");
    await page.getByRole("button", { name: "Play Goblins" }).click();
    const how = page.getByRole("dialog", { name: "Play Goblins" });
    const findChoice = how.getByRole("button", { name: /^Find a match/ });
    await how.getByRole("button", { name: /^Play with friends/ }).waitFor();
    if (state === "play-choose") {
      if (kind === "none") {
        // A format that cannot be queued says so and offers only the table.
        await expect(findChoice).toBeDisabled();
        await expect(findChoice).toContainText(
          "Not available for Standard yet.",
        );
      } else await expect(findChoice).toBeEnabled();
    } else {
      await findChoice.click();
      const options = page.getByRole("dialog", { name: "Find a match" });
      await options.waitFor();
      const three = options.getByRole("radio", { name: "Best of three" });
      const ranked = options.getByRole("switch", { name: "Ranked" });
      // Only a Collection deck can be played for rank.
      await expect(ranked).toHaveCount(collection ? 1 : 0);
      if (kind === "single-only") await expect(three).toBeDisabled();
      const start = options.getByRole("button", { name: "Find match" });
      if (state === "play-options") {
        // Back returns to the two ways to play.
        await options.getByRole("button", { name: "Back" }).click();
        await how.waitFor();
        await findChoice.click();
        await options.waitFor();
      }
      if (state === "play-search") {
        await three.check({ force: true });
        await start.click();
        const searching = page.getByRole("dialog", { name: "Finding a match" });
        await searching
          .getByText(
            collection
              ? "Standard · Best of three · Ranked"
              : "Standard · Best of three",
          )
          .waitFor();
        await searching.getByText(/Searching for players \d:\d\d/).waitFor();
        // Cancelling returns to the options, with nothing left running.
        await searching.getByRole("button", { name: "Cancel search" }).click();
        await options.waitFor();
        await start.click();
        await searching.waitFor();
      }
      if (state === "play-refused") {
        await start.click();
        await options.getByText("Couldn’t start the search.").waitFor();
        await expect(start).toBeEnabled();
      }
      if (state === "play-found") {
        await start.click();
        // A found match goes straight to the table: no lobby in between.
        await page.getByRole("dialog", { name: "Match found" }).waitFor();
        await page.waitForURL(/\/match$/);
        await page.getByRole("region", { name: "Players" }).waitFor();
      }
    }
    await page.waitForTimeout(900);
  }
  if (state === "league-history") {
    const kind = new URLSearchParams(test.path.split("?")[1]).get("history");
    const table = page.getByRole("table", { name: "Match history" });
    await table.waitFor();
    if (kind === "empty") {
      await table.getByText("No matches yet").waitFor();
    } else if (kind === "failed") {
      // A list that could not be read says so; it does not claim there are none.
      await page.getByText("Couldn’t load your matches.").waitFor();
      await expect(table.getByText("No matches yet")).toHaveCount(0);
    } else {
      const rows = table.getByRole("rowgroup").getByRole("row");
      await expect(rows.first()).toBeVisible();
      if (kind === "slow") {
        // Two matches are still being read; the rest already show.
        expect(await rows.count()).toBeGreaterThanOrEqual(6);
        await expect(rows.first()).toContainText("Sam and 2 others");
      } else {
        // Newest first, and the oldest are shown as the end comes into view.
        await expect(rows.first()).toContainText("Won");
        await expect(rows.first()).toContainText("Alex");
        await expect(rows.first()).toContainText("Today");
        await rows.last().scrollIntoViewIfNeeded();
        await expect(rows).toHaveCount(10);
      }
    }
    await page.waitForTimeout(900);
  }
  if (state === "league-scrolled") {
    await page
      .getByRole("table", { name: /standings$/ })
      .locator("[tabindex='0']")
      .first()
      .evaluate(async (body) => {
        // Your row is past the first page, so read on until it arrives.
        for (let pass = 0; pass < 6; pass += 1) {
          if (body.querySelector("[data-you='true']")) break;
          // A sideways phone scrolls the page, not the list.
          const scroller =
            getComputedStyle(body).overflowY === "visible"
              ? (body.closest("section") ?? body)
              : body;
          scroller.scrollTop = scroller.scrollHeight;
          await new Promise((resolve) => setTimeout(resolve, 1000));
        }
        body
          .querySelector<HTMLElement>("[data-you='true']")
          ?.scrollIntoView({ block: "center" });
      });
    await page.waitForTimeout(1200);
  }
  if (state === "league-profile") {
    await page.getByRole("button", { name: "EmberFox’s profile" }).click();
    await page
      .getByRole("dialog")
      .getByRole("heading", { name: "EmberFox" })
      .first()
      .waitFor();
    await page.waitForTimeout(900);
  }
  if (state === "prediction-bet" || state === "prediction-bet-armed") {
    await page
      .getByRole("button", { name: /^Make a prediction: Will Lightning Bolt/ })
      .click();
    await page.getByRole("heading", { name: "Make a prediction" }).waitFor();
    await page.waitForTimeout(800);
    if (state === "prediction-bet-armed") {
      await page.getByRole("button", { name: "Yes", exact: true }).click();
      await page.waitForTimeout(1400);
    }
  }
  if (state === "prediction-view") {
    await page
      .getByRole("button", { name: /^View prediction: / })
      .first()
      .click();
    await page
      .getByRole("heading", { name: /^Prediction( result)?$/ })
      .waitFor();
    await page.waitForTimeout(800);
  }
  if (state === "prediction-mine-view") {
    await page
      .getByRole("button", { name: /^View: Will Goblin Bushwhacker/ })
      .click();
    await page.getByRole("heading", { name: "Prediction result" }).waitFor();
    await page.waitForTimeout(800);
  }
  if (
    state === "prediction-create" ||
    state === "prediction-picker" ||
    state === "prediction-calendar"
  ) {
    await page
      .getByRole("button", { name: "Create prediction", exact: true })
      .first()
      .click();
    await page.getByRole("heading", { name: "Create prediction" }).waitFor();
    await page.waitForTimeout(600);
    if (state !== "prediction-create") {
      await page.getByRole("button", { name: "Choose card" }).click();
      await page.getByRole("heading", { name: "Choose card" }).waitFor();
      await page.waitForTimeout(700);
    }
    if (state === "prediction-calendar") {
      const first = page.getByRole("option").first();
      if (await first.isVisible()) {
        await first.click();
        await page.getByRole("button", { name: "Select" }).click();
      } else await page.getByRole("button", { name: /^Back/ }).click();
      await page.getByRole("heading", { name: "Create prediction" }).waitFor();
      await page.getByRole("button", { name: /^End date/ }).click();
      await page.waitForTimeout(800);
    }
  }
  if (state === "market-filters") {
    await page.getByRole("button", { name: /^Filters/ }).click();
    await page.getByRole("heading", { name: "Filters" }).waitFor();
    await page.waitForTimeout(800);
  }
  if (state === "market-buy") {
    // A compact layout shows the inspector, with Buy, once a row is chosen.
    const buy = page.getByRole("button", { name: "Buy", exact: true });
    if (!(await buy.isVisible()))
      await page.locator("[data-listing] button").first().click();
    await buy.click();
    await page.getByRole("heading", { name: "Buy listing" }).waitFor();
    await page.waitForTimeout(800);
  }
  if (state === "market-create" || state === "market-create-terms") {
    await page.getByRole("button", { name: "Create listing" }).click();
    await page.getByRole("heading", { name: "Create listing" }).waitFor();
    if (state === "market-create-terms") {
      // The radio itself is visually hidden; its row is the target.
      await page.locator("label:has(input[type=radio])").first().click();
      await page.getByRole("button", { name: "Next" }).click();
      await page.getByRole("button", { name: "Back" }).waitFor();
    }
    await page.waitForTimeout(800);
  }
  if (state === "market-cancel") {
    // The probe's own listing is the one that can be cancelled.
    await page.locator('[data-listing="mine"] button').click();
    await page.getByRole("button", { name: "Cancel listing" }).click();
    await page.getByRole("heading", { name: "Cancel listing?" }).waitFor();
    await page.waitForTimeout(800);
  }
  if (state === "card-filters") {
    await page.getByRole("button", { name: /^Filters/ }).click();
    await page.getByRole("heading", { name: "Filters" }).waitFor();
    await page.waitForTimeout(800);
  }
  if (state === "card-open") {
    await page.getByRole("button", { name: /^Lightning Bolt, / }).click();
    await page.getByText("Owned printings").first().waitFor();
    await page.waitForTimeout(800);
  }
  if (state === "pack-open") {
    await page
      .getByRole("button", { name: "Open a Foundations Play Booster" })
      .click();
    await page.getByRole("button", { name: "Reveal all" }).click();
    await page.getByRole("button", { name: "Done" }).waitFor();
    await page.waitForTimeout(1200);
  }
  if (state === "end-of-list") {
    const covered = await page.evaluate(() => {
      const buttons = document.querySelectorAll<HTMLElement>(
        "section[aria-label=Decks] article button",
      );
      const last = buttons[buttons.length - 1];
      if (!last) return "no deck tiles rendered";
      last.focus();
      const rect = last.getBoundingClientRect();
      const dock = document
        .querySelector("[data-frame=dock]")
        ?.getBoundingClientRect();
      if (rect.bottom > window.innerHeight + 1)
        return "focused control is off screen";
      if (
        dock &&
        rect.bottom > dock.top &&
        rect.right > dock.left &&
        rect.left < dock.right
      )
        return "focused control sits under the dock";
      return null;
    });
    if (covered) findings.push({ check: "focus", detail: covered });
    await page.waitForTimeout(300);
  }
  const expectsGlass =
    state !== "reduced-transparency" && state !== "forced-colors";
  findings.push(
    ...(await page.evaluate(inspect, {
      expectsGlass,
      minimumTarget: viewport.touch ? 44 : 24,
    })),
  );
  for (const error of errors)
    findings.push({ check: "console", detail: error });
  // Software rendering of the blurred surfaces can take a while to settle a frame.
  await page.screenshot({
    path: resolve(output, `${caseName(test)}.png`),
    timeout: 120_000,
  });
  await context.close();
  return findings;
}

const caseName = (test: Case): string =>
  [
    test.viewport.name,
    test.path === BUILDER_PATH
      ? "builder"
      : test.path
          .split(/[/?=&]/)
          .filter((part) => part && part !== "state")
          .join("-"),
    `art-${test.art}`,
    `decks-${test.decks}`,
    test.state,
  ]
    .filter(Boolean)
    .join("__");

await mkdir(output, { recursive: true });
// The full browser in headless mode can use the GPU. The blurred surfaces take
// about a minute per frame in the software-rendered headless shell.
const browser = await chromium.launch({
  headless: true,
  channel: "chromium",
  args: ["--use-angle=metal", "--ignore-gpu-blocklist"],
});
const results: { name: string; findings: Finding[] }[] = [];
try {
  for (const test of cases) {
    const name = caseName(test);
    if (filter && !name.includes(filter)) continue;
    let findings: Finding[];
    try {
      findings = await run(browser, test);
    } catch (error: unknown) {
      findings = [{ check: "run", detail: String(error).slice(0, 300) }];
    }
    results.push({ name, findings });
    console.info(
      `${findings.length === 0 ? "pass" : "FAIL"}  ${name}${findings
        .map((finding) => `\n        ${finding.check}: ${finding.detail}`)
        .join("")}`,
    );
  }
  await writeFile(
    resolve(output, "results.json"),
    JSON.stringify(results, null, 2),
  );
  await writeFile(
    resolve(output, "index.html"),
    `<!doctype html><meta charset="utf-8"><title>Frame stress</title><style>body{margin:0;padding:24px;background:#111;color:#eee;font:14px system-ui}main{display:flex;flex-wrap:wrap;gap:20px;align-items:flex-start}figure{margin:0}img{display:block;height:340px;border:1px solid #444}figcaption{max-width:340px;padding-top:6px;overflow-wrap:anywhere}.fail{color:#ff9d8f}</style><main>${results
      .map(
        (result) =>
          `<figure><img src="${result.name}.png" alt=""><figcaption class="${result.findings.length ? "fail" : ""}">${result.name}${result.findings.length ? ` (${result.findings.length} findings)` : ""}</figcaption></figure>`,
      )
      .join("")}</main>`,
  );
  const sheet = await browser.newPage({
    viewport: { width: 2200, height: 1200 },
  });
  await sheet.goto(`file://${resolve(output, "index.html")}`);
  await sheet.screenshot({
    path: resolve(output, "contact-sheet.png"),
    fullPage: true,
  });
} finally {
  await browser.close();
}
const failed = results.filter((result) => result.findings.length > 0);
console.info(
  `\n${results.length - failed.length} of ${results.length} cases passed.`,
);
if (failed.length > 0) process.exitCode = 1;

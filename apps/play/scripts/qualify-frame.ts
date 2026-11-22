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
import { chromium, type Browser, type Page } from "@playwright/test";
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
    | "league-profile";
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
  // League on every screen, both sections, then its formats and states.
  ...viewports.flatMap((viewport): Case[] =>
    ["/league/season", "/league/standings"].map((path) => ({
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
      !control.closest("[data-frame=dock]")
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
          .split(/[/?=]/)
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

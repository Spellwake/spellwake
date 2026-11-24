/**
 * Controlled check of account entry and the account deck screens against
 * stand-in Auth and application services. It starts its own configured dev
 * server, answers every Auth and API request from memory, and drives the real
 * screens: sign-in, email code, provider return, username, then the shelf and
 * builder. It proves the screens speak the wire contracts; it does not prove
 * the hosted services or real providers, which need their own smoke test.
 *
 * Usage: bun apps/play/scripts/qualify-account.ts <output-directory>
 */
import { chromium, type Page, type Route } from "@playwright/test";
import { mkdir, writeFile } from "node:fs/promises";
import { resolve } from "node:path";
import { createServer } from "vite";

const output = resolve(process.argv[2] ?? "");
if (!process.argv[2]) throw new Error("Give an output directory.");

const AUTH = "http://127.0.0.1:59991";
const API = "http://127.0.0.1:59992";
const PORT = 6219;
const id = (n: number): string =>
  `00000000-0000-4000-8000-${String(n).padStart(12, "0")}`;
const SUBJECT = id(1);
const NEW_SUBJECT = id(4);
const NEW_ACCOUNT = id(5);
const OWNER_TOKEN = "stand-in-access-token";
const NEW_TOKEN = "stand-in-new-person-token";
const REJECTED_TOKEN = "stand-in-rejected-token";
const GOOD_CODE = "123456";
const ACCOUNT = id(2);
const STANDARD = { id: id(10), key: "standard", name: "Standard" };
const MODERN = { id: id(11), key: "modern", name: "Modern" };
const BOLT = id(20);
const KRENKO = id(21);
const GUIDE = id(22);
const TOKEN = id(23);
const BOLT_PRINT = id(30);
const GOBLINS = id(40);
const BREW = id(41);
const appearance = {
  backId: id(53),
  sleeveId: id(54),
  diceId: id(55),
  playmatId: null,
};
/** The account's saved defaults. Save in Default settings changes them. */
let accountDefaults: typeof appearance = { ...appearance };
const cards = [
  { id: BOLT, name: "Lightning Bolt", oracleRef: "bolt" },
  { id: KRENKO, name: "Krenko, Mob Boss", oracleRef: "krenko" },
  { id: GUIDE, name: "Goblin Guide", oracleRef: "guide" },
];
const GUIDE_PRINT = id(32);
/** In id order, as the catalogue returns them. */
const printings = [
  {
    id: BOLT_PRINT,
    definitionId: BOLT,
    externalPrintingRef: "msc-806",
    rarity: "uncommon",
    finish: "nonfoil",
    collectorNumber: "806",
    language: "en",
    set: { id: id(31), name: "Secret Lair Commander", code: "msc" },
  },
  {
    id: GUIDE_PRINT,
    definitionId: GUIDE,
    externalPrintingRef: "zen-126",
    rarity: "uncommon",
    finish: "foil",
    collectorNumber: "126",
    language: "en",
    set: { id: id(33), name: "Zendikar", code: "zen" },
  },
];

type Entry = {
  definitionId: string;
  printingId: string | null;
  section: string;
  quantity: string;
};
type Deck = {
  id: string;
  name: string;
  mode: "open" | "collection";
  format: typeof STANDARD;
  revision: { id: string; number: string; entries: Entry[] } | null;
  appearance: typeof appearance;
};
/** The stand-in server's whole state. */
const decks = new Map<string, Deck>([
  [
    GOBLINS,
    {
      id: GOBLINS,
      name: "Goblins",
      mode: "collection",
      format: STANDARD,
      appearance: { ...appearance },
      revision: {
        id: id(60),
        number: "1",
        entries: [
          {
            definitionId: BOLT,
            printingId: BOLT_PRINT,
            section: "main",
            quantity: "4",
          },
          {
            definitionId: KRENKO,
            printingId: null,
            section: "commander",
            quantity: "1",
          },
          // A section the builder does not show. A save must carry it back.
          {
            definitionId: TOKEN,
            printingId: null,
            section: "tokens",
            quantity: "2",
          },
        ],
      },
    },
  ],
  [
    BREW,
    {
      id: BREW,
      name: "Open Brew",
      mode: "open",
      format: MODERN,
      appearance: { ...appearance },
      revision: null,
    },
  ],
]);
let nextId = 100;
/** The account's owned cards and packs. Opening a pack changes them. */
const PACK_ASSET = id(70);
const RECIPE = id(71);
const OPENING = id(90);
const holdings = { bolt: 3, guide: 1, packs: 2 };
/** Lobby rooms the stand-in holds. Mira is the other person in each. */
const PROFILE = id(94);
const MIRA = id(95);
type StandSlot = {
  id: string;
  accountId: string | null;
  seatIndex: string;
  controllerKind: "human" | "ai";
  ready: boolean;
  deckRevisionId: string | null;
};
type StandRoom = {
  id: string;
  hostId: string;
  formatId: string;
  mode: string;
  state: "lobby" | "countdown" | "closed";
  revision: number;
  countdownAt: string | null;
  slots: StandSlot[];
  /** Seats the room holds; two unless it says otherwise. */
  seats?: number;
};
const rooms = new Map<string, StandRoom>();
/** A room the owner hosts alone with a seat open, and Mira's room with a seat open. */
const HOSTED_ROOM = id(310);
const FRIEND_ROOM = id(311);
const FRIEND_INVITE = `${"F".repeat(42)}Q`;
const WRONG_INVITE = `${"W".repeat(42)}A`;
/** Active invitation tokens by room. */
const invitations = new Map<string, string[]>([[FRIEND_ROOM, [FRIEND_INVITE]]]);
const standSlot = (n: number, accountId: string | null): StandSlot => ({
  id: id(n),
  accountId,
  seatIndex: String(n % 10),
  controllerKind: "human",
  ready: false,
  deckRevisionId: null,
});
const roomSummary = (room: StandRoom) => ({
  id: room.id,
  hostId: room.hostId,
  formatId: room.formatId,
  profileId: PROFILE,
  mode: room.mode,
  competitive: false,
  visibility: "private",
  state: room.state,
  revision: String(room.revision),
  structure: "bo1",
  configurationId: id(96),
  targetSeatCount: String(room.seats ?? 2),
  countdownAt: room.countdownAt,
});
/** Messages with Mira, newest first, as the conversation read returns them. */
const CONVERSATION = id(97);
const chat: {
  id: string;
  senderId: string;
  author: string;
  body: string;
  sentAt: string;
}[] = [
  {
    id: id(98),
    senderId: MIRA,
    author: "Mira",
    body: "Want to play later?",
    sentAt: "2026-11-21T14:20:00.000Z",
  },
];
let miraUnread = 1;
/** Set to lose the answer to the next message sent, as a dropped connection would. */
let loseNextSend = false;
/** While set, asking about a message's outcome says it is not yet known. */
let holdSocialDisposition = false;
const mira = () => ({
  id: MIRA,
  username: "Mira",
  isDeleted: false,
  relationship: "friend",
  friendshipId: id(99),
  isBlocked: false,
  canContact: true,
  conversationId: CONVERSATION,
  unreadMessages: String(miraUnread),
  supporter: false,
});
/** Set to lose the answer to the next pack opening, as a dropped connection would. */
let loseNextOpenReply = false;
/** While set, asking about an opening's outcome says it is not yet known. */
let holdOpenDisposition = false;
const inventoryItem = (
  n: number,
  assetId: string,
  owned: number,
  asset: Record<string, string>,
) => ({
  id: id(n),
  assetId,
  revision: "1",
  owned: String(owned),
  held: "0",
  available: String(owned),
  asset,
});
/** The stand-in market: other people's lots, and the ones this account lists. */
const ASH = id(140);
type MarketLot = {
  id: string;
  seller: { id: string; username: string; isDeleted: boolean };
  assetId: string;
  asset: Record<string, string>;
  quantity: string;
  totalPrice: string;
  revision: string;
};
const marketLots: MarketLot[] = [
  {
    id: id(141),
    seller: { id: MIRA, username: "Mira", isDeleted: false },
    assetId: id(142),
    asset: { kind: "card", printingId: BOLT_PRINT },
    quantity: "4",
    totalPrice: "500",
    revision: "1",
  },
  {
    id: id(143),
    seller: { id: ASH, username: "Ash", isDeleted: false },
    assetId: id(144),
    asset: { kind: "pack", recipeId: RECIPE },
    quantity: "2",
    totalPrice: "6000",
    revision: "1",
  },
];
let nextLot = 150;
/** The stand-in supply: one product whose packs each cost 3 coins more than the last. */
let packsSold = 0;
const PACK_STOCK = 420;
const packTotal = (quantity: number) =>
  quantity * 1000 +
  3 * (quantity * packsSold + (quantity * (quantity - 1)) / 2);
/** Set to lose the answer to the next market command, as a dropped connection would. */
let loseNextMarketReply = false;
/** While set, asking about a market command's outcome says it is not yet known. */
let holdMarketDisposition = false;
/** Set to lose the answer to the next import, as a dropped connection would. */
let loseNextImportReply = false;
const SLEEVE = id(50);
const RETIRED_COSMETIC = id(51);
const EMBER = id(56);
/** Purchases the stand-in has reserved, by request. */
const checkouts = new Map<string, { purchaseId: string; state: string }>();
const cosmetic = (
  n: number,
  name: string,
  kind: string,
  isOwned: boolean,
  isFree = false,
) => ({
  id: id(n),
  name,
  kind,
  isFree,
  isOwned,
  includedWithPatreon: false,
});
const purchase = (
  n: number,
  cosmeticId: string,
  state: string,
  day: number,
) => ({
  purchaseId: id(n),
  cosmeticId,
  createdAt: `2026-11-0${day}T12:00:00Z`,
  state,
  revision: "1",
  price: { currency: "usd", minorUnits: "299" },
  checkout: { status: "unavailable" },
});
/** Patreon link requests the stand-in has issued, by request. */
const patreonFlows = new Map<
  string,
  { stateKey: string; accountId: string; isLinked: boolean }
>();
/** Accounts the stand-in treats as current supporters. */
const supporters = new Set<string>();
/** The one code the stand-in Patreon hands back. */
const PATREON_CODE = "stand-in-patreon-code-0001";
const PATREON_EXPIRY = "2030-01-01T00:10:00.123456Z";
/** Addresses the app opened on hosted checkout. Nothing reaches Stripe. */
const checkoutVisits: URL[] = [];
/** Addresses the app opened on Patreon. Nothing reaches the real site. */
const patreonVisits: URL[] = [];
/** Accounts whose deletion has been accepted, with the request that asked. */
const deleting = new Map<string, string>();
/** Accounts the stand-in has finished deleting. */
const deleted = new Set<string>();
/** Each progress read, and whether it wrongly carried the account's credential. */
const observations: { hadBearer: boolean }[] = [];
const received: { path: string; body: unknown }[] = [];
const outcomes = new Map<string, unknown>();

const summary = (deck: Deck) => ({
  id: deck.id,
  name: deck.name,
  mode: deck.mode,
  currentRevisionId: deck.revision?.id ?? null,
  format: deck.format,
  appearance: deck.appearance,
});
const success = (data: unknown) => ({
  code: 200,
  body: { status: "success", data },
});
const refuse = (data: Record<string, string>) => ({
  code: 409,
  body: { status: "fail", data },
});

/** Who each stand-in bearer is. A new person has no account until they register. */
const people = new Map<
  string,
  { subject: string; method: string; accountId: string | null }
>([
  [OWNER_TOKEN, { subject: SUBJECT, method: "google", accountId: ACCOUNT }],
  [NEW_TOKEN, { subject: NEW_SUBJECT, method: "email_code", accountId: null }],
]);
for (const [roomId, hostId, slots] of [
  [HOSTED_ROOM, ACCOUNT, [standSlot(320, ACCOUNT)]],
  [FRIEND_ROOM, MIRA, [standSlot(330, MIRA), standSlot(331, null)]],
] as const)
  rooms.set(roomId, {
    id: roomId,
    hostId,
    formatId: STANDARD.id,
    mode: "collection",
    state: "lobby",
    revision: 1,
    countdownAt: null,
    slots: [...slots],
  });
const unauthorized = {
  code: 401,
  body: { status: "fail", data: { session: "unauthorized" } },
};

function answer(method: string, url: URL, body: unknown, bearer: string) {
  const path = url.pathname;
  const command = (body ?? {}) as Record<string, unknown>;
  // Deletion progress is read with a capability in the body and no credential.
  if (method === "POST" && path === "/v1/account/deletion/observe") {
    observations.push({ hadBearer: bearer !== "" });
    const accountId = String(command["accountId"]);
    const requestId = String(command["requestId"]);
    if (deleting.get(accountId) !== requestId)
      return refuse({ observer: "unavailable" });
    return success(
      deleted.has(accountId)
        ? {
            accountId,
            requestId,
            phase: "deleted",
            retirementOperationId: id(70),
            authCleanupReceiptId: id(71),
          }
        : { accountId, requestId, phase: "pending" },
    );
  }
  const person = people.get(bearer);
  // A deleted account's credential no longer opens anything.
  if (!person || (person.accountId && deleted.has(person.accountId)))
    return unauthorized;
  if (method === "GET" && path === "/v1/session")
    return success({
      subjectId: person.subject,
      sessionId: id(3),
      method: person.method,
      ...(person.accountId
        ? {
            state: deleting.has(person.accountId)
              ? "deletion_pending"
              : "active",
            accountId: person.accountId,
          }
        : { state: "needs_registration", accountId: null }),
    });
  if (method === "POST" && path === "/v1/account") {
    if (command["username"] === "Moonwatcher")
      return refuse({ username: "unavailable" });
    person.accountId = NEW_ACCOUNT;
    return success({ accountId: NEW_ACCOUNT });
  }
  // Everything below belongs to an account.
  if (!person.accountId) return unauthorized;
  const owns = person.accountId === ACCOUNT;
  if (method === "GET" && path === "/v1/account")
    return success({
      accountId: person.accountId,
      username: owns ? "Planeswalker" : "Stargazer_7",
      state: deleting.has(person.accountId) ? "deletion_pending" : "active",
      appearanceRevision: "1",
      defaults: accountDefaults,
      wallet: { owned: "14500", held: "0", available: "14500" },
    });
  if (method === "GET" && path === "/v1/account/profile")
    return success({
      accountId: person.accountId,
      cardArtProfile: null,
      supporter: supporters.has(person.accountId),
    });
  if (method === "POST" && path.startsWith("/v1/account/patreon/")) {
    const requestId = String(command["requestId"]);
    const stateKey = String(command["stateKey"]);
    const flow = patreonFlows.get(requestId);
    const status = () => {
      const known =
        flow?.stateKey === stateKey && flow.accountId === person.accountId
          ? flow
          : null;
      return success({
        accountId: person.accountId,
        requestId,
        phase: known ? (known.isLinked ? "linked" : "issued") : "unknown",
        expiresAt: known ? PATREON_EXPIRY : null,
        associationId: known?.isLinked ? id(80) : null,
      });
    };
    if (path.endsWith("/start")) {
      patreonFlows.set(requestId, {
        stateKey,
        accountId: person.accountId,
        isLinked: false,
      });
      const authorization = new URL("https://www.patreon.com/oauth2/authorize");
      authorization.search = new URLSearchParams({
        client_id: "stand_in_client",
        redirect_uri: "https://app.spellwake.example/play/patreon/return",
        response_type: "code",
        scope: "identity",
        state: stateKey,
      }).toString();
      return {
        code: 202,
        body: {
          status: "success",
          data: {
            accountId: person.accountId,
            requestId,
            expiresAt: PATREON_EXPIRY,
            authorizationUrl: authorization.href,
          },
        },
      };
    }
    if (path.endsWith("/complete")) {
      if (
        !flow ||
        flow.stateKey !== stateKey ||
        flow.accountId !== person.accountId ||
        command["code"] !== PATREON_CODE
      )
        return refuse({ request: "expired" });
      flow.isLinked = true;
      supporters.add(person.accountId);
      return status();
    }
    if (path.endsWith("/status")) return status();
  }
  if (method === "GET" && path === "/v1/cosmetics") {
    const isEmberPaid = [...checkouts.values()].some(
      (entry) => entry.state === "paid",
    );
    // In id order, as the server returns them.
    return success({
      items: [
        cosmetic(50, "Midnight sleeve", "sleeve", true),
        cosmetic(53, "Classic back", "back", true, true),
        cosmetic(54, "Classic sleeve", "sleeve", true, true),
        cosmetic(55, "Ivory dice", "dice", true, true),
        cosmetic(56, "Ember sleeve", "sleeve", isEmberPaid),
        cosmetic(57, "Moonlit playmat", "playmat", true),
      ],
      nextCursor: null,
    });
  }
  if (method === "POST" && path === "/v1/account/defaults") {
    const expected = command["expected"];
    if (JSON.stringify(expected) !== JSON.stringify(accountDefaults))
      return refuse({
        defaults: "account defaults changed; reload before saving",
      });
    accountDefaults = command["appearance"] as typeof appearance;
    return success({ resourceId: person.accountId });
  }
  if (method === "POST" && path === "/v1/purchases") {
    const requestId = String(command["requestId"]);
    if (command["cosmeticId"] !== EMBER)
      return refuse({ cosmetic: "unavailable" });
    const entry = checkouts.get(requestId) ?? {
      purchaseId: id(90 + checkouts.size),
      state: "pending",
    };
    checkouts.set(requestId, entry);
    return {
      code: 202,
      body: { status: "success", data: { resourceId: entry.purchaseId } },
    };
  }
  if (method === "GET" && path.startsWith("/v1/purchases/")) {
    const entry = [...checkouts.values()].find(
      (candidate) => path === `/v1/purchases/${candidate.purchaseId}`,
    );
    if (!entry) return refuse({ purchase: "unavailable" });
    return success({
      purchaseId: entry.purchaseId,
      cosmeticId: EMBER,
      createdAt: "2026-11-21T12:00:00Z",
      state: entry.state,
      revision: entry.state === "paid" ? "2" : "1",
      price: { currency: "usd", minorUnits: "299" },
      checkout:
        entry.state === "pending"
          ? {
              status: "ready",
              expiresAt: "2030-01-01T00:00:00Z",
              url: "https://checkout.stripe.com/c/pay/stand-in",
            }
          : { status: "unavailable" },
    });
  }
  if (method === "GET" && path === "/v1/purchases")
    return success({
      items: owns
        ? [
            purchase(60, SLEEVE, "paid", 1),
            // An item no longer in the catalogue still has to be listed.
            purchase(61, RETIRED_COSMETIC, "refunded", 3),
          ]
        : [],
      nextCursor: null,
    });
  if (method === "POST" && path === "/v1/account/deletion") {
    if (
      typeof command["requestId"] !== "string" ||
      !/^[0-9a-f]{64}$/.test(String(command["statusKeyHash"]))
    )
      return refuse({ request: "invalid" });
    deleting.set(person.accountId, command["requestId"]);
    return success({ resourceId: person.accountId });
  }
  if (method === "GET" && path === "/v1/inventory")
    return success({
      items: owns
        ? [
            inventoryItem(80, id(75), holdings.bolt, {
              kind: "card",
              printingId: BOLT_PRINT,
            }),
            inventoryItem(81, id(76), holdings.guide, {
              kind: "card",
              printingId: GUIDE_PRINT,
            }),
            ...(holdings.packs > 0
              ? [
                  inventoryItem(82, PACK_ASSET, holdings.packs, {
                    kind: "pack",
                    recipeId: RECIPE,
                  }),
                ]
              : []),
          ]
        : [],
      nextCursor: null,
    });
  if (method === "GET" && path === "/v1/sets")
    return success({
      items: [
        { id: id(74), name: "Foundations", code: "fdn" },
        { id: id(160), name: "15th Anniversary Cards", code: "p15a" },
      ],
      nextCursor: null,
    });
  if (method === "GET" && path === `/v1/packs/products/${RECIPE}/quote`) {
    const quantity = Number(url.searchParams.get("quantity") ?? "1");
    const remaining = PACK_STOCK - packsSold;
    const enough = remaining >= quantity;
    return success({
      accountId: person.accountId,
      quote: {
        recipeId: RECIPE,
        productId: id(73),
        policyId: id(161),
        revision: String(packsSold + 1),
        remaining: String(remaining),
        unitPrice: String(1000 + 3 * packsSold),
        quantity: String(quantity),
        total: enough ? String(packTotal(quantity)) : null,
        purpose: enough ? "purchase" : "valuation_only",
      },
    });
  }
  if (method === "GET" && path === "/v1/packs/products")
    return success({
      accountId: person.accountId,
      items: [
        {
          id: RECIPE,
          productId: id(73),
          setId: id(74),
          name: "Foundations Play Booster",
          setName: "Foundations",
          revisionRef: "foundations-1",
        },
      ],
      nextCursor: null,
    });
  if (method === "POST" && path === "/v1/economy/commands/disposition") {
    const inner = (command["command"] ?? {}) as { requestId?: string };
    const known =
      holdOpenDisposition || holdMarketDisposition
        ? undefined
        : outcomes.get(inner.requestId ?? "");
    return success(
      known === undefined
        ? { state: "unknown", response: null }
        : (known as { status: string }).status === "success"
          ? { state: "accepted", response: known }
          : { state: "rejected", response: known },
    );
  }
  if (method === "GET" && path === "/v1/market/listings") {
    const owned = url.searchParams.get("owned") === "true";
    const lots = marketLots.filter((lot) =>
      owned
        ? lot.seller.id === person.accountId
        : lot.seller.id !== person.accountId,
    );
    return success({
      accountId: person.accountId,
      items: [...lots]
        .sort((a, b) => a.id.localeCompare(b.id))
        .map((lot) => ({
          ...lot,
          state: "listed",
          // Your own lots come with their (empty) participation record.
          participant: owned
            ? {
                buyer: null,
                paidCoins: null,
                completedAt: null,
                settlement: null,
              }
            : null,
        })),
      nextCursor: null,
    });
  }
  if (
    method === "POST" &&
    path === "/v1/economy/commands" &&
    command["kind"] === "pack_buy"
  ) {
    const inner = command["command"] as Record<string, string>;
    const quantity = Number(inner["quantity"]);
    const result =
      inner["recipeId"] !== RECIPE ||
      inner["expectedRevision"] !== String(packsSold + 1) ||
      inner["expectedTotal"] !== String(packTotal(quantity))
        ? refuse({ quote: "stock or price changed; request a new quote" })
        : (() => {
            packsSold += quantity;
            holdings.packs += quantity;
            return success({ resourceId: id(162) });
          })();
    outcomes.set(inner["requestId"] ?? "", result.body);
    if (loseNextMarketReply) {
      loseNextMarketReply = false;
      return { code: 503, body: { status: "error", message: "Lost." } };
    }
    return result;
  }
  if (
    method === "POST" &&
    path === "/v1/economy/commands" &&
    String(command["kind"]).startsWith("listing_")
  ) {
    const inner = command["command"] as Record<string, unknown> & {
      requestId: string;
    };
    const at = marketLots.findIndex((lot) => lot.id === inner["listingId"]);
    let result: { code: number; body: unknown };
    if (command["kind"] === "listing_buy") {
      const lot = marketLots[at];
      if (
        !lot ||
        lot.revision !== inner["expectedRevision"] ||
        lot.totalPrice !== inner["expectedTotal"]
      )
        result = refuse({ listing: "listing terms or state changed" });
      else {
        marketLots.splice(at, 1);
        result = success({ resourceId: lot.id });
      }
    } else if (command["kind"] === "listing_cancel") {
      const lot = marketLots[at];
      if (!lot || lot.seller.id !== person.accountId)
        result = refuse({ listing: "listing unavailable" });
      else {
        marketLots.splice(at, 1);
        result = success({ resourceId: lot.id });
      }
    } else {
      const lot = inner["lot"] as { assetId: string; quantity: string };
      const listingId = id(nextLot++);
      marketLots.push({
        id: listingId,
        seller: {
          id: person.accountId,
          username: "Planeswalker",
          isDeleted: false,
        },
        assetId: lot.assetId,
        asset:
          lot.assetId === PACK_ASSET
            ? { kind: "pack", recipeId: RECIPE }
            : {
                kind: "card",
                printingId: lot.assetId === id(76) ? GUIDE_PRINT : BOLT_PRINT,
              },
        quantity: lot.quantity,
        totalPrice: String(inner["totalPrice"]),
        revision: "1",
      });
      result = success({ resourceId: listingId });
    }
    outcomes.set(inner.requestId, result.body);
    if (loseNextMarketReply) {
      loseNextMarketReply = false;
      return { code: 503, body: { status: "error", message: "Lost." } };
    }
    return result;
  }
  if (method === "POST" && path === "/v1/economy/commands") {
    const inner = command["command"] as {
      requestId: string;
      inventoryAssetId: string;
    };
    if (
      command["kind"] !== "pack_open" ||
      inner.inventoryAssetId !== PACK_ASSET
    )
      return refuse({ assets: "not enough available assets" });
    holdings.packs -= 1;
    holdings.bolt += 1;
    holdings.guide += 2;
    const result = success({ resourceId: OPENING });
    outcomes.set(inner.requestId, result.body);
    // The opening is made, but its answer is lost on the way back.
    if (loseNextOpenReply) {
      loseNextOpenReply = false;
      return { code: 503, body: { status: "error", message: "Lost." } };
    }
    return result;
  }
  if (method === "GET" && path === `/v1/packs/openings/${OPENING}`)
    return success({
      accountId: person.accountId,
      outcome: {
        id: OPENING,
        inventoryAssetId: PACK_ASSET,
        recipeId: RECIPE,
        operationId: id(91),
        contents: [
          { printingId: BOLT_PRINT, quantity: "1" },
          { printingId: GUIDE_PRINT, quantity: "2" },
        ],
      },
    });
  if (method === "GET" && path === "/v1/formats")
    return success({ items: [STANDARD, MODERN], nextCursor: null });
  if (method === "GET" && path === "/v1/social/threads")
    return success({
      accountId: person.accountId,
      items: owns ? [mira()] : [],
      nextCursor: null,
    });
  if (method === "GET" && path === "/v1/social/status") {
    const wanted = url.searchParams.get("personId");
    return success({
      accountId: person.accountId,
      unreadMessages: owns ? String(miraUnread) : "0",
      personId: wanted,
      person: owns && wanted === MIRA ? mira() : null,
    });
  }
  if (
    method === "GET" &&
    path === `/v1/social/conversations/${CONVERSATION}/messages`
  )
    return success({
      accountId: person.accountId,
      conversationId: CONVERSATION,
      person: mira(),
      items: chat,
      nextCursor: null,
    });
  if (
    method === "POST" &&
    (path === "/v1/social/commands/disposition" ||
      path === "/v1/social/read/disposition")
  ) {
    const inner = ((command["command"] ?? command) as { requestId?: string })
      .requestId;
    const known = holdSocialDisposition ? undefined : outcomes.get(inner ?? "");
    return success(
      known === undefined
        ? { state: "unknown", response: null }
        : { state: "accepted", response: known },
    );
  }
  if (method === "POST" && path === "/v1/social/read") {
    miraUnread = 0;
    const result = success({ resourceId: CONVERSATION });
    outcomes.set(String(command["requestId"]), result.body);
    return result;
  }
  if (method === "POST" && path === "/v1/social/commands") {
    const inner = command["command"] as Record<string, string>;
    if (command["kind"] !== "send_dm")
      return refuse({ contact: "contact unavailable" });
    const message = {
      id: id(nextId++),
      senderId: person.accountId,
      author: "Planeswalker",
      body: String(inner["body"]),
      sentAt: new Date().toISOString(),
    };
    chat.unshift(message);
    const result = success({ resourceId: message.id });
    outcomes.set(String(inner["requestId"]), result.body);
    // The message is sent, but its answer is lost on the way back.
    if (loseNextSend) {
      loseNextSend = false;
      return { code: 503, body: { status: "error", message: "Lost." } };
    }
    return result;
  }
  if (method === "GET" && path === "/v1/table-profiles")
    return success({
      accountId: person.accountId,
      items: [
        {
          id: PROFILE,
          key: "free_for_all",
          minimumSeats: "2",
          seatCapacity: "4",
          hasMatchmaking: false,
        },
      ],
      nextCursor: null,
    });
  if (method === "GET" && path === `/v1/players/${MIRA}/profile`)
    return success({
      viewerAccountId: person.accountId,
      person: { id: MIRA, username: "Mira", isDeleted: false },
      cardArtProfile: null,
      supporter: false,
    });
  if (method === "POST" && path === "/v1/rooms/commands/disposition") {
    const inner = (command["command"] ?? {}) as { requestId?: string };
    const known = outcomes.get(inner.requestId ?? "");
    return success(
      known === undefined
        ? { state: "unknown", response: null }
        : (known as { status: string }).status === "success"
          ? { state: "accepted", response: known }
          : { state: "rejected", response: known },
    );
  }
  if (method === "POST" && path === "/v1/rooms/commands") {
    const kind = command["kind"];
    const inner = command["command"] as Record<string, string | null>;
    const keep = (result: { code: number; body: unknown }) => {
      outcomes.set(String(inner["requestId"]), result.body);
      return result;
    };
    if (kind === "create") {
      const room: StandRoom = {
        id: id(nextId++),
        hostId: person.accountId,
        formatId: String(inner["formatId"]),
        mode: String(inner["mode"]),
        state: "lobby",
        revision: 1,
        countdownAt: null,
        slots: [
          {
            id: id(300),
            accountId: person.accountId,
            seatIndex: "0",
            controllerKind: "human",
            ready: false,
            deckRevisionId: null,
          },
          {
            id: id(301),
            accountId: MIRA,
            seatIndex: "1",
            controllerKind: "human",
            ready: false,
            deckRevisionId: null,
          },
        ],
      };
      rooms.set(room.id, room);
      return keep(success({ resourceId: room.id }));
    }
    const room = rooms.get(String(inner["roomId"]));
    if (!room) return keep(refuse({ room: "unavailable" }));
    if (inner["expectedRevision"] !== String(room.revision))
      return keep(refuse({ revision: "conflict" }));
    room.revision += 1;
    if (kind === "select_deck") {
      const own = room.slots.find(
        (slot) => slot.accountId === person.accountId,
      );
      if (own) own.deckRevisionId = inner["deckRevisionId"] ?? null;
    } else room.state = "closed";
    return keep(success({ resourceId: room.id }));
  }
  const roomPath =
    /^\/v1\/rooms\/([^/]+)(?:\/(ready|unready|configuration|invitations|join))?$/.exec(
      path,
    );
  if (roomPath) {
    const room = rooms.get(roomPath[1] ?? "");
    if (!room)
      return {
        code: 404,
        body: { status: "fail", data: { room: "unavailable" } },
      };
    const action = roomPath[2];
    if (method === "POST" && action === "invitations") {
      if (room.hostId !== person.accountId)
        return refuse({ room: "unavailable" });
      if (command["expectedRevision"] !== String(room.revision))
        return refuse({ revision: "conflict" });
      const token = `${String(nextId++).padStart(42, "I")}A`;
      invitations.set(room.id, [...(invitations.get(room.id) ?? []), token]);
      return success({ resourceId: id(nextId++), token });
    }
    if (method === "POST" && action === "join") {
      const mine = room.slots.find(
        (slot) => slot.accountId === person.accountId,
      );
      if (mine) return success({ resourceId: room.id, slotId: mine.id });
      if (!invitations.get(room.id)?.includes(String(command["token"])))
        return refuse({ invitation: "unavailable" });
      const open = room.slots.find((slot) => slot.accountId === null);
      if (!open) return refuse({ capacity: "full" });
      open.accountId = person.accountId;
      room.revision += 1;
      return success({ resourceId: room.id, slotId: open.id });
    }
    // Only a member may read a room.
    if (!room.slots.some((slot) => slot.accountId === person.accountId))
      return {
        code: 404,
        body: { status: "fail", data: { room: "unavailable" } },
      };
    if (method === "GET" && !action)
      return success({
        accountId: person.accountId,
        room: roomSummary(room),
        // Another person's chosen deck is private.
        slots: {
          items: room.slots.map((slot) => ({
            ...slot,
            deckRevisionId:
              slot.accountId === person.accountId ? slot.deckRevisionId : null,
          })),
          nextCursor: null,
        },
      });
    if (method === "GET" && action === "configuration")
      return success({
        accountId: person.accountId,
        roomId: room.id,
        revision: String(room.revision),
        profileId: PROFILE,
        structure: "bo1",
        teams: [],
      });
    if (method === "POST" && (action === "ready" || action === "unready")) {
      if (command["expectedRevision"] !== String(room.revision))
        return refuse({ revision: "conflict" });
      room.revision += 1;
      const ready = action === "ready";
      for (const slot of room.slots) slot.ready = ready;
      room.state = ready ? "countdown" : "lobby";
      room.countdownAt = ready
        ? new Date(Date.now() + 60_000).toISOString()
        : null;
      const result = success({
        resourceId: room.id,
        revision: String(room.revision),
      });
      outcomes.set(String(command["requestId"]), result.body);
      return result;
    }
  }
  if (method === "GET" && path === "/v1/decks")
    return success({
      items: [...(owns ? decks.values() : [])]
        .sort((a, b) => a.id.localeCompare(b.id))
        .map(summary),
      nextCursor: null,
    });
  if (method === "GET" && path === "/v1/cards") {
    const needle = (url.searchParams.get("query") ?? "").toLowerCase();
    const isExact = url.searchParams.get("exact") === "true";
    return success({
      items: cards.filter((card) =>
        isExact
          ? card.name.toLowerCase() === needle
          : card.name.toLowerCase().includes(needle),
      ),
      nextCursor: null,
    });
  }
  if (method === "GET" && path === "/v1/cards/printings") {
    const name = (url.searchParams.get("name") ?? "").toLowerCase();
    const setCode = url.searchParams.get("setCode")?.toLowerCase();
    const number = url.searchParams.get("collectorNumber");
    const named = cards.find((card) => card.name.toLowerCase() === name);
    return success({
      items: printings.filter(
        (printing) =>
          printing.definitionId === named?.id &&
          (!setCode || printing.set.code === setCode) &&
          (!number || printing.collectorNumber === number),
      ),
      nextCursor: null,
    });
  }
  if (method === "POST" && path === "/v1/catalogue/resolve") {
    const wanted = command as {
      definitionIds: string[];
      printingIds: string[];
    };
    return success({
      cards: cards.filter((card) => wanted.definitionIds.includes(card.id)),
      printings: printings.filter((printing) =>
        wanted.printingIds.includes(printing.id),
      ),
    });
  }
  if (method === "POST" && path === "/v1/decks/commands/disposition") {
    const inner = (command["command"] ?? {}) as { requestId?: string };
    const known = outcomes.get(inner.requestId ?? "");
    return success(
      known === undefined
        ? { state: "unknown", response: null }
        : (known as { status: string }).status === "success"
          ? { state: "accepted", response: known }
          : { state: "rejected", response: known },
    );
  }
  const remember = (result: { code: number; body: unknown }) => {
    outcomes.set(String(command["requestId"]), result.body);
    return result;
  };
  if (method === "POST" && path === "/v1/decks") {
    const created: Deck = {
      id: id(nextId++),
      name: String(command["name"]),
      mode: command["mode"] === "open" ? "open" : "collection",
      format: command["formatId"] === MODERN.id ? MODERN : STANDARD,
      revision: null,
      appearance: { ...accountDefaults },
    };
    decks.set(created.id, created);
    return remember(success({ resourceId: created.id }));
  }
  if (method === "POST" && path === "/v1/decks/import/disposition") {
    const known = outcomes.get(String(command["requestId"]));
    return success(
      known === undefined
        ? { state: "unknown", response: null }
        : (known as { status: string }).status === "success"
          ? { state: "accepted", response: known }
          : { state: "rejected", response: known },
    );
  }
  if (method === "POST" && path === "/v1/decks/import") {
    const revisionId = id(nextId++);
    const created: Deck = {
      id: id(nextId++),
      name: String(command["name"]),
      mode: command["mode"] === "open" ? "open" : "collection",
      format: command["formatId"] === MODERN.id ? MODERN : STANDARD,
      revision: {
        id: revisionId,
        number: "1",
        entries: command["entries"] as Entry[],
      },
      appearance: { ...accountDefaults },
    };
    decks.set(created.id, created);
    const result = remember(success({ resourceId: created.id, revisionId }));
    // The import is made, but its answer is lost on the way back.
    if (loseNextImportReply) {
      loseNextImportReply = false;
      return { code: 503, body: { status: "error", message: "Lost." } };
    }
    return result;
  }
  const [, deckId, action] =
    /^\/v1\/decks\/([^/]+)(?:\/(revisions|settings))?$/.exec(path) ?? [];
  const deck = decks.get(deckId ?? "");
  if (!deck)
    return {
      code: 404,
      body: { status: "fail", data: { deck: "deck unavailable" } },
    };
  if (method === "GET" && !action)
    return success({ deck: summary(deck), revision: deck.revision });
  if (method === "POST" && action === "revisions") {
    if (command["expectedRevisionId"] !== (deck.revision?.id ?? null))
      return remember(
        refuse({ revision: "deck changed; reload before saving" }),
      );
    deck.revision = {
      id: id(nextId++),
      number: String(Number(deck.revision?.number ?? 0) + 1),
      entries: command["entries"] as Entry[],
    };
    return remember(success({ resourceId: deck.revision.id }));
  }
  if (method === "POST" && action === "settings") {
    type Settings = {
      name: string;
      formatId: string;
      appearance: typeof appearance;
    };
    const expected = command["expected"] as Settings;
    const settings = command["settings"] as Settings;
    if (
      expected.name !== deck.name ||
      expected.formatId !== deck.format.id ||
      JSON.stringify(expected.appearance) !== JSON.stringify(deck.appearance)
    )
      return remember(
        refuse({ settings: "deck settings changed; reload before saving" }),
      );
    deck.appearance = settings.appearance;
    deck.name = settings.name;
    deck.format = settings.formatId === MODERN.id ? MODERN : STANDARD;
    return remember(success({ resourceId: deck.id }));
  }
  return { code: 404, body: { status: "error", message: "No such route." } };
}

const sessionFor = (token: string, subject: string) => ({
  access_token: token,
  refresh_token: `${token}-refresh`,
  token_type: "bearer",
  expires_in: 86400,
  expires_at: Math.floor(Date.now() / 1000) + 86400,
  user: {
    id: subject,
    aud: "authenticated",
    role: "authenticated",
    app_metadata: {},
    user_metadata: {},
    created_at: "2026-01-01T00:00:00Z",
  },
});
const authRequests: { path: string; body: Record<string, unknown> }[] = [];
/** The stand-in Auth service: email codes, the provider redirect and its code exchange. */
async function serveAuth(route: Route): Promise<void> {
  const request = route.request();
  const url = new URL(request.url());
  const cors = {
    "access-control-allow-origin": "*",
    "access-control-allow-headers": "*",
    "access-control-allow-methods": "GET, POST, OPTIONS",
  };
  if (request.method() === "OPTIONS") {
    await route.fulfill({ status: 204, headers: cors });
    return;
  }
  const json = (status: number, body: unknown) =>
    route.fulfill({
      status,
      headers: cors,
      contentType: "application/json",
      body: JSON.stringify(body),
    });
  if (url.pathname === "/auth/v1/authorize") {
    // A provider would ask the person to approve. The stand-in approves at once.
    const back = new URL(url.searchParams.get("redirect_to") ?? "");
    back.searchParams.set("code", "stand-in-provider-code");
    await route.fulfill({ status: 302, headers: { location: back.href } });
    return;
  }
  const body = (request.postDataJSON() ?? {}) as Record<string, unknown>;
  authRequests.push({ path: url.pathname, body });
  if (url.pathname === "/auth/v1/otp")
    return json(
      String(body["email"]).startsWith("fail@") ? 500 : 200,
      String(body["email"]).startsWith("fail@") ? { msg: "unavailable" } : {},
    );
  if (url.pathname === "/auth/v1/verify")
    return body["token"] === GOOD_CODE
      ? json(200, sessionFor(NEW_TOKEN, NEW_SUBJECT))
      : json(403, {
          code: 403,
          error_code: "otp_expired",
          msg: "Token has expired or is invalid",
        });
  if (url.pathname === "/auth/v1/token")
    return json(200, sessionFor(OWNER_TOKEN, SUBJECT));
  if (url.pathname === "/auth/v1/logout") return json(204, {});
  return json(404, { msg: "No such route." });
}

async function serve(route: Route): Promise<void> {
  const request = route.request();
  const cors = {
    "access-control-allow-origin": "*",
    "access-control-allow-headers": "authorization, content-type",
    "access-control-allow-methods": "GET, POST, OPTIONS",
  };
  if (request.method() === "OPTIONS") {
    await route.fulfill({ status: 204, headers: cors });
    return;
  }
  const url = new URL(request.url());
  const body: unknown = request.postDataJSON();
  if (request.method() === "POST") received.push({ path: url.pathname, body });
  const bearer = (request.headers()["authorization"] ?? "").replace(
    /^Bearer /,
    "",
  );
  const result = answer(request.method(), url, body, bearer);
  await route.fulfill({
    status: result.code,
    headers: cors,
    contentType: "application/json",
    body: JSON.stringify(result.body),
  });
}

const checks: { name: string; pass: boolean; detail?: string }[] = [];
async function check(
  name: string,
  test: () => Promise<unknown>,
): Promise<void> {
  try {
    const result = await test();
    checks.push({ name, pass: result !== false });
  } catch (error) {
    checks.push({
      name,
      pass: false,
      detail: (error instanceof Error ? error.message : String(error)).slice(
        0,
        400,
      ),
    });
  }
  console.log(`${checks.at(-1)?.pass ? "pass" : "FAIL"}  ${name}`);
}
/**
 * Presses Save and waits for the builder to settle: the server has answered,
 * the builder has re-read the deck, and nothing is left unsaved.
 */
async function saveAndSettle(page: Page): Promise<void> {
  const save = page.getByRole("button", { name: "Save", exact: true });
  const reread = page.waitForResponse(
    (response) =>
      response.url() === `${API}/v1/decks/${GOBLINS}` &&
      response.request().method() === "GET",
  );
  await save.click();
  await reread;
  await save.and(page.locator(":disabled")).waitFor();
}
/** Waits for something the stand-in services will see, such as a request arriving. */
async function expectEventually(
  isTrue: () => boolean,
  timeout = 30_000,
): Promise<void> {
  const deadline = Date.now() + timeout;
  while (!isTrue()) {
    if (Date.now() > deadline) throw new Error("Timed out waiting.");
    await new Promise((resolve) => setTimeout(resolve, 200));
  }
}
const sent = (path: string) =>
  received
    .filter((request) => request.path === path)
    .map((r) => r.body) as Record<string, unknown>[];

process.env["VITE_SUPABASE_URL"] = AUTH;
process.env["VITE_APPLICATION_API_URL"] = API;
// A made-up key in the publishable-key format. It never reaches a real service.
process.env["VITE_SUPABASE_PUBLISHABLE_KEY"] = "sb_publishable_local_stand_in";
const root = resolve(import.meta.dir, "..");
const server = await createServer({
  root,
  // Its own dependency cache, so the everyday dev server's cache is left alone.
  cacheDir: resolve(output, "vite-cache"),
  server: { port: PORT, strictPort: true, host: "127.0.0.1" },
  logLevel: "error",
});
await server.listen();
const origin = `http://127.0.0.1:${PORT}/play`;
await mkdir(output, { recursive: true });
const browser = await chromium.launch({
  channel: "chromium",
  args: ["--use-angle=metal"],
});

async function open(token: string | null): Promise<Page> {
  const context = await browser.newContext({
    viewport: { width: 1440, height: 900 },
    reducedMotion: "reduce",
  });
  await context.route(`${API}/**`, serve);
  await context.route(`${AUTH}/**`, serveAuth);
  await context.route("**/*.glb*", (route) => route.abort());
  // Stands in for hosted checkout. Nothing reaches Stripe, and no money moves.
  await context.route("https://checkout.stripe.com/**", (route) => {
    checkoutVisits.push(new URL(route.request().url()));
    return route.fulfill({
      contentType: "text/html",
      body: "<!doctype html><title>Stand-in checkout</title><p>Stand-in checkout</p>",
    });
  });
  // Stands in for Patreon's own authorisation page: it approves at once and
  // sends the person back, as Patreon does. The real site is never contacted.
  await context.route("https://www.patreon.com/**", (route) => {
    const visited = new URL(route.request().url());
    patreonVisits.push(visited);
    const back = new URLSearchParams({
      code: PATREON_CODE,
      state: visited.searchParams.get("state") ?? "",
    });
    return route.fulfill({
      status: 302,
      headers: { location: `${origin}/patreon/return?${back.toString()}` },
    });
  });
  if (token !== null)
    await context.addInitScript(
      ([key, session]) => {
        // Placed once, so a sign-out in the page is not undone by a later load.
        if (window.sessionStorage.getItem("stand-in-seeded")) return;
        window.sessionStorage.setItem("stand-in-seeded", "1");
        window.localStorage.setItem(key, session);
      },
      [
        `spellwake.auth.v1:${new URL(AUTH).host}`,
        JSON.stringify(sessionFor(token, SUBJECT)),
      ] as const,
    );
  const page = await context.newPage();
  // The first load compiles the app from cold, so waits are generous.
  page.setDefaultTimeout(90_000);
  page.setDefaultNavigationTimeout(180_000);
  return page;
}
/** Saves a capture once the screen has finished arriving. */
async function capture(page: Page, name: string): Promise<void> {
  await page.waitForTimeout(700);
  await page.screenshot({ path: resolve(output, name) });
}
/** Opens a path once the document is parsed. Each check then waits for what it needs. */
const visit = (page: Page, path: string) =>
  page.goto(`${origin}${path}`, { waitUntil: "domcontentloaded" });

try {
  const guest = await open(null);
  const guestErrors: string[] = [];
  guest.on("pageerror", (error) => guestErrors.push(error.message));
  /** Waits for a screen by its heading, and for the screen it replaced to finish leaving. */
  const heading = (name: string) => ({
    waitFor: async () => {
      await guest.getByRole("heading", { name }).waitFor();
      await guest.waitForFunction(
        () => document.querySelectorAll("h1").length === 1,
      );
    },
    count: () => guest.getByRole("heading", { name }).count(),
  });
  const email = guest.getByRole("textbox", { name: "Email" });
  const codeField = guest.getByRole("textbox", { name: "Verification code" });
  const username = guest.getByRole("textbox", { name: "Username" });
  await visit(guest, `/collection/decks`);
  await check(
    "signed out: the app opens on sign-in with no navigation and no preview decks",
    async () => {
      await heading("Sign in to Spellwake").waitFor();
      await guest.waitForURL(/\/sign-in\?next=\/collection$|\/sign-in$/);
      return (
        (await heading("Goblins").count()) === 0 &&
        (await guest.getByRole("link", { name: "Spellwake home" }).count()) ===
          0
      );
    },
  );
  await capture(guest, "sign-in.png");
  await check(
    "sign up: the same entry under its own heading, and back",
    async () => {
      await guest.getByRole("button", { name: /Sign up/ }).click();
      await heading("Sign up for Spellwake").waitFor();
      await guest.getByRole("button", { name: "Sign up with Email" }).waitFor();
      await guest.getByRole("button", { name: /Sign in/ }).click();
      await heading("Sign in to Spellwake").waitFor();
    },
  );
  await check(
    "email: a malformed address is refused in the field and nothing is sent",
    async () => {
      await email.fill("mage@");
      await guest.getByRole("button", { name: "Login with Email" }).click();
      await guest.getByText("Enter a valid email address.").waitFor();
      return (
        authRequests.length === 0 &&
        (await email.evaluate((el) => el === document.activeElement))
      );
    },
  );
  await check(
    "email: a failed send keeps the address and says so passively",
    async () => {
      await email.fill("fail@example.com");
      await guest.getByRole("button", { name: "Login with Email" }).click();
      await guest.getByText("Couldn’t send code.").waitFor();
      return (
        (await email.inputValue()) === "fail@example.com" &&
        (await heading("Sign in to Spellwake").count()) === 1
      );
    },
  );
  await check(
    "email: a sent code moves to the code step for that exact address",
    async () => {
      await email.fill("  new@example.com ");
      await email.press("Enter");
      await heading("Enter your code").waitFor();
      await guest
        .getByText("We sent a 6-digit code to new@example.com")
        .waitFor();
      await guest.getByRole("button", { name: /Resend in \d+s/ }).waitFor();
      return authRequests.at(-1)?.body["email"] === "new@example.com";
    },
  );
  await capture(guest, "code.png");
  await check(
    "code: six wrong digits submit once, clear, and name the problem",
    async () => {
      const before = authRequests.filter(
        (r) => r.path === "/auth/v1/verify",
      ).length;
      await codeField.fill("000000");
      await guest.getByText("Invalid or expired code.").waitFor();
      return (
        (await codeField.inputValue()) === "" &&
        authRequests.filter((r) => r.path === "/auth/v1/verify").length ===
          before + 1
      );
    },
  );
  await check(
    "code: a different email returns to entry with the address kept",
    async () => {
      await guest
        .getByRole("button", { name: "Use a different email" })
        .click();
      await heading("Sign in to Spellwake").waitFor();
      const kept = (await email.inputValue()).trim() === "new@example.com";
      await email.press("Enter");
      await heading("Enter your code").waitFor();
      return kept;
    },
  );
  await check(
    "code: the right six digits lead a new person to choose a username",
    async () => {
      await codeField.fill(GOOD_CODE);
      await heading("Choose your username").waitFor();
      await guest.waitForURL(/\/username$/);
    },
  );
  await capture(guest, "username.png");
  await check(
    "username: a bad shape is refused locally, a taken name by the server, both keeping the name",
    async () => {
      await username.fill("moon watcher");
      await username.press("Enter");
      await guest
        .getByText("Use 3–24 letters, numbers or underscores.")
        .waitFor();
      const localOnly = sent("/v1/account").length === 0;
      await username.fill("Moonwatcher");
      await username.press("Enter");
      await guest.getByText("That username is taken.").waitFor();
      return localOnly && (await username.inputValue()) === "Moonwatcher";
    },
  );
  await check(
    "username: a free name creates the account once and opens an empty Collection",
    async () => {
      await username.fill("Stargazer_7");
      await username.press("Enter");
      await guest.getByText("No decks yet").waitFor();
      await guest.getByRole("link", { name: "Spellwake home" }).waitFor();
      const registrations = sent("/v1/account");
      return (
        registrations.length === 2 &&
        registrations[1]?.["username"] === "Stargazer_7"
      );
    },
  );
  await capture(guest, "new-account-collection.png");
  await check("no page errors during entry", () =>
    Promise.resolve(guestErrors.length === 0),
  );
  if (guestErrors.length) console.log(guestErrors.join("\n"));
  await guest.context().close();

  const provider = await open(null);
  await visit(provider, `/sign-in`);
  await check(
    "provider: Google hands off, returns through the callback and lands in the account",
    async () => {
      await provider
        .getByRole("button", { name: "Continue with Google" })
        .click();
      await provider.getByRole("heading", { name: "Goblins" }).waitFor();
      const exchange = authRequests.find((r) => r.path === "/auth/v1/token");
      return (
        exchange?.body["auth_code"] === "stand-in-provider-code" &&
        !provider.url().includes("code=") &&
        /\/collection\/decks$/.test(new URL(provider.url()).pathname)
      );
    },
  );
  await visit(provider, `/auth/callback?code=only-a-code`);
  await check(
    "provider: a malformed return says it could not finish and leads back to sign-in",
    async () => {
      await provider
        .getByRole("heading", { name: "Couldn’t finish signing in" })
        .waitFor();
      return true;
    },
  );
  await provider.context().close();

  const turnedAway = await open(REJECTED_TOKEN);
  await visit(turnedAway, `/collection/decks`);
  await check(
    "refused session: asks for the original method, and Back to sign in ends it",
    async () => {
      await turnedAway
        .getByRole("heading", { name: "Sign in again" })
        .waitFor();
      await turnedAway.getByText("Use your original sign-in method.").waitFor();
      await capture(turnedAway, "sign-in-again.png");
      await turnedAway.getByRole("button", { name: "Back to sign in" }).click();
      await turnedAway
        .getByRole("heading", { name: "Sign in to Spellwake" })
        .waitFor();
      return turnedAway.evaluate(
        (key) => window.localStorage.getItem(key) === null,
        `spellwake.auth.v1:${new URL(AUTH).host}`,
      );
    },
  );
  await turnedAway.context().close();

  const page = await open(OWNER_TOKEN);
  const errors: string[] = [];
  page.on("pageerror", (error) => errors.push(error.message));
  await visit(page, `/collection/decks`);
  await check("shelf: lists the account's collection decks only", async () => {
    await page.getByRole("heading", { name: "Goblins" }).waitFor();
    return (
      (await page.getByRole("heading", { name: "Open Brew" }).count()) === 0
    );
  });
  await capture(page, "shelf.png");
  await visit(page, `/open-play`);
  await check("shelf: Open Play lists open decks only", async () => {
    await page.getByRole("heading", { name: "Open Brew" }).waitFor();
    return (await page.getByRole("heading", { name: "Goblins" }).count()) === 0;
  });

  await visit(page, `/decks/${GOBLINS}`);
  await check(
    "builder: saved entries show under their catalogue names",
    async () => {
      await page.getByText("Lightning Bolt").first().waitFor();
      await page.getByText("Krenko, Mob Boss").first().waitFor();
      await page.getByRole("tab", { name: "Main 4" }).waitFor();
    },
  );
  await check("builder: offers only the commands the server has", async () => {
    await page.getByRole("button", { name: "Deck actions" }).click();
    await page.getByRole("menuitem", { name: "Export" }).waitFor();
    const absent =
      (await page.getByRole("menuitem", { name: "Duplicate" }).count()) === 0 &&
      (await page.getByRole("menuitem", { name: "Delete" }).count()) === 0;
    await page.getByRole("menuitem", { name: "Export" }).click();
    return absent;
  });
  await check("export: names the saved printing", async () => {
    const text = await page.locator("dialog[open] textarea").inputValue();
    await page.getByRole("button", { name: "Close", exact: true }).click();
    return (
      text.includes("4 Lightning Bolt (MSC) 806") && text.includes("Commander")
    );
  });
  await check(
    "search: waits for a name, then lists the server's cards",
    async () => {
      await page.getByText("Search by card name").waitFor();
      await page.getByRole("searchbox", { name: "Search cards" }).fill("gob");
      await page.getByText("Goblin Guide").waitFor();
      return (
        (await page.getByRole("combobox", { name: "Color" }).count()) === 0
      );
    },
  );
  await check(
    "save: sends the opened revision, the added card and the kept section",
    async () => {
      await page
        .getByRole("button", { name: "Add Goblin Guide to Main" })
        .click();
      await page.getByRole("tab", { name: "Main 5" }).waitFor();
      await saveAndSettle(page);
      const [save] = sent(`/v1/decks/${GOBLINS}/revisions`);
      const entries = (save?.["entries"] ?? []) as Entry[];
      return (
        save?.["expectedRevisionId"] === id(60) &&
        entries.some(
          (e) =>
            e.definitionId === GUIDE &&
            e.printingId === null &&
            e.quantity === "1" &&
            e.section === "main",
        ) &&
        entries.some(
          (e) =>
            e.definitionId === BOLT &&
            e.printingId === BOLT_PRINT &&
            e.quantity === "4",
        ) &&
        entries.some((e) => e.definitionId === TOKEN && e.section === "tokens")
      );
    },
  );
  await check(
    "format: a change goes through the settings command against what was opened",
    async () => {
      await page.getByRole("combobox", { name: "Format" }).click();
      await page.getByRole("option", { name: "Modern" }).click();
      await saveAndSettle(page);
      const [change] = sent(`/v1/decks/${GOBLINS}/settings`);
      const expected = change?.["expected"] as {
        name: string;
        formatId: string;
      };
      const settings = change?.["settings"] as {
        name: string;
        formatId: string;
      };
      return (
        expected.formatId === STANDARD.id &&
        settings.formatId === MODERN.id &&
        settings.name === "Goblins" &&
        sent(`/v1/decks/${GOBLINS}/revisions`).length === 1
      );
    },
  );
  await check(
    "conflict: a deck changed elsewhere is refused, and the draft is kept",
    async () => {
      const deck = decks.get(GOBLINS);
      if (deck?.revision)
        deck.revision = { ...deck.revision, id: id(nextId++) };
      await page
        .getByRole("button", { name: "Add Goblin Guide to Main" })
        .click();
      await page.getByRole("tab", { name: "Main 6" }).waitFor();
      await page.getByRole("button", { name: "Save", exact: true }).click();
      await page
        .getByText("This deck changed elsewhere. Reopen it before saving.")
        .waitFor();
      return page
        .getByRole("button", { name: "Save", exact: true })
        .isEnabled();
    },
  );
  await capture(page, "builder-conflict.png");

  // Leaving with the refused draft raises the unsaved-changes dialog; discard it.
  await visit(page, `/collection/decks`);
  await check(
    "create: makes one Untitled deck in this mode and opens it",
    async () => {
      await page.getByRole("button", { name: "Create deck" }).first().click();
      await page.waitForURL(/\/decks\/00000000-0000-4000-8000-0000000001\d\d$/);
      const [create] = sent("/v1/decks");
      await page.getByRole("tab", { name: "Main 0" }).waitFor();
      return (
        create?.["name"] === "Untitled" &&
        create["mode"] === "collection" &&
        sent("/v1/decks").length === 1
      );
    },
  );
  const leaving = await open(OWNER_TOKEN);
  await visit(leaving, `/collection/decks`);
  await check("header: shows the account's own name and balance", async () => {
    await leaving.getByRole("heading", { name: "Goblins" }).waitFor();
    await leaving.getByText(/14[,.\u202f ]?500 Spellwake coins/).waitFor();
    await leaving.getByRole("button", { name: "Account" }).click();
    await leaving.getByRole("menuitem", { name: /Planeswalker/ }).waitFor();
    await capture(leaving, "account-menu.png");
    return true;
  });
  await check(
    "sign out: ends this device's session and returns to sign-in for good",
    async () => {
      await leaving.getByRole("menuitem", { name: "Sign out" }).click();
      await leaving
        .getByRole("heading", { name: "Sign in to Spellwake" })
        .waitFor();
      const isCleared = await leaving.evaluate(
        (key) => window.localStorage.getItem(key) === null,
        `spellwake.auth.v1:${new URL(AUTH).host}`,
      );
      // Reloading a signed-in address must not bring the account back.
      await visit(leaving, `/collection/decks`);
      await leaving
        .getByRole("heading", { name: "Sign in to Spellwake" })
        .waitFor();
      return (
        isCleared &&
        authRequests.some((request) => request.path === "/auth/v1/logout") &&
        (await leaving.getByRole("heading", { name: "Goblins" }).count()) === 0
      );
    },
  );
  await leaving.context().close();

  // Account settings, as the signed-in account. Deletion is last: it is not undone.
  const settings = await open(OWNER_TOKEN);
  settings.on("pageerror", (error) => errors.push(String(error)));
  await visit(settings, `/collection/decks`);
  const defaults = settings.getByRole("dialog", { name: "Default settings" });
  await check(
    "default settings: shows the account's own choices, and Save writes them once",
    async () => {
      await settings.getByRole("heading", { name: "Goblins" }).waitFor();
      await settings.getByRole("button", { name: "Account" }).click();
      await settings
        .getByRole("menuitem", { name: "Default settings" })
        .click();
      const classic = defaults.getByRole("button", { name: "Classic sleeve" });
      await classic.waitFor();
      const startsSaved =
        (await classic.getAttribute("aria-pressed")) === "true";
      const save = defaults.getByRole("button", { name: "Save" });
      const startsDisabled = await save.isDisabled();
      // The account has four slots, and none for a pet.
      const hasPets =
        (await defaults.getByRole("tab", { name: "Pets" }).count()) > 0;
      await defaults.getByRole("button", { name: "Midnight sleeve" }).click();
      await capture(settings, "default-settings.png");
      await save.click();
      await defaults.waitFor({ state: "hidden" });
      const [sent1] = sent("/v1/account/defaults");
      const sentAppearance = sent1?.["appearance"] as Record<string, unknown>;
      const sentExpected = sent1?.["expected"] as Record<string, unknown>;
      return (
        startsSaved &&
        startsDisabled &&
        !hasPets &&
        sent("/v1/account/defaults").length === 1 &&
        sentAppearance["sleeveId"] === id(50) &&
        sentAppearance["backId"] === id(53) &&
        sentExpected["sleeveId"] === id(54) &&
        accountDefaults.sleeveId === id(50) &&
        new URL(settings.url()).searchParams.get("defaults") === null
      );
    },
  );
  // Messages, on a page of their own.
  const talker = await open(OWNER_TOKEN);
  talker.on("pageerror", (error) => errors.push(String(error)));
  await visit(talker, `/collection/decks`);
  await check(
    "messages: Mira's thread reads once; a send whose answer is lost waits for Check delivery and is sent once",
    async () => {
      await talker.getByRole("button", { name: "Messages" }).click();
      const panel = talker.getByRole("dialog", { name: "Messages" });
      await panel.getByText("Want to play later?").waitFor();
      await expectEventually(() => sent("/v1/social/read").length === 1);
      loseNextSend = true;
      holdSocialDisposition = true;
      const box = panel.getByRole("textbox", { name: "Message Mira" });
      await box.fill("Ready when you are.");
      await box.press("Enter");
      const checkButton = panel.getByRole("button", { name: "Check delivery" });
      await checkButton.waitFor();
      const keptText = await box.inputValue();
      holdSocialDisposition = false;
      await checkButton.click();
      await checkButton.waitFor({ state: "detached" });
      await panel.getByText("Ready when you are.").waitFor();
      const sends = sent("/v1/social/commands").filter(
        (body) => body["kind"] === "send_dm",
      );
      return (
        keptText === "Ready when you are." &&
        sends.length === 1 &&
        chat.filter((m) => m.body === "Ready when you are.").length === 1 &&
        (await box.inputValue()) === "" &&
        miraUnread === 0
      );
    },
  );
  await talker.close();

  // The friend-game lobby, on a page of its own.
  const host = await open(OWNER_TOKEN);
  host.on("pageerror", (error) => errors.push(String(error)));
  await visit(host, `/collection/decks`);
  await check(
    "lobby: Play makes one room with the deck, Ready follows the room, and Back closes it once",
    async () => {
      await host.getByRole("button", { name: "Play Goblins" }).click();
      await host.waitForURL(/\/collection\/lobby\?.*room=/);
      const yourDeck = host.getByRole("button", {
        name: /^Your deck: Goblins/,
      });
      await yourDeck.waitFor();
      await host.getByText("Mira", { exact: true }).waitFor();
      const [create] = sent("/v1/rooms/commands");
      const created = create?.["command"] as Record<string, unknown>;
      const goblins = decks.get(GOBLINS);
      const chose = sent("/v1/rooms/commands").find(
        (body) => body["kind"] === "select_deck",
      )?.["command"] as Record<string, unknown> | undefined;
      await host.getByRole("button", { name: "Ready", exact: true }).click();
      await host.getByText(/^Starting in \d+ seconds$/).waitFor();
      const [ready] = sent(`/v1/rooms/${String(chose?.["roomId"])}/ready`);
      await host.getByRole("button", { name: "Back" }).click();
      const confirm = host.getByRole("dialog", { name: "Close lobby?" });
      await confirm
        .getByRole("button", { name: "Close lobby", exact: true })
        .click();
      await host.waitForURL(/\/collection\/decks$/);
      const leaves = sent("/v1/rooms/commands").filter(
        (body) => body["kind"] === "leave",
      );
      return (
        create?.["kind"] === "create" &&
        created["mode"] === "collection" &&
        created["formatId"] === goblins?.format.id &&
        created["profileId"] === PROFILE &&
        chose?.["deckRevisionId"] === goblins?.revision?.id &&
        ready?.["expectedRevision"] === "2" &&
        leaves.length === 1 &&
        rooms.get(String(chose?.["roomId"]))?.state === "closed"
      );
    },
  );
  await host.close();

  // Friend invitations: the host's link, and a friend opening one.
  const inviter = await open(OWNER_TOKEN);
  inviter.on("pageerror", (error) => errors.push(String(error)));
  await inviter
    .context()
    .grantPermissions(["clipboard-read", "clipboard-write"]);
  await visit(inviter, `/collection/lobby?room=${HOSTED_ROOM}`);
  await check(
    "lobby: Copy invite link issues one invitation against the room and copies an address with its token only in the fragment",
    async () => {
      const copy = inviter.getByRole("button", { name: "Copy invite link" });
      await copy.click();
      await inviter.getByText("Invite link copied").first().waitFor();
      const first = await inviter.evaluate(() =>
        navigator.clipboard.readText(),
      );
      // A second press hands out the same link without asking again.
      await copy.click();
      await inviter.waitForTimeout(600);
      const second = await inviter.evaluate(() =>
        navigator.clipboard.readText(),
      );
      const issued = sent(`/v1/rooms/${HOSTED_ROOM}/invitations`);
      const address = new URL(first);
      const token = invitations.get(HOSTED_ROOM)?.[0];
      return (
        issued.length === 1 &&
        issued[0]?.["expectedRevision"] === "1" &&
        address.pathname.endsWith("/collection/lobby") &&
        address.searchParams.get("room") === HOSTED_ROOM &&
        !!token &&
        address.hash === `#invite=${token}` &&
        !address.search.includes(token) &&
        second === first
      );
    },
  );
  await inviter.close();
  const friend = await open(OWNER_TOKEN);
  friend.on("pageerror", (error) => errors.push(String(error)));
  await check(
    "lobby: an invitation link takes a seat before the room is read and then forgets its token; a wrong one says the lobby is unavailable",
    async () => {
      await visit(
        friend,
        `/collection/lobby?room=${FRIEND_ROOM}#invite=${WRONG_INVITE}`,
      );
      await friend
        .getByRole("heading", { name: "Lobby unavailable" })
        .waitFor();
      const refused = sent(`/v1/rooms/${FRIEND_ROOM}/join`).length;
      const seatedEarly = rooms
        .get(FRIEND_ROOM)
        ?.slots.some((slot) => slot.accountId === ACCOUNT);
      await visit(friend, "/collection/decks");
      await visit(
        friend,
        `/collection/lobby?room=${FRIEND_ROOM}#invite=${FRIEND_INVITE}`,
      );
      // Mira hosts this room; the friend arrives as its second player.
      await friend.getByText("Mira").first().waitFor();
      await friend.getByText("2/2").waitFor();
      await friend.waitForURL((url) => url.hash === "");
      const joins = sent(`/v1/rooms/${FRIEND_ROOM}/join`);
      return (
        refused === 1 &&
        !seatedEarly &&
        joins.length === 2 &&
        joins[1]?.["token"] === FRIEND_INVITE &&
        new URL(friend.url()).searchParams.get("room") === FRIEND_ROOM &&
        !!rooms
          .get(FRIEND_ROOM)
          ?.slots.some((slot) => slot.accountId === ACCOUNT)
      );
    },
  );
  await friend.close();

  // Trade, on a page of its own.
  const trader = await open(OWNER_TOKEN);
  trader.on("pageerror", (error) => errors.push(String(error)));
  await visit(trader, `/collection/trade/mira`);
  await check(
    "trade: the account's room says it is unavailable, offers only Back, and asks the service nothing",
    async () => {
      const before = sent("/v1/economy/commands").length;
      await trader
        .getByRole("heading", { name: "Trade unavailable." })
        .waitFor();
      const offers = await trader
        .getByRole("button", { name: "Add items" })
        .count();
      const ready = await trader.getByRole("button", { name: "Ready" }).count();
      await trader.getByRole("button", { name: "Back" }).click();
      await trader.waitForURL(/\/collection\/decks$/);
      return (
        offers === 0 &&
        ready === 0 &&
        sent("/v1/economy/commands").length === before
      );
    },
  );
  await trader.close();

  // Cards and Packs, on a page of their own.
  const collector = await open(OWNER_TOKEN);
  collector.on("pageerror", (error) => errors.push(String(error)));
  await visit(collector, `/collection/cards`);
  await check(
    "cards: each owned card once, with its printings' counts and no invented artwork",
    async () => {
      const bolt = collector.getByRole("button", {
        name: "Lightning Bolt, 3 copies, MSC · 806",
      });
      await bolt.waitFor();
      const guide = await collector
        .getByRole("button", { name: "Goblin Guide, 1 copy, ZEN · 126" })
        .count();
      const images = await collector.locator("[data-card] img").count();
      await collector.getByRole("button", { name: /^Filters/ }).click();
      const filters = collector.getByRole("dialog", { name: "Filters" });
      await filters.getByRole("heading", { name: "Rarity" }).waitFor();
      const hasColor =
        (await filters.getByRole("heading", { name: "Color" }).count()) > 0;
      await filters.getByRole("checkbox", { name: "Zendikar" }).check();
      // Only Goblin Guide has a Zendikar printing.
      await collector
        .getByRole("button", { name: /^Lightning Bolt, / })
        .waitFor({ state: "detached" });
      return guide === 1 && images === 0 && !hasColor;
    },
  );
  await visit(collector, `/collection/packs`);
  await check(
    "packs: an opening whose answer is lost waits for Check opening, then shows that opening's cards",
    async () => {
      await collector.getByText("2 packs").waitFor();
      loseNextOpenReply = true;
      holdOpenDisposition = true;
      await collector
        .getByRole("button", { name: "Open a Foundations Play Booster" })
        .click();
      const stage = collector.getByRole("dialog", {
        name: "Foundations Play Booster",
      });
      const checkButton = stage.getByRole("button", { name: "Check opening" });
      await checkButton.waitFor();
      const closeDisabled = await stage
        .getByRole("button", { name: "Close" })
        .isDisabled();
      const sentBeforeCheck = sent("/v1/economy/commands").length;
      // The outcome can now be read; only Check opening asks for it.
      holdOpenDisposition = false;
      const asked = sent("/v1/economy/commands/disposition").length;
      await checkButton.click();
      await stage.getByRole("button", { name: "Reveal all" }).click();
      await stage.getByRole("button", { name: "Done" }).waitFor();
      const names = await stage
        .getByRole("button", { name: /, (Nonfoil|Foil)$/ })
        .evaluateAll((buttons) =>
          buttons.map((button) => button.getAttribute("aria-label")),
        );
      await stage.getByRole("button", { name: "Done" }).click();
      await collector.getByText("1 pack", { exact: true }).waitFor();
      return (
        closeDisabled &&
        sentBeforeCheck === 1 &&
        sent("/v1/economy/commands").length === 1 &&
        sent("/v1/economy/commands/disposition").length > asked &&
        names.filter((name) => name === "Goblin Guide, Foil").length === 2 &&
        names.includes("Lightning Bolt, Nonfoil") &&
        names.length === 3
      );
    },
  );
  await capture(collector, "packs-after-opening.png");
  await collector.close();

  // Market Listings, on a page of their own.
  const trader2 = await open(OWNER_TOKEN);
  trader2.on("pageerror", (error) => errors.push(String(error)));
  await visit(trader2, `/market/listings`);
  await check(
    "market: a purchase whose answer is lost waits for Check status, which settles that same purchase",
    async () => {
      await trader2.getByText("Seller: Mira").waitFor();
      // The service records no listing time, so Newest is not offered.
      await trader2.getByRole("combobox", { name: "Sort listings" }).click();
      const sorts = await trader2.getByRole("option").allTextContents();
      await trader2.keyboard.press("Escape");
      const before = sent("/v1/economy/commands").length;
      loseNextMarketReply = true;
      holdMarketDisposition = true;
      await trader2.getByRole("button", { name: "Buy", exact: true }).click();
      const dialog = trader2.getByRole("dialog", { name: "Buy listing" });
      await dialog.getByRole("button", { name: "Buy", exact: true }).click();
      await dialog.getByText("Purchase not confirmed.").waitFor();
      const cancelDisabled = await dialog
        .getByRole("button", { name: "Cancel" })
        .isDisabled();
      holdMarketDisposition = false;
      await dialog.getByRole("button", { name: "Check status" }).click();
      await trader2
        .getByText("Purchased 4 copies of Lightning Bolt.")
        .waitFor();
      return (
        !sorts.includes("Newest") &&
        sorts.includes("Price: Low to high") &&
        cancelDisabled &&
        sent("/v1/economy/commands").length === before + 1
      );
    },
  );
  await check(
    "market: a new listing shows among your own, and cancelling it asks first and removes it",
    async () => {
      await trader2.getByRole("button", { name: "Create listing" }).click();
      const create = trader2.getByRole("dialog", { name: "Create listing" });
      await create.locator("label:has(input[type=radio])").first().click();
      await create.getByRole("button", { name: "Next" }).click();
      await create.getByRole("textbox", { name: "Total price" }).fill("300");
      await create
        .getByRole("button", { name: "Create listing", exact: true })
        .click();
      await trader2.getByText("Listing created.").waitFor();
      const mineChecked = await trader2
        .getByRole("checkbox", { name: "Only my listings" })
        .isChecked();
      await trader2.getByText("Seller: You").waitFor();
      await trader2.getByRole("button", { name: "Cancel listing" }).click();
      const confirm = trader2.getByRole("dialog", { name: "Cancel listing?" });
      await confirm.getByRole("button", { name: "Cancel listing" }).click();
      await trader2.getByText("Listing cancelled.").waitFor();
      await trader2.getByText("No active listings.").waitFor();
      return mineChecked;
    },
  );
  await capture(trader2, "market-listings.png");
  await trader2.close();

  // Market Supply, on a page of its own.
  const shopper = await open(OWNER_TOKEN);
  shopper.on("pageerror", (error) => errors.push(String(error)));
  await visit(shopper, `/market/supply`);
  await check(
    "supply: a purchase whose answer is lost waits for Check status, which settles that same purchase",
    async () => {
      await shopper.getByText("420 remaining").waitFor();
      const bare = await shopper.getByText("15th Anniversary Cards").count();
      // The application has no vote source yet, so the vote is not offered.
      const banner = await shopper
        .getByRole("heading", { name: "Choose the next supply drop" })
        .count();
      const before = sent("/v1/economy/commands").length;
      await shopper
        .getByRole("button", { name: "Buy Foundations Play Booster" })
        .click();
      const dialog = shopper.getByRole("dialog", { name: "Buy packs" });
      await dialog.getByRole("button", { name: "Increase Quantity" }).click();
      await dialog.getByText("2003").waitFor();
      loseNextMarketReply = true;
      holdMarketDisposition = true;
      await dialog.getByRole("button", { name: "Buy", exact: true }).click();
      await dialog.getByText("Purchase not confirmed.").waitFor();
      const cancelDisabled = await dialog
        .getByRole("button", { name: "Cancel" })
        .isDisabled();
      holdMarketDisposition = false;
      await dialog.getByRole("button", { name: "Check status" }).click();
      await shopper.getByText("Bought 2 Foundations packs.").waitFor();
      const buys = sent("/v1/economy/commands").slice(before);
      return (
        bare === 1 &&
        banner === 0 &&
        cancelDisabled &&
        buys.length === 1 &&
        JSON.stringify(buys[0]).includes('"expectedTotal":"2003"')
      );
    },
  );
  await capture(shopper, "market-supply.png");
  await shopper.close();

  // Import deck and a deck's Appearance, on a page of their own.
  const importer = await open(OWNER_TOKEN);
  importer.on("pageerror", (error) => errors.push(String(error)));
  await visit(importer, `/collection/decks`);
  await check(
    "import: an answer lost on the way is recovered, making one deck of the known cards",
    async () => {
      loseNextImportReply = true;
      const before = decks.size;
      await importer.getByRole("heading", { name: "Goblins" }).waitFor();
      await importer.getByRole("button", { name: "Import deck" }).click();
      const dialog = importer.getByRole("dialog", { name: "Import deck" });
      await dialog
        .getByRole("textbox", { name: "Deck list" })
        .fill(
          "4 Lightning Bolt (MSC) 806\n2 Goblin Guide\n1 Not A Real Card\n\nSideboard\n1 Krenko, Mob Boss",
        );
      await dialog
        .getByRole("button", { name: "Import deck", exact: true })
        .click();
      await importer.waitForURL(
        /\/decks\/00000000-0000-4000-8000-0000000001\d\d$/,
      );
      await importer.getByRole("tab", { name: "Main 6" }).waitFor();
      const imports = sent("/v1/decks/import");
      const [first] = imports;
      const entries = (first?.["entries"] ?? []) as Entry[];
      return (
        imports.length === 1 &&
        sent("/v1/decks/import/disposition").length === 1 &&
        decks.size === before + 1 &&
        first?.["name"] === "Untitled" &&
        first["mode"] === "collection" &&
        entries.length === 3 &&
        entries.some(
          (e) =>
            e.definitionId === BOLT &&
            e.printingId === BOLT_PRINT &&
            e.quantity === "4" &&
            e.section === "main",
        ) &&
        entries.some(
          (e) =>
            e.definitionId === GUIDE &&
            e.printingId === null &&
            e.quantity === "2",
        ) &&
        entries.some((e) => e.definitionId === KRENKO && e.section !== "main")
      );
    },
  );
  await capture(importer, "import-opened.png");
  await check(
    "appearance: an imported deck starts from the defaults, and Save changes it against them",
    async () => {
      const deckId = importer.url().split("/").pop() ?? "";
      const startsDefault = decks.get(deckId)?.appearance.sleeveId === id(50);
      await importer.getByRole("button", { name: "Appearance" }).click();
      const dialog = importer.getByRole("dialog", { name: "Deck appearance" });
      await dialog.getByRole("button", { name: "Classic sleeve" }).click();
      await dialog.getByRole("button", { name: "Apply" }).click();
      await dialog.waitFor({ state: "hidden" });
      await importer.getByRole("button", { name: "Save", exact: true }).click();
      await expectEventually(
        () => decks.get(deckId)?.appearance.sleeveId === id(54),
      );
      const [change] = sent(`/v1/decks/${deckId}/settings`);
      const expected = change?.["expected"] as {
        appearance: typeof appearance;
      };
      const settings = change?.["settings"] as {
        appearance: typeof appearance;
      };
      return (
        startsDefault &&
        expected.appearance.sleeveId === id(50) &&
        settings.appearance.sleeveId === id(54) &&
        settings.appearance.backId === id(53) &&
        sent(`/v1/decks/${deckId}/revisions`).length === 0
      );
    },
  );
  await importer.close();
  const cosmetics = settings.getByRole("dialog", { name: "Cosmetics" });
  await check(
    "cosmetics: Buy opens hosted checkout once, and ownership comes from the account",
    async () => {
      await visit(settings, `/collection/decks?cosmetics=shop`);
      await cosmetics.getByRole("button", { name: "Ember sleeve" }).click();
      // The catalogue lists no price, so none is shown or guessed.
      const showsPrice = (await cosmetics.getByText(/\$\d/).count()) > 0;
      const opened = settings.context().waitForEvent("page");
      await cosmetics.getByRole("button", { name: "Buy" }).click();
      const tab = await opened;
      await tab.waitForURL(/checkout\.stripe\.com/);
      await cosmetics.getByText("Checkout opened.").waitFor();
      const isOwnedEarly = (await cosmetics.getByText("Owned").count()) > 0;
      // The person pays on the hosted page. The stand-in records it.
      for (const entry of checkouts.values()) entry.state = "paid";
      await tab.close();
      await cosmetics.getByText("Owned").waitFor({ timeout: 20_000 });
      await capture(settings, "cosmetics-owned.png");
      return (
        !showsPrice &&
        !isOwnedEarly &&
        sent("/v1/purchases").length === 1 &&
        checkoutVisits.length === 1 &&
        checkoutVisits[0]?.pathname === "/c/pay/stand-in"
      );
    },
  );
  await visit(settings, `/collection/decks`);
  const dialog = settings.getByRole("dialog", { name: "Account settings" });
  const section = (name: string) => dialog.getByRole("tab", { name });
  await check(
    "account settings: View profile opens the account's own profile",
    async () => {
      await settings.getByRole("heading", { name: "Goblins" }).waitFor();
      await settings.getByRole("button", { name: "Account" }).click();
      await settings.getByRole("menuitem", { name: /Planeswalker/ }).click();
      await dialog.getByRole("heading", { name: "Planeswalker" }).waitFor();
      await capture(settings, "account-settings-profile.png");
      return (
        new URL(settings.url()).searchParams.get("account") === "profile" &&
        // The account has no picture or badge command yet, so neither is offered.
        (await dialog
          .getByRole("button", { name: "Change picture" })
          .isDisabled()) &&
        (await dialog
          .getByRole("button", { name: "Change badge" })
          .isDisabled())
      );
    },
  );
  const connect = dialog.getByRole("button", { name: "Connect Patreon" });
  await check(
    "patreon: says nothing it cannot read, and offers to connect",
    async () => {
      await section("Patreon").click();
      await connect.waitFor();
      return (
        (await connect.isEnabled()) &&
        (await dialog.getByText("Not linked.").count()) === 0 &&
        (await dialog.getByText("Linked").count()) === 0
      );
    },
  );
  await check(
    "patreon: a return that is not this device's request is refused, its code unsent",
    async () => {
      const foreign = new URLSearchParams({
        code: "someone-elses-code-000001",
        state: "BBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBA",
      });
      await visit(settings, `/patreon/return?${foreign.toString()}`);
      await dialog.getByText("Couldn’t link Patreon.").first().waitFor();
      await connect.waitFor();
      const address = new URL(settings.url());
      return (
        sent("/v1/account/patreon/complete").length === 0 &&
        address.searchParams.get("account") === "patreon" &&
        !address.searchParams.has("code") &&
        !address.searchParams.has("state") &&
        (await connect.isEnabled())
      );
    },
  );
  await check(
    "patreon: Connect saves one request, opens Patreon's own page, and the return links it",
    async () => {
      await connect.click();
      await dialog.getByText("Active supporter").waitFor();
      await capture(settings, "account-settings-patreon-linked.png");
      const [start] = sent("/v1/account/patreon/start");
      const [complete] = sent("/v1/account/patreon/complete");
      const [visited] = patreonVisits;
      const saved = await settings.evaluate(
        (key) => window.localStorage.getItem(key),
        `spellwake.patreon-link.v1:${API}:${ACCOUNT}`,
      );
      const record = JSON.parse(saved ?? "{}") as Record<string, unknown>;
      const address = new URL(settings.url());
      return (
        sent("/v1/account/patreon/start").length === 1 &&
        sent("/v1/account/patreon/complete").length === 1 &&
        patreonVisits.length === 1 &&
        visited?.pathname === "/oauth2/authorize" &&
        visited.searchParams.get("state") === start?.["stateKey"] &&
        complete?.["requestId"] === start?.["requestId"] &&
        complete?.["stateKey"] === start?.["stateKey"] &&
        complete?.["code"] === PATREON_CODE &&
        record["phase"] === "linked" &&
        record["requestId"] === start?.["requestId"] &&
        address.searchParams.get("account") === "patreon" &&
        !address.searchParams.has("code") &&
        (await dialog.getByText("Linked", { exact: true }).count()) === 1 &&
        (await dialog
          .getByRole("button", { name: "Manage on Patreon" })
          .isEnabled())
      );
    },
  );
  await check(
    "patreon: the link is read again after a reload, and the code is kept nowhere",
    async () => {
      const reads = sent("/v1/account/patreon/status").length;
      await visit(settings, `/collection/decks?account=patreon`);
      await dialog.getByText("Active supporter").waitFor();
      const kept = await settings.evaluate(
        (code) =>
          [window.localStorage, window.sessionStorage].some((storage) =>
            Object.keys(storage).some(
              (key) =>
                key.includes(code) ||
                (storage.getItem(key) ?? "").includes(code),
            ),
          ),
        PATREON_CODE,
      );
      return (
        !kept &&
        sent("/v1/account/patreon/status").length > reads &&
        sent("/v1/account/patreon/complete").length === 1 &&
        patreonVisits.length === 1
      );
    },
  );
  await check(
    "account settings: purchases come from the account, named from the catalogue",
    async () => {
      await section("Billing & purchases").click();
      const row = (name: RegExp) => dialog.getByRole("row", { name });
      await row(/Midnight sleeve/).waitFor();
      await capture(settings, "account-settings-billing.png");
      return (
        (await row(/Midnight sleeve Nov 1, 2026 \$2\.99 USD Paid/).count()) ===
          1 &&
        (await row(/Cosmetic Nov 3, 2026 \$2\.99 USD Refunded/).count()) ===
          1 &&
        (await dialog
          .getByRole("button", { name: "Manage billing" })
          .isDisabled())
      );
    },
  );
  await check(
    "account settings: deletion asks first, is sent once, and stays requested",
    async () => {
      await section("Account").click();
      const confirm = settings.getByRole("dialog", {
        name: "Delete your account?",
      });
      await dialog.getByRole("button", { name: "Delete account" }).click();
      await confirm.getByRole("button", { name: "Cancel" }).click();
      await confirm.waitFor({ state: "hidden" });
      const wasCancelled = sent("/v1/account/deletion").length === 0;
      await dialog.getByRole("button", { name: "Delete account" }).click();
      await confirm.getByRole("button", { name: "Delete account" }).click();
      await dialog
        .getByRole("heading", { name: "Deletion requested" })
        .waitFor();
      await capture(settings, "account-settings-deletion.png");
      const requests = sent("/v1/account/deletion");
      const isLocked =
        (await section("Profile").getAttribute("aria-disabled")) === "true";
      // The server now reports the account as pending, so a reload agrees.
      await visit(settings, `/collection/decks?account=profile`);
      await dialog
        .getByRole("heading", { name: "Deletion requested" })
        .waitFor();
      return (
        wasCancelled &&
        requests.length === 1 &&
        typeof requests[0]?.["requestId"] === "string" &&
        isLocked
      );
    },
  );
  await check(
    "pending deletion: the account is held outside the app, with no navigation",
    async () => {
      await dialog.getByRole("button", { name: "Close", exact: true }).click();
      await dialog.waitFor({ state: "hidden" });
      const held = settings.getByRole("heading", {
        name: "Deletion requested",
      });
      await held.waitFor();
      await capture(settings, "deletion-pending.png");
      // Any signed-in address leads to the same place.
      await visit(settings, `/open-play`);
      await held.waitFor();
      return (
        (await settings.locator("[data-frame=header]").count()) === 0 &&
        (await settings.getByRole("heading", { name: "Goblins" }).count()) ===
          0 &&
        (await settings
          .getByRole("button", { name: "Back to sign in" })
          .count()) === 1
      );
    },
  );
  await check(
    "pending deletion: progress is read without the credential, and never re-sent",
    async () => {
      await expectEventually(() => observations.length > 0);
      return (
        observations.every((read) => !read.hadBearer) &&
        sent("/v1/account/deletion").length === 1
      );
    },
  );
  await check(
    "deleted: this device signs itself out and clears the account it held",
    async () => {
      deleted.add(ACCOUNT);
      await settings
        .getByRole("heading", { name: "Sign in to Spellwake" })
        .waitFor();
      const left = await settings.evaluate(
        ([authKey, journalKey, patreonKey]) => ({
          session: window.localStorage.getItem(authKey),
          journal: window.localStorage.getItem(journalKey),
          patreon: window.localStorage.getItem(patreonKey),
        }),
        [
          `spellwake.auth.v1:${new URL(AUTH).host}`,
          `spellwake.deletion.v1:${API}:${ACCOUNT}`,
          `spellwake.patreon-link.v1:${API}:${ACCOUNT}`,
        ] as const,
      );
      const reads = observations.length;
      // Nothing is left to follow, and a reload does not bring the account back.
      await visit(settings, `/collection/decks`);
      await settings
        .getByRole("heading", { name: "Sign in to Spellwake" })
        .waitFor();
      await settings.waitForTimeout(12_000);
      return (
        left.session === null &&
        left.journal === null &&
        // The saved Patreon request, with its private state, goes with the account.
        left.patreon === null &&
        observations.length === reads &&
        sent("/v1/account/deletion").length === 1
      );
    },
  );
  await settings.context().close();

  await check("no page errors", () => Promise.resolve(errors.length === 0));
  if (errors.length) console.log(errors.join("\n"));
  await page.context().close();
} finally {
  await browser.close();
  await server.close();
}
await writeFile(
  resolve(output, "results.json"),
  JSON.stringify({ checks }, null, 2),
);
const failed = checks.filter((result) => !result.pass);
console.log(`${checks.length - failed.length} of ${checks.length} checks pass`);
for (const result of failed)
  console.log(`${result.name}\n  ${result.detail ?? ""}`);
process.exit(failed.length ? 1 : 0);

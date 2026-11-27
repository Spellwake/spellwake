/**
 * Standalone Playwright responsive UI qualification for the Rendering domain
 * (`ui-responsive` subdomain).
 *
 * Verifies 12 core Spellwake screens across Desktop (1440x900), Tablet (768x1024),
 * and Mobile (390x844) viewports (36 checks total):
 * - Zero uncaught page errors
 * - Zero horizontal document overflow (scrollWidth <= clientWidth)
 * - Non-empty rendered layout with expected primary landmark/heading visible
 * - Open dialogs stay within horizontal viewport bounds
 *
 * Usage: node apps/play/scripts/qualify-responsive.ts
 */
import {
  apiError,
  apiFail,
  apiSuccess,
  assertCondition,
  makeUuid,
  withPlayE2eRuntime,
  type ApiReply,
} from "./playwright-harness.ts";

const SUBJECT = makeUuid(1);
const ACCOUNT = makeUuid(2);
const SESSION_ID = makeUuid(3);
const STANDARD = { id: makeUuid(10), key: "standard", name: "Standard" };
const SEASON_ID = makeUuid(600);
const MIRA = makeUuid(12);
const PROFILE = makeUuid(25);
const MATCH_1 = makeUuid(610);
const SEAT_OWN = makeUuid(611);
const SEAT_MIRA = makeUuid(612);
const RESULT_1 = makeUuid(613);
const GOBLINS = makeUuid(40);
const GOBLINS_REV = makeUuid(60);
const BOLT = makeUuid(20);
const BOLT_PRINT = makeUuid(30);
const CLASSIC_BACK = makeUuid(53);
const CLASSIC_SLEEVE = makeUuid(54);
const IVORY_DICE = makeUuid(55);
const RECIPE = makeUuid(71);
const PACK_ASSET = makeUuid(72);
const CUSTOM_CARD_ID = makeUuid(500);
const CUSTOM_REV_ID = makeUuid(501);
const OWNER_TOKEN = "stand-in-responsive-access-token";

const APPEARANCE = {
  backId: CLASSIC_BACK,
  sleeveId: CLASSIC_SLEEVE,
  diceId: IVORY_DICE,
  playmatId: null,
};

const VIEWPORTS = [
  { name: "desktop-1440x900", width: 1440, height: 900 },
  { name: "tablet-768x1024", width: 768, height: 1024 },
  { name: "mobile-390x844", width: 390, height: 844 },
] as const;

const SCREENS = [
  {
    name: "collection-decks",
    path: "/collection/decks",
    readyText: "Goblins",
    dialogName: null,
  },
  {
    name: "collection-cards",
    path: "/collection/cards",
    readyText: "Lightning Bolt",
    dialogName: null,
  },
  {
    name: "collection-packs",
    path: "/collection/packs",
    readyText: "2 packs",
    dialogName: null,
  },
  {
    name: "open-play-decks",
    path: "/open-play/decks",
    readyText: "Create deck",
    dialogName: null,
  },
  {
    name: "open-play-custom-cards",
    path: "/open-play/custom-cards",
    readyText: "Aetherblade Vanguard",
    dialogName: null,
  },
  {
    name: "league-season",
    path: "/league/season",
    readyText: "1,620",
    dialogName: null,
  },
  {
    name: "league-standings",
    path: "/league/standings",
    readyText: "Planeswalker",
    dialogName: null,
  },
  {
    name: "league-history",
    path: "/league/history",
    readyText: "Mira",
    dialogName: null,
  },
  {
    name: "market-listings",
    path: "/market/listings",
    readyText: "Create listing",
    dialogName: null,
  },
  {
    name: "market-supply",
    path: "/market/supply",
    readyText: "420 remaining",
    dialogName: null,
  },
  {
    name: "market-predictions",
    path: "/market/predictions",
    readyText: "Predictions",
    dialogName: null,
  },
  {
    name: "account-settings-dialog",
    path: "/collection/decks?account=profile",
    readyText: "Account settings",
    dialogName: "Account settings",
  },
] as const;

const CARDS = [
  { id: BOLT, name: "Lightning Bolt", oracleRef: "bolt" },
];

const PRINTINGS = [
  {
    id: BOLT_PRINT,
    definitionId: BOLT,
    externalPrintingRef: "msc-806",
    rarity: "uncommon",
    finish: "nonfoil",
    collectorNumber: "806",
    language: "en",
    set: { id: makeUuid(31), name: "Secret Lair Commander", code: "msc" },
  },
];

const answer = (
  method: string,
  url: URL,
  body: unknown,
  bearer: string,
): ApiReply => {
  if (bearer !== OWNER_TOKEN) {
    return apiFail({ session: "unauthorized" }, 401);
  }
  const path = url.pathname;
  const command = (body ?? {}) as Record<string, unknown>;

  if (method === "GET" && path === "/v1/session") {
    return apiSuccess({
      subjectId: SUBJECT,
      sessionId: SESSION_ID,
      method: "google",
      state: "active",
      accountId: ACCOUNT,
    });
  }
  if (method === "GET" && path === "/v1/account") {
    return apiSuccess({
      accountId: ACCOUNT,
      username: "Planeswalker",
      state: "active",
      appearanceRevision: "1",
      defaults: APPEARANCE,
      wallet: { owned: "5000", held: "0", available: "5000" },
    });
  }
  if (method === "GET" && path === "/v1/account/profile") {
    return apiSuccess({
      accountId: ACCOUNT,
      cardArtProfile: null,
      supporter: false,
    });
  }
  if (method === "GET" && path === "/v1/account/patreon") {
    return apiSuccess({
      accountId: ACCOUNT,
      linked: false,
      active: false,
      supporter: false,
      tier: null,
    });
  }
  if (method === "GET" && path === "/v1/formats") {
    return apiSuccess({ items: [STANDARD], nextCursor: null });
  }
  if (method === "GET" && path === "/v1/decks") {
    return apiSuccess({
      items: [
        {
          id: GOBLINS,
          name: "Goblins",
          mode: "collection",
          currentRevisionId: GOBLINS_REV,
          format: STANDARD,
          appearance: APPEARANCE,
        },
      ],
      nextCursor: null,
    });
  }
  if (method === "GET" && path === "/v1/cosmetics") {
    return apiSuccess({ items: [], nextCursor: null });
  }
  if (method === "GET" && path === "/v1/purchases") {
    return apiSuccess({ items: [], nextCursor: null });
  }
  if (method === "GET" && path === "/v1/social/status") {
    return apiSuccess({
      accountId: ACCOUNT,
      unreadMessages: "0",
      personId: null,
      person: null,
    });
  }
  if (method === "GET" && path === "/v1/social/threads") {
    return apiSuccess({ accountId: ACCOUNT, items: [], nextCursor: null });
  }
  if (method === "GET" && path === "/v1/inventory") {
    return apiSuccess({
      items: [
        {
          id: makeUuid(80),
          assetId: makeUuid(75),
          revision: "1",
          asset: { kind: "card", printingId: BOLT_PRINT },
          owned: "3",
          held: "0",
          available: "3",
        },
        {
          id: makeUuid(82),
          assetId: PACK_ASSET,
          revision: "1",
          asset: { kind: "pack", recipeId: RECIPE },
          owned: "2",
          held: "0",
          available: "2",
        },
      ],
      nextCursor: null,
    });
  }
  if (method === "GET" && path === "/v1/sets") {
    return apiSuccess({
      items: [{ id: makeUuid(74), name: "Foundations", code: "fdn" }],
      nextCursor: null,
    });
  }
  if (method === "GET" && path === "/v1/market/listings") {
    return apiSuccess({ accountId: ACCOUNT, items: [], nextCursor: null });
  }
  if (method === "GET" && path === "/v1/packs/products") {
    return apiSuccess({
      accountId: ACCOUNT,
      items: [
        {
          id: RECIPE,
          productId: makeUuid(73),
          setId: makeUuid(74),
          name: "Foundations Play Booster",
          setName: "Foundations",
          revisionRef: "foundations-1",
        },
      ],
      nextCursor: null,
    });
  }
  if (method === "GET" && path === `/v1/packs/products/${RECIPE}/quote`) {
    return apiSuccess({
      accountId: ACCOUNT,
      quote: {
        recipeId: RECIPE,
        productId: makeUuid(73),
        policyId: makeUuid(161),
        revision: "1",
        remaining: "420",
        unitPrice: "1000",
        quantity: "1",
        total: "1000",
        purpose: "purchase",
      },
    });
  }
  if (method === "GET" && path === "/v1/predictions/markets") {
    return apiSuccess({ accountId: ACCOUNT, items: [], nextCursor: null });
  }
  if (method === "GET" && path === "/v1/predictions/orders") {
    return apiSuccess({ accountId: ACCOUNT, items: [], nextCursor: null });
  }
  if (method === "GET" && path === "/v1/predictions/positions") {
    return apiSuccess({ accountId: ACCOUNT, items: [], nextCursor: null });
  }
  if (method === "GET" && path === "/v1/custom-cards") {
    return apiSuccess({
      accountId: ACCOUNT,
      activeCount: 1,
      activeLimit: 250,
      items: [
        {
          id: CUSTOM_CARD_ID,
          archivedAt: null,
          createdAt: "2026-11-20T12:00:00Z",
          updatedAt: "2026-11-20T12:00:00Z",
          currentRevision: {
            revisionId: CUSTOM_REV_ID,
            customCardId: CUSTOM_CARD_ID,
            revisionNumber: "1",
            rulesSha256: "a".repeat(64),
            name: "Aetherblade Vanguard",
            layout: "normal",
            rarity: "rare",
            faces: [
              {
                name: "Aetherblade Vanguard",
                manaCost: "{2}{U}{R}",
                typeLine: "Creature — Human Artificer",
                oracleText: "Flying, haste",
                power: "3",
                toughness: "2",
                loyalty: null,
                defense: null,
                colorIndicator: null,
              },
            ],
            visualFrameAccents: ["auto"],
            flavorTexts: [null],
            collectorNote: null,
            stage1Parsed: true,
            stage2Playable: true,
            compilerRevision: "spellwake-custom-v1",
            art: [],
            createdAt: "2026-11-20T12:00:00Z",
          },
        },
      ],
      nextCursor: null,
    });
  }
  if (method === "POST" && path === "/v1/custom-cards/compile") {
    return apiSuccess({
      rulesSha256: "a".repeat(64),
      compilerRevision: "spellwake-custom-v1",
      stage1Parsed: true,
      stage2Playable: true,
      diagnostics: [],
      astReading: [],
      colorIdentity: ["U", "R"],
      manaValue: 4,
    });
  }
  if (method === "GET" && path === "/v1/seasons") {
    return apiSuccess({
      items: [
        {
          id: SEASON_ID,
          startAt: "2026-10-01T00:00:00Z",
          endAt: "2026-12-31T23:59:59Z",
          state: "active",
        },
      ],
      nextCursor: null,
    });
  }
  if (method === "GET" && path === "/v1/ranks") {
    return apiSuccess({
      items: [{ formatId: STANDARD.id, seasonId: SEASON_ID, rating: "1620" }],
      nextCursor: null,
    });
  }
  if (method === "GET" && path === "/v1/competitive-results") {
    return apiSuccess({ items: [], nextCursor: null });
  }
  if (method === "GET" && path === "/v1/league/leaderboard") {
    return apiSuccess({
      seasonId: SEASON_ID,
      formatId: STANDARD.id,
      offset: 0,
      total: 1,
      items: [
        {
          rankPosition: 1,
          id: makeUuid(623),
          accountId: ACCOUNT,
          username: "Planeswalker",
          formatId: STANDARD.id,
          seasonId: SEASON_ID,
          tier: "gold",
          rating: "1620",
          finalized: false,
          selectedBadge: null,
          cardArtProfile: null,
          supporter: false,
          wins: 12,
          losses: 4,
          draws: 0,
        },
      ],
    });
  }
  if (method === "GET" && path === "/v1/matches") {
    return apiSuccess({
      accountId: ACCOUNT,
      items: [
        {
          id: MATCH_1,
          formatId: STANDARD.id,
          profileId: PROFILE,
          mode: "collection",
          competitive: true,
          structure: "bo1",
          state: "completed",
          revision: "4",
          startedAt: "2026-11-20T18:00:00Z",
          currentGameId: null,
          reservedGameId: null,
          availability: "available",
          availabilityRevision: "1",
        },
      ],
      nextCursor: null,
    });
  }
  if (method === "GET" && path === `/v1/matches/${MATCH_1}`) {
    return apiSuccess({
      accountId: ACCOUNT,
      match: {
        id: MATCH_1,
        formatId: STANDARD.id,
        profileId: PROFILE,
        mode: "collection",
        competitive: true,
        structure: "bo1",
        state: "completed",
        revision: "4",
        startedAt: "2026-11-20T18:00:00Z",
        currentGameId: null,
        reservedGameId: null,
        availability: "available",
        availabilityRevision: "1",
      },
      ownSeatId: SEAT_OWN,
      result: {
        id: RESULT_1,
        outcome: "win",
        committedAt: "2026-11-20T18:22:00Z",
        rankingFormatId: STANDARD.id,
        rankSequence: "1",
        winnerSeatIds: [SEAT_OWN],
        terminalKind: "game_outcome",
      },
    });
  }
  if (method === "GET" && path === `/v1/matches/${MATCH_1}/seats`) {
    return apiSuccess({
      accountId: ACCOUNT,
      matchId: MATCH_1,
      items: [
        {
          id: SEAT_OWN,
          seatIndex: "0",
          controllerKind: "human",
          person: { id: ACCOUNT, username: "Planeswalker", isDeleted: false },
          clock: null,
        },
        {
          id: SEAT_MIRA,
          seatIndex: "1",
          controllerKind: "human",
          person: { id: MIRA, username: "Mira", isDeleted: false },
          clock: null,
        },
      ],
      nextCursor: null,
    });
  }
  if (method === "POST" && path === "/v1/catalogue/resolve") {
    const wanted = command as {
      definitionIds?: string[];
      printingIds?: string[];
    };
    return apiSuccess({
      cards: CARDS.filter((c) => (wanted.definitionIds ?? []).includes(c.id)),
      printings: PRINTINGS.filter((p) =>
        (wanted.printingIds ?? []).includes(p.id),
      ),
    });
  }
  return apiError("No such route.", 404);
};

await withPlayE2eRuntime(
  "rendering-responsive",
  { authPort: 59993, apiPort: 59994 },
  async (runtime) => {
    for (const vp of VIEWPORTS) {
      await runtime.withAuthedPage(
        {
          token: OWNER_TOKEN,
          subject: SUBJECT,
          viewport: { width: vp.width, height: vp.height },
          answer,
        },
        async ({ page }) => {
          const pageErrors: string[] = [];
          page.on("pageerror", (err) => pageErrors.push(err.message));

          for (const screen of SCREENS) {
            await runtime.runCheck(
              `${screen.name} @ ${vp.name}: renders cleanly with zero horizontal overflow`,
              async () => {
                const errorsBefore = pageErrors.length;
                await page.goto(`${runtime.origin}${screen.path}`);
                await page.getByText(screen.readyText).first().waitFor();
                const metrics = await page.evaluate(() => ({
                  scrollWidth: document.documentElement.scrollWidth,
                  clientWidth: document.documentElement.clientWidth,
                  textLength: document.body.innerText.trim().length,
                }));
                assertCondition(
                  pageErrors.length === errorsBefore,
                  `Uncaught page errors: ${pageErrors.slice(errorsBefore).join("; ")}`,
                );
                assertCondition(
                  metrics.textLength > 0,
                  "Screen rendered empty body",
                );
                assertCondition(
                  metrics.scrollWidth <= metrics.clientWidth,
                  `Horizontal overflow on ${screen.name} (${vp.name}): scrollWidth=${metrics.scrollWidth} > clientWidth=${metrics.clientWidth}`,
                );
                if (screen.dialogName) {
                  const box = await page
                    .getByRole("dialog", { name: screen.dialogName })
                    .boundingBox();
                  assertCondition(
                    box !== null &&
                      box.x >= -1 &&
                      box.x + box.width <= vp.width + 1,
                    `Dialog clipped horizontally at ${vp.name}: ${JSON.stringify(box)}`,
                  );
                }
              },
            );
          }
        },
      );
    }

    runtime.finish();
  },
);

/**
 * Standalone Playwright browser E2E qualification for the Predictions domain.
 *
 * Uses the shared loopback Playwright harness (`playwright-harness.ts`) with
 * canonical prediction market, order, and position wire payloads.
 *
 * Covers 5 scenarios across desktop (1440x900) and mobile (390x844):
 * 1. Explore board renders open and resolved predictions with catalogue printing names.
 * 2. My predictions tab shows the account's matched positions accurately.
 * 3. Resolved prediction opens read-only detail dialog with outcome and closes cleanly.
 * 4. Failed market read surfaces "Could not load predictions" honestly.
 * 5. Mobile (390x844) Predictions page renders without horizontal overflow.
 *
 * Usage: node apps/play/scripts/qualify-predictions.ts
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
const BOLT = makeUuid(20);
const GUIDE = makeUuid(22);
const BOLT_PRINT = makeUuid(30);
const GUIDE_PRINT = makeUuid(32);
const CLASSIC_BACK = makeUuid(53);
const CLASSIC_SLEEVE = makeUuid(54);
const IVORY_DICE = makeUuid(55);
const OPEN_MARKET = makeUuid(401);
const RESOLVED_MARKET = makeUuid(402);
const UNBET_MARKET = makeUuid(403);
const ORDER_ID = makeUuid(410);
const POSITION_ID = makeUuid(411);
const OWNER_TOKEN = "stand-in-predictions-access-token";

const APPEARANCE = {
  backId: CLASSIC_BACK,
  sleeveId: CLASSIC_SLEEVE,
  diceId: IVORY_DICE,
  playmatId: null,
};

const OPEN_TERMS = {
  id: OPEN_MARKET,
  printingId: BOLT_PRINT,
  priceMetric: "scryfall:usd",
  targetUsd: "10.00",
  condition: "reached_by",
  deadline: "2030-01-03T13:00:00Z",
  tradingClosesAt: "2030-01-01T00:00:00Z",
  observationStartsAt: "2030-01-02T12:00:00Z",
  samplingPolicy: "scryfall-daily-12-13-utc-v1",
  terminalSlot: "2030-01-03",
  rulesRevision: "scryfall-sampled-gte-v1",
  payoutCoins: "100",
  state: "open",
  acceptingOrders: true,
  outcome: null,
  resolvedAt: null,
};

const UNBET_TERMS = {
  ...OPEN_TERMS,
  id: UNBET_MARKET,
  targetUsd: "15.00",
};

const RESOLVED_TERMS = {
  id: RESOLVED_MARKET,
  printingId: GUIDE_PRINT,
  priceMetric: "scryfall:usd",
  targetUsd: "5.00",
  condition: "reached_by",
  deadline: "2026-10-03T13:00:00Z",
  tradingClosesAt: "2026-10-01T00:00:00Z",
  observationStartsAt: "2026-10-02T12:00:00Z",
  samplingPolicy: "scryfall-daily-12-13-utc-v1",
  terminalSlot: "2026-10-03",
  rulesRevision: "scryfall-sampled-gte-v1",
  payoutCoins: "100",
  state: "resolved",
  acceptingOrders: false,
  outcome: "yes",
  resolvedAt: "2026-10-03T13:05:00Z",
};

const CARDS = [
  { id: BOLT, name: "Lightning Bolt", oracleRef: "bolt" },
  { id: GUIDE, name: "Goblin Guide", oracleRef: "guide" },
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
  {
    id: GUIDE_PRINT,
    definitionId: GUIDE,
    externalPrintingRef: "zen-126",
    rarity: "rare",
    finish: "foil",
    collectorNumber: "126",
    language: "en",
    set: { id: makeUuid(33), name: "Zendikar", code: "zen" },
  },
];

await withPlayE2eRuntime(
  "predictions",
  { authPort: 59987, apiPort: 59988 },
  async (runtime) => {
    const withPredictionScenario = async (
      options: {
        failMarketsRead?: boolean;
        viewport?: { width: number; height: number };
      },
      fn: (harness: {
        page: Awaited<ReturnType<typeof runtime.browser.newPage>>;
      }) => Promise<void>,
    ): Promise<void> => {
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
        if (method === "GET" && path === "/v1/cosmetics") {
          return apiSuccess({ items: [], nextCursor: null });
        }
        if (method === "GET" && path === "/v1/purchases") {
          return apiSuccess({ items: [], nextCursor: null });
        }
        if (method === "GET" && path === "/v1/decks") {
          return apiSuccess({ items: [], nextCursor: null });
        }
        if (method === "POST" && path === "/v1/catalogue/resolve") {
          const wanted = command as {
            definitionIds?: string[];
            printingIds?: string[];
          };
          const defIds = wanted.definitionIds ?? [];
          const printIds = wanted.printingIds ?? [];
          return apiSuccess({
            cards: CARDS.filter((card) => defIds.includes(card.id)),
            printings: PRINTINGS.filter((printing) =>
              printIds.includes(printing.id),
            ),
          });
        }
        if (method === "GET" && path === "/v1/predictions/markets") {
          if (options.failMarketsRead) {
            return apiError("Simulated predictions outage.", 503);
          }
          return apiSuccess({
            accountId: ACCOUNT,
            items: [OPEN_TERMS, RESOLVED_TERMS, UNBET_TERMS],
            nextCursor: null,
          });
        }
        if (method === "GET" && path === `/v1/predictions/markets/${OPEN_MARKET}`) {
          return apiSuccess({
            accountId: ACCOUNT,
            market: {
              terms: OPEN_TERMS,
              book: [{ side: "yes", limitCost: "60", quantity: "50" }],
            },
          });
        }
        if (method === "GET" && path === `/v1/predictions/markets/${RESOLVED_MARKET}`) {
          return apiSuccess({
            accountId: ACCOUNT,
            market: {
              terms: RESOLVED_TERMS,
              book: [],
            },
          });
        }
        if (method === "GET" && path === "/v1/predictions/orders") {
          return apiSuccess({
            accountId: ACCOUNT,
            items: [
              {
                id: ORDER_ID,
                marketId: OPEN_MARKET,
                side: "yes",
                limitCost: "60",
                quantity: "3",
                filledQuantity: "2",
                actualFilledCost: "120",
                unfilledReservation: "60",
                state: "open",
                acceptedSequence: "1",
                acceptedAt: "2026-11-02T12:00:00Z",
              },
            ],
            nextCursor: null,
          });
        }
        if (method === "GET" && path === "/v1/predictions/positions") {
          return apiSuccess({
            accountId: ACCOUNT,
            items: [
              {
                id: POSITION_ID,
                marketId: OPEN_MARKET,
                orderId: ORDER_ID,
                side: "yes",
                quantity: "2",
                actualCost: "120",
                fixedPayout: "100",
                matchedAt: "2026-11-02T12:01:00Z",
              },
            ],
            nextCursor: null,
          });
        }
        return apiError("No such route.", 404);
      };

      await runtime.withAuthedPage(
        {
          token: OWNER_TOKEN,
          subject: SUBJECT,
          viewport: options.viewport,
          answer,
        },
        async ({ page }) => {
          await fn({ page });
        },
      );
    };

    await runtime.runCheck(
      "Explore board renders open and resolved predictions with catalogue printing names",
      async () => {
        await withPredictionScenario({}, async ({ page }) => {
          await page.goto(`${runtime.origin}/market/predictions`);
          await page.getByText("Lightning Bolt").first().waitFor();
          await page.getByText("Goblin Guide").first().waitFor();
          assertCondition(
            (await page
              .getByRole("button", { name: /^Make a prediction:.*\$15/ })
              .count()) >= 1,
            "Expected unbet open $15 Lightning Bolt 'Make a prediction' action",
          );
          assertCondition(
            (await page
              .getByRole("button", { name: /^View prediction:.*\$5/ })
              .count()) >= 1,
            "Expected resolved $5 Goblin Guide read-only prediction action",
          );
        });
      },
    );

    await runtime.runCheck(
      "My predictions tab displays the account's matched Yes position",
      async () => {
        await withPredictionScenario({}, async ({ page }) => {
          await page.goto(`${runtime.origin}/market/predictions?tab=mine`);
          await page.getByText("Lightning Bolt").first().waitFor();
          await page.getByText("Yes", { exact: true }).first().waitFor();
        });
      },
    );

    await runtime.runCheck(
      "View prediction opens read-only dialog for resolved market and closes cleanly",
      async () => {
        await withPredictionScenario({}, async ({ page }) => {
          await page.goto(`${runtime.origin}/market/predictions`);
          const viewBtn = page
            .getByRole("button", { name: /^View prediction:.*\$5/ })
            .first();
          await viewBtn.waitFor();
          await viewBtn.click();
          const dialog = page.getByRole("dialog");
          await dialog.waitFor();
          await dialog.getByText(/Goblin Guide|Will.*\$5/).first().waitFor();
          await page.keyboard.press("Escape");
          await dialog.waitFor({ state: "hidden" });
        });
      },
    );

    await runtime.runCheck(
      "failed markets read surfaces error notice honestly",
      async () => {
        await withPredictionScenario(
          { failMarketsRead: true },
          async ({ page }) => {
            await page.goto(`${runtime.origin}/market/predictions`);
            await page
              .getByText("Could not load predictions")
              .first()
              .waitFor();
          },
        );
      },
    );

    await runtime.runCheck(
      "mobile viewport (390x844) renders Predictions page without horizontal overflow",
      async () => {
        await withPredictionScenario(
          { viewport: { width: 390, height: 844 } },
          async ({ page }) => {
            await page.goto(`${runtime.origin}/market/predictions`);
            await page.getByText("Lightning Bolt").first().waitFor();
            const metrics = await page.evaluate(() => ({
              scrollWidth: document.documentElement.scrollWidth,
              clientWidth: document.documentElement.clientWidth,
            }));
            assertCondition(
              metrics.scrollWidth <= metrics.clientWidth,
              `Mobile viewport overflow detected: scrollWidth=${metrics.scrollWidth} > clientWidth=${metrics.clientWidth}`,
            );
          },
        );
      },
    );

    runtime.finish();
  },
);

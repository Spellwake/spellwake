/**
 * Standalone Playwright browser E2E qualification for the Market domain
 * (Listings, Pack Supply Shop, and Direct Trade).
 *
 * Uses the shared loopback Playwright harness (`playwright-harness.ts`) with
 * in-memory stand-in Economy, Market, Pack Supply, Inventory, and Catalogue APIs.
 *
 * Covers 6 scenarios across desktop (1440x900) and mobile (390x844):
 * 1. Market listing Buy recovers cleanly from a dropped reply via Check status without double-posting.
 * 2. Create listing appears under Only my listings, and Cancel listing confirms and removes it.
 * 3. Pack Supply dynamic quote (2 packs = 2003 coins) recovers cleanly from dropped reply via Check status.
 * 4. Unavailable connected Trade room shows Back only and sends zero economy commands.
 * 5. Mobile (390x844) Market Listings layout renders without horizontal overflow.
 * 6. Mobile (390x844) Pack Supply layout renders without horizontal overflow.
 *
 * Usage: node apps/play/scripts/qualify-market.ts
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
const PACK_ASSET = makeUuid(70);
const RECIPE = makeUuid(71);
const MIRA = makeUuid(95);
const ASH = makeUuid(140);
const OWNER_TOKEN = "stand-in-market-access-token";

const APPEARANCE = {
  backId: CLASSIC_BACK,
  sleeveId: CLASSIC_SLEEVE,
  diceId: IVORY_DICE,
  playmatId: null,
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
    rarity: "uncommon",
    finish: "foil",
    collectorNumber: "126",
    language: "en",
    set: { id: makeUuid(33), name: "Zendikar", code: "zen" },
  },
];

type MarketLot = {
  id: string;
  seller: { id: string; username: string; isDeleted: boolean };
  assetId: string;
  asset: Record<string, string>;
  quantity: string;
  totalPrice: string;
  revision: string;
};

const inventoryItem = (
  n: number,
  assetId: string,
  owned: number,
  asset: Record<string, string>,
) => ({
  id: makeUuid(n),
  assetId,
  revision: "1",
  owned: String(owned),
  held: "0",
  available: String(owned),
  asset,
});

await withPlayE2eRuntime(
  "market",
  { authPort: 59989, apiPort: 59990 },
  async (runtime) => {
    const withMarketScenario = async (
      options: { viewport?: { width: number; height: number } },
      fn: (harness: {
        page: Awaited<ReturnType<typeof runtime.browser.newPage>>;
        sent: (path: string) => Record<string, unknown>[];
        setFault: (opts: { loseNext?: boolean; holdDisposition?: boolean }) => void;
      }) => Promise<void>,
    ): Promise<void> => {
      const holdings = { bolt: 3, guide: 1, packs: 2 };
      let packsSold = 0;
      const PACK_STOCK = 420;
      const packTotal = (qty: number) =>
        qty * 1000 + 3 * (qty * packsSold + (qty * (qty - 1)) / 2);
      let nextLot = 150;
      const marketLots: MarketLot[] = [
        {
          id: makeUuid(141),
          seller: { id: MIRA, username: "Mira", isDeleted: false },
          assetId: makeUuid(142),
          asset: { kind: "card", printingId: BOLT_PRINT },
          quantity: "4",
          totalPrice: "500",
          revision: "1",
        },
        {
          id: makeUuid(143),
          seller: { id: ASH, username: "Ash", isDeleted: false },
          assetId: makeUuid(144),
          asset: { kind: "pack", recipeId: RECIPE },
          quantity: "2",
          totalPrice: "6000",
          revision: "1",
        },
      ];
      const outcomes = new Map<string, unknown>();
      let loseNextMarketReply = false;
      let holdMarketDisposition = false;

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
            wallet: { owned: "14500", held: "0", available: "14500" },
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
        if (method === "GET" && path === "/v1/inventory") {
          return apiSuccess({
            items: [
              inventoryItem(80, makeUuid(75), holdings.bolt, {
                kind: "card",
                printingId: BOLT_PRINT,
              }),
              inventoryItem(81, makeUuid(76), holdings.guide, {
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
            ],
            nextCursor: null,
          });
        }
        if (method === "GET" && path === "/v1/sets") {
          return apiSuccess({
            items: [
              { id: makeUuid(74), name: "Foundations", code: "fdn" },
              { id: makeUuid(160), name: "15th Anniversary Cards", code: "p15a" },
            ],
            nextCursor: null,
          });
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
        if (method === "GET" && path === `/v1/packs/products/${RECIPE}/quote`) {
          const quantity = Number(url.searchParams.get("quantity") ?? "1");
          const remaining = PACK_STOCK - packsSold;
          const enough = remaining >= quantity;
          return apiSuccess({
            accountId: ACCOUNT,
            quote: {
              recipeId: RECIPE,
              productId: makeUuid(73),
              policyId: makeUuid(161),
              revision: String(packsSold + 1),
              remaining: String(remaining),
              unitPrice: String(1000 + 3 * packsSold),
              quantity: String(quantity),
              total: enough ? String(packTotal(quantity)) : null,
              purpose: enough ? "purchase" : "valuation_only",
            },
          });
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
        if (method === "POST" && path === "/v1/economy/commands/disposition") {
          const inner = (command["command"] ?? {}) as { requestId?: string };
          const known = holdMarketDisposition
            ? undefined
            : outcomes.get(inner.requestId ?? "");
          return apiSuccess(
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
            owned ? lot.seller.id === ACCOUNT : lot.seller.id !== ACCOUNT,
          );
          return apiSuccess({
            accountId: ACCOUNT,
            items: [...lots]
              .sort((a, b) => a.id.localeCompare(b.id))
              .map((lot) => ({
                ...lot,
                state: "listed",
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
          const reply =
            inner["recipeId"] !== RECIPE ||
            inner["expectedRevision"] !== String(packsSold + 1) ||
            inner["expectedTotal"] !== String(packTotal(quantity))
              ? apiFail({ quote: "stock or price changed; request a new quote" })
              : (() => {
                  packsSold += quantity;
                  holdings.packs += quantity;
                  return apiSuccess({ resourceId: makeUuid(162) });
                })();
          outcomes.set(inner["requestId"] ?? "", reply.body);
          if (loseNextMarketReply) {
            loseNextMarketReply = false;
            return apiError("Lost.", 503);
          }
          return reply;
        }
        if (
          method === "POST" &&
          path === "/v1/economy/commands" &&
          String(command["kind"]).startsWith("listing_")
        ) {
          const inner = command["command"] as Record<string, unknown> & {
            requestId: string;
          };
          const at = marketLots.findIndex(
            (lot) => lot.id === inner["listingId"],
          );
          let reply: ApiReply;
          if (command["kind"] === "listing_buy") {
            const lot = marketLots[at];
            if (
              !lot ||
              lot.revision !== inner["expectedRevision"] ||
              lot.totalPrice !== inner["expectedTotal"]
            ) {
              reply = apiFail({ listing: "listing terms or state changed" });
            } else {
              marketLots.splice(at, 1);
              reply = apiSuccess({ resourceId: lot.id });
            }
          } else if (command["kind"] === "listing_cancel") {
            const lot = marketLots[at];
            if (!lot || lot.seller.id !== ACCOUNT) {
              reply = apiFail({ listing: "listing unavailable" });
            } else {
              marketLots.splice(at, 1);
              reply = apiSuccess({ resourceId: lot.id });
            }
          } else {
            const lot = inner["lot"] as { assetId: string; quantity: string };
            const listingId = makeUuid(nextLot++);
            marketLots.push({
              id: listingId,
              seller: {
                id: ACCOUNT,
                username: "Planeswalker",
                isDeleted: false,
              },
              assetId: lot.assetId,
              asset:
                lot.assetId === PACK_ASSET
                  ? { kind: "pack", recipeId: RECIPE }
                  : {
                      kind: "card",
                      printingId:
                        lot.assetId === makeUuid(76) ? GUIDE_PRINT : BOLT_PRINT,
                    },
              quantity: lot.quantity,
              totalPrice: String(inner["totalPrice"]),
              revision: "1",
            });
            reply = apiSuccess({ resourceId: listingId });
          }
          outcomes.set(inner.requestId, reply.body);
          if (loseNextMarketReply) {
            loseNextMarketReply = false;
            return apiError("Lost.", 503);
          }
          return reply;
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
        async ({ page, sent }) => {
          await fn({
            page,
            sent,
            setFault: ({ loseNext, holdDisposition }) => {
              if (loseNext !== undefined) loseNextMarketReply = loseNext;
              if (holdDisposition !== undefined) {
                holdMarketDisposition = holdDisposition;
              }
            },
          });
        },
      );
    };

    await runtime.runCheck(
      "listing Buy recovers from dropped reply via Check status without re-submitting command",
      async () => {
        await withMarketScenario({}, async ({ page, sent, setFault }) => {
          await page.goto(`${runtime.origin}/market/listings`);
          await page.getByText("Seller: Mira").waitFor();
          const before = sent("/v1/economy/commands").length;
          setFault({ loseNext: true, holdDisposition: true });
          await page.getByRole("button", { name: "Buy", exact: true }).click();
          const dialog = page.getByRole("dialog", { name: "Buy listing" });
          await dialog
            .getByRole("button", { name: "Buy", exact: true })
            .click();
          await dialog.getByText("Purchase not confirmed.").waitFor();
          assertCondition(
            await dialog.getByRole("button", { name: "Cancel" }).isDisabled(),
            "Cancel must stay disabled while disposition is unresolved",
          );
          setFault({ holdDisposition: false });
          await dialog.getByRole("button", { name: "Check status" }).click();
          await page
            .getByText("Purchased 4 copies of Lightning Bolt.")
            .waitFor();
          assertCondition(
            sent("/v1/economy/commands").length === before + 1,
            "Recovery must not dispatch a second listing_buy command",
          );
        });
      },
    );

    await runtime.runCheck(
      "Create listing appears under Only my listings and Cancel listing removes it after confirmation",
      async () => {
        await withMarketScenario({}, async ({ page }) => {
          await page.goto(`${runtime.origin}/market/listings`);
          await page.getByText("Seller: Mira").waitFor();
          await page.getByRole("button", { name: "Create listing" }).click();
          const create = page.getByRole("dialog", { name: "Create listing" });
          await create.locator("label:has(input[type=radio])").first().click();
          await create.getByRole("button", { name: "Next" }).click();
          await create
            .getByRole("textbox", { name: "Total price" })
            .fill("300");
          await create
            .getByRole("button", { name: "Create listing", exact: true })
            .click();
          await page.getByText("Listing created.").waitFor();
          assertCondition(
            await page
              .getByRole("checkbox", { name: "Only my listings" })
              .isChecked(),
            "Only my listings filter should activate after creating a listing",
          );
          await page.getByText("Seller: You").waitFor();
          await page.getByRole("button", { name: "Cancel listing" }).click();
          const confirm = page.getByRole("dialog", {
            name: "Cancel listing?",
          });
          await confirm
            .getByRole("button", { name: "Cancel listing" })
            .click();
          await page.getByText("Listing cancelled.").waitFor();
          await page.getByText("No active listings.").waitFor();
        });
      },
    );

    await runtime.runCheck(
      "Pack Supply dynamic quote buys 2 packs for 2003 coins and recovers from dropped reply via Check status",
      async () => {
        await withMarketScenario({}, async ({ page, sent, setFault }) => {
          await page.goto(`${runtime.origin}/market/supply`);
          await page.getByText("420 remaining").waitFor();
          const before = sent("/v1/economy/commands").length;
          await page
            .getByRole("button", { name: "Buy Foundations Play Booster" })
            .click();
          const dialog = page.getByRole("dialog", { name: "Buy packs" });
          await dialog
            .getByRole("button", { name: "Increase Quantity" })
            .click();
          await dialog.getByText("2003", { exact: true }).waitFor();
          setFault({ loseNext: true, holdDisposition: true });
          await dialog
            .getByRole("button", { name: "Buy", exact: true })
            .click();
          await dialog.getByText("Purchase not confirmed.").waitFor();
          setFault({ holdDisposition: false });
          await dialog.getByRole("button", { name: "Check status" }).click();
          await page.getByText("Bought 2 Foundations packs.").waitFor();
          const buys = sent("/v1/economy/commands").slice(before);
          assertCondition(
            buys.length === 1 &&
              JSON.stringify(buys[0]).includes('"expectedTotal":"2003"'),
            `Expected single pack_buy command for 2003 coins, got: ${JSON.stringify(buys)}`,
          );
        });
      },
    );

    await runtime.runCheck(
      "unavailable direct trade room renders Back only and dispatches zero economy commands",
      async () => {
        await withMarketScenario({}, async ({ page, sent }) => {
          const before = sent("/v1/economy/commands").length;
          await page.goto(`${runtime.origin}/collection/trade/mira`);
          await page.getByRole("button", { name: "Back" }).first().waitFor();
          assertCondition(
            (await page.getByRole("button", { name: "Ready" }).count()) === 0,
            "Unavailable trade room must not expose Ready button",
          );
          assertCondition(
            sent("/v1/economy/commands").length === before,
            "Unavailable trade room must not send economy commands",
          );
        });
      },
    );

    await runtime.runCheck(
      "mobile viewport (390x844) renders Market Listings without horizontal overflow",
      async () => {
        await withMarketScenario(
          { viewport: { width: 390, height: 844 } },
          async ({ page }) => {
            await page.goto(`${runtime.origin}/market/listings`);
            await page.getByText("Lightning Bolt").first().waitFor();
            const metrics = await page.evaluate(() => ({
              scrollWidth: document.documentElement.scrollWidth,
              clientWidth: document.documentElement.clientWidth,
            }));
            assertCondition(
              metrics.scrollWidth <= metrics.clientWidth,
              `Mobile Listings overflow: scrollWidth=${metrics.scrollWidth} > clientWidth=${metrics.clientWidth}`,
            );
          },
        );
      },
    );

    await runtime.runCheck(
      "mobile viewport (390x844) renders Pack Supply without horizontal overflow",
      async () => {
        await withMarketScenario(
          { viewport: { width: 390, height: 844 } },
          async ({ page }) => {
            await page.goto(`${runtime.origin}/market/supply`);
            await page.getByText("420 remaining").waitFor();
            const metrics = await page.evaluate(() => ({
              scrollWidth: document.documentElement.scrollWidth,
              clientWidth: document.documentElement.clientWidth,
            }));
            assertCondition(
              metrics.scrollWidth <= metrics.clientWidth,
              `Mobile Supply overflow: scrollWidth=${metrics.scrollWidth} > clientWidth=${metrics.clientWidth}`,
            );
          },
        );
      },
    );

    runtime.finish();
  },
);

/**
 * Standalone Playwright browser E2E qualification for the Stripe domain.
 *
 * Uses the shared loopback Playwright harness (`playwright-harness.ts`) with
 * in-memory stand-in Auth, Application API, Stripe Checkout, and Stripe
 * Customer Portal endpoints (never contacting live Stripe or moving money).
 *
 * Covers 7 scenarios across desktop (1440x900) and mobile (390x844):
 * 1. Empty purchase history shows "No purchases yet".
 * 2. Failed purchase history read surfaces "Couldn’t load purchases." and never claims empty.
 * 3. Multi-status purchase ledger renders Paid, Refunded, and Reversed rows with catalogue names and currencies.
 * 4. Billing portal "no_customer" fail shows "No billing history yet." without opening a popup.
 * 5. Billing portal happy path sends returnUrl and opens hosted Stripe billing portal in a new tab.
 * 6. Cosmetic Shop Buy reserves checkout, opens hosted Stripe checkout, transitions to Paid, and marks Owned.
 * 7. Mobile (390x844) Billing & purchases view renders without horizontal overflow.
 *
 * Usage: node apps/play/scripts/qualify-stripe.ts
 */
import {
  apiAccepted,
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
const MIDNIGHT_SLEEVE = makeUuid(50);
const CLASSIC_BACK = makeUuid(53);
const CLASSIC_SLEEVE = makeUuid(54);
const IVORY_DICE = makeUuid(55);
const EMBER_SLEEVE = makeUuid(56);
const UNKNOWN_COSMETIC = makeUuid(59);
const OWNER_TOKEN = "stand-in-stripe-access-token";
const CHECKOUT_URL = "https://checkout.stripe.com/c/pay/stand-in-ember";
const PORTAL_URL = "https://billing.stripe.com/p/session/stand-in-portal";

const APPEARANCE = {
  backId: CLASSIC_BACK,
  sleeveId: CLASSIC_SLEEVE,
  diceId: IVORY_DICE,
  playmatId: null,
};

type PurchaseRecord = {
  purchaseId: string;
  cosmeticId: string;
  createdAt: string;
  state: "pending" | "paid" | "canceled" | "refunded" | "reversed";
  revision: string;
  price: { currency: string; minorUnits: string };
  checkout:
    | { status: "ready"; expiresAt: string; url: string }
    | { status: "unavailable" };
};

type PortalMode = "ready" | "no_customer" | "error";

await withPlayE2eRuntime(
  "stripe",
  { authPort: 59983, apiPort: 59984 },
  async (runtime) => {
    const withStripeScenario = async (
      options: {
        purchases?: PurchaseRecord[];
        failPurchasesRead?: boolean;
        portalMode?: PortalMode;
        viewport?: { width: number; height: number };
      },
      fn: (harness: {
        page: Awaited<ReturnType<typeof runtime.browser.newPage>>;
        sent: (path: string) => Record<string, unknown>[];
        checkoutVisits: URL[];
        portalVisits: URL[];
        markCheckoutPaid: (purchaseId: string) => void;
      }) => Promise<void>,
    ): Promise<void> => {
      const checkouts = new Map<string, PurchaseRecord>();
      const historyItems: PurchaseRecord[] = [...(options.purchases ?? [])];
      const checkoutVisits: URL[] = [];
      const portalVisits: URL[] = [];
      const portalMode: PortalMode = options.portalMode ?? "ready";
      let isEmberOwned = false;

      const cosmetic = (
        cid: string,
        name: string,
        kind: "back" | "sleeve" | "dice",
        isOwned: boolean,
        isFree = false,
      ) => ({
        id: cid,
        name,
        kind,
        isFree,
        isOwned,
        includedWithPatreon: false,
      });

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
            wallet: { owned: "0", held: "0", available: "0" },
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
        if (method === "GET" && path === "/v1/decks") {
          return apiSuccess({ items: [], nextCursor: null });
        }
        if (method === "GET" && path === "/v1/formats") {
          return apiSuccess({ items: [STANDARD] });
        }
        if (method === "GET" && path === "/v1/cosmetics") {
          return apiSuccess({
            items: [
              cosmetic(MIDNIGHT_SLEEVE, "Midnight sleeve", "sleeve", true),
              cosmetic(CLASSIC_BACK, "Classic back", "back", true, true),
              cosmetic(CLASSIC_SLEEVE, "Classic sleeve", "sleeve", true, true),
              cosmetic(IVORY_DICE, "Ivory dice", "dice", true, true),
              cosmetic(EMBER_SLEEVE, "Ember sleeve", "sleeve", isEmberOwned),
            ],
            nextCursor: null,
          });
        }
        if (method === "GET" && path === "/v1/cosmetics/catalogue") {
          return apiSuccess({
            items: [
              {
                id: MIDNIGHT_SLEEVE,
                kind: "sleeve",
                name: "Midnight sleeve",
                description: "Midnight sleeve",
                previewUrl: null,
                priceCents: 299,
                owned: true,
                purchasable: false,
              },
              {
                id: EMBER_SLEEVE,
                kind: "sleeve",
                name: "Ember sleeve",
                description: "Ember sleeve",
                previewUrl: null,
                priceCents: 299,
                owned: isEmberOwned,
                purchasable: !isEmberOwned,
              },
            ],
            nextCursor: null,
          });
        }
        if (method === "GET" && path === "/v1/purchases") {
          if (options.failPurchasesRead) {
            return apiError("Simulated purchase read outage.", 503);
          }
          return apiSuccess({
            items: historyItems,
            nextCursor: null,
          });
        }
        if (method === "POST" && path === "/v1/purchases") {
          const requestId = String(command["requestId"] ?? "");
          const cosmeticId = String(command["cosmeticId"] ?? "");
          if (cosmeticId !== EMBER_SLEEVE) {
            return apiFail({ cosmetic: "unavailable" });
          }
          const existing = checkouts.get(requestId);
          if (existing) {
            return apiAccepted({ resourceId: existing.purchaseId });
          }
          const created: PurchaseRecord = {
            purchaseId: makeUuid(90 + checkouts.size),
            cosmeticId: EMBER_SLEEVE,
            createdAt: "2026-11-27T12:00:00Z",
            state: "pending",
            revision: "1",
            price: { currency: "usd", minorUnits: "299" },
            checkout: {
              status: "ready",
              expiresAt: "2030-01-01T00:00:00Z",
              url: CHECKOUT_URL,
            },
          };
          checkouts.set(requestId, created);
          historyItems.unshift(created);
          return apiAccepted({ resourceId: created.purchaseId });
        }
        if (method === "GET" && path.startsWith("/v1/purchases/")) {
          const purchaseId = path.slice("/v1/purchases/".length);
          const found = [...checkouts.values()].find(
            (p) => p.purchaseId === purchaseId,
          );
          if (!found) {
            return apiFail({ purchase: "unavailable" }, 404);
          }
          return apiSuccess(found);
        }
        if (method === "POST" && path === "/v1/billing/portal") {
          if (portalMode === "no_customer") {
            return apiFail({ billing: "no_customer" }, 404);
          }
          if (portalMode === "error") {
            return apiError("Stripe portal unavailable.", 503);
          }
          return apiSuccess({ url: PORTAL_URL });
        }
        return apiError("No such route.", 404);
      };

      const markCheckoutPaid = (purchaseId: string): void => {
        for (const rec of checkouts.values()) {
          if (rec.purchaseId === purchaseId) {
            rec.state = "paid";
            rec.revision = "2";
            rec.checkout = { status: "unavailable" };
            isEmberOwned = true;
          }
        }
      };

      await runtime.withAuthedPage(
        {
          token: OWNER_TOKEN,
          subject: SUBJECT,
          viewport: options.viewport,
          answer,
          extraRoutes: async (context) => {
            await context.route("https://checkout.stripe.com/**", (route) => {
              checkoutVisits.push(new URL(route.request().url()));
              return route.fulfill({
                contentType: "text/html",
                body: "<!doctype html><title>Stand-in Stripe Checkout</title><p>Stand-in Stripe Checkout</p>",
              });
            });
            await context.route("https://billing.stripe.com/**", (route) => {
              portalVisits.push(new URL(route.request().url()));
              return route.fulfill({
                contentType: "text/html",
                body: "<!doctype html><title>Stand-in Billing Portal</title><p>Stand-in Billing Portal</p>",
              });
            });
          },
        },
        async ({ page, sent }) => {
          await fn({
            page,
            sent,
            checkoutVisits,
            portalVisits,
            markCheckoutPaid,
          });
        },
      );
    };

    await runtime.runCheck(
      "empty purchase history renders 'No purchases yet'",
      async () => {
        await withStripeScenario({ purchases: [] }, async ({ page }) => {
          await page.goto(`${runtime.origin}/collection/decks?account=billing`);
          const dialog = page.getByRole("dialog", { name: "Account settings" });
          await dialog.getByText("No purchases yet", { exact: true }).waitFor();
        });
      },
    );

    await runtime.runCheck(
      "failed purchase history read surfaces error notice and never claims 'No purchases yet'",
      async () => {
        await withStripeScenario(
          { failPurchasesRead: true },
          async ({ page }) => {
            await page.goto(
              `${runtime.origin}/collection/decks?account=billing`,
            );
            const dialog = page.getByRole("dialog", {
              name: "Account settings",
            });
            await dialog
              .getByText("Couldn’t load purchases.")
              .first()
              .waitFor();
            assertCondition(
              (await dialog.getByText("No purchases yet").count()) === 0,
              "Failed ledger read must never display 'No purchases yet'",
            );
          },
        );
      },
    );

    await runtime.runCheck(
      "purchase ledger renders Paid, Refunded, and Reversed rows with catalogue names and currency formatting",
      async () => {
        await withStripeScenario(
          {
            purchases: [
              {
                purchaseId: makeUuid(201),
                cosmeticId: MIDNIGHT_SLEEVE,
                createdAt: "2026-11-01T14:00:00Z",
                state: "paid",
                revision: "2",
                price: { currency: "usd", minorUnits: "299" },
                checkout: { status: "unavailable" },
              },
              {
                purchaseId: makeUuid(202),
                cosmeticId: UNKNOWN_COSMETIC,
                createdAt: "2026-11-03T10:00:00Z",
                state: "refunded",
                revision: "3",
                price: { currency: "usd", minorUnits: "299" },
                checkout: { status: "unavailable" },
              },
              {
                purchaseId: makeUuid(203),
                cosmeticId: MIDNIGHT_SLEEVE,
                createdAt: "2026-11-05T08:00:00Z",
                state: "reversed",
                revision: "4",
                price: { currency: "jpy", minorUnits: "500" },
                checkout: { status: "unavailable" },
              },
            ],
          },
          async ({ page }) => {
            await page.goto(
              `${runtime.origin}/collection/decks?account=billing`,
            );
            const dialog = page.getByRole("dialog", {
              name: "Account settings",
            });
            const row = (pattern: RegExp) =>
              dialog.getByRole("row", { name: pattern });
            await row(/Midnight sleeve/).first().waitFor();
            assertCondition(
              (await row(
                /Midnight sleeve Nov 1, 2026 \$2\.99 USD Paid/,
              ).count()) === 1,
              "Expected Paid Midnight sleeve USD row",
            );
            assertCondition(
              (await row(
                /Cosmetic Nov 3, 2026 \$2\.99 USD Refunded/,
              ).count()) === 1,
              "Expected Refunded fallback Cosmetic row",
            );
            assertCondition(
              (await row(
                /Midnight sleeve Nov 5, 2026 ¥500 JPY Reversed/,
              ).count()) === 1,
              "Expected Reversed JPY row",
            );
          },
        );
      },
    );

    await runtime.runCheck(
      "Manage billing surfaces 'No billing history yet.' on no_customer without opening a popup",
      async () => {
        await withStripeScenario(
          { portalMode: "no_customer" },
          async ({ page, sent, portalVisits }) => {
            await page.goto(
              `${runtime.origin}/collection/decks?account=billing`,
            );
            const dialog = page.getByRole("dialog", {
              name: "Account settings",
            });
            const manageBtn = dialog.getByRole("button", {
              name: "Manage billing",
            });
            await manageBtn.waitFor();
            await manageBtn.click();
            await dialog
              .getByText("No billing history yet.")
              .first()
              .waitFor();
            assertCondition(
              sent("/v1/billing/portal").length === 1,
              "Expected one POST /v1/billing/portal call",
            );
            assertCondition(
              portalVisits.length === 0,
              "no_customer refusal must not open Stripe billing portal",
            );
          },
        );
      },
    );

    await runtime.runCheck(
      "Manage billing posts returnUrl and opens hosted Stripe Billing Portal in a new tab",
      async () => {
        await withStripeScenario(
          { portalMode: "ready" },
          async ({ page, sent, portalVisits }) => {
            await page.goto(
              `${runtime.origin}/collection/decks?account=billing`,
            );
            const dialog = page.getByRole("dialog", {
              name: "Account settings",
            });
            const manageBtn = dialog.getByRole("button", {
              name: "Manage billing",
            });
            await manageBtn.waitFor();
            const popupPromise = page.waitForEvent("popup");
            await manageBtn.click();
            const popup = await popupPromise;
            await popup.waitForLoadState("domcontentloaded");
            const [portalBody] = sent("/v1/billing/portal");
            assertCondition(
              typeof portalBody?.["returnUrl"] === "string" &&
                String(portalBody["returnUrl"]).includes("account=billing"),
              `Expected returnUrl containing account=billing, got: ${JSON.stringify(portalBody)}`,
            );
            assertCondition(
              portalVisits.length === 1 &&
                portalVisits[0]?.href === PORTAL_URL,
              `Expected hosted portal navigation to ${PORTAL_URL}`,
            );
            await popup.close();
          },
        );
      },
    );

    await runtime.runCheck(
      "Cosmetic shop Buy reserves checkout, opens Stripe Checkout, and transitions to Owned once paid",
      async () => {
        await withStripeScenario(
          { purchases: [] },
          async ({ page, sent, checkoutVisits, markCheckoutPaid }) => {
            await page.goto(
              `${runtime.origin}/collection/decks?cosmetics=shop`,
            );
            const dialog = page.getByRole("dialog", { name: "Cosmetics" });
            await dialog.waitFor();
            await dialog
              .getByRole("button", { name: /Ember sleeve/ })
              .click();
            const buyBtn = dialog.getByRole("button", {
              name: "Buy",
              exact: true,
            });
            await buyBtn.waitFor();
            const popupPromise = page.context().waitForEvent("page");
            await buyBtn.click();
            const popup = await popupPromise;
            await popup.waitForURL(CHECKOUT_URL);
            await dialog.getByText("Checkout opened.").waitFor();
            assertCondition(
              checkoutVisits.length === 1 &&
                checkoutVisits[0]?.href === CHECKOUT_URL,
              `Expected hosted Stripe checkout visit to ${CHECKOUT_URL}`,
            );
            const [reservation] = sent("/v1/purchases");
            assertCondition(
              reservation?.["cosmeticId"] === EMBER_SLEEVE &&
                typeof reservation["requestId"] === "string",
              "Expected idempotent POST /v1/purchases reservation for Ember sleeve",
            );
            // Simulate Stripe webhook settlement completing the purchase.
            markCheckoutPaid(makeUuid(90));
            await popup.close();
            await page.bringToFront();
            await page.evaluate(() => {
              window.dispatchEvent(new Event("focus"));
            });
            await dialog.getByText("Owned", { exact: true }).waitFor();
            assertCondition(
              (await dialog
                .getByRole("button", { name: "Buy", exact: true })
                .count()) === 0,
              "Paid cosmetic must replace Buy button with Owned",
            );
          },
        );
      },
    );

    await runtime.runCheck(
      "mobile viewport (390x844) renders Billing & purchases without horizontal overflow",
      async () => {
        await withStripeScenario(
          {
            viewport: { width: 390, height: 844 },
            purchases: [
              {
                purchaseId: makeUuid(201),
                cosmeticId: MIDNIGHT_SLEEVE,
                createdAt: "2026-11-01T14:00:00Z",
                state: "paid",
                revision: "2",
                price: { currency: "usd", minorUnits: "299" },
                checkout: { status: "unavailable" },
              },
            ],
          },
          async ({ page }) => {
            await page.goto(
              `${runtime.origin}/collection/decks?account=billing`,
            );
            const dialog = page.getByRole("dialog", {
              name: "Account settings",
            });
            const manageBtn = dialog.getByRole("button", {
              name: "Manage billing",
            });
            await manageBtn.waitFor();
            const metrics = await page.evaluate(() => ({
              scrollWidth: document.documentElement.scrollWidth,
              clientWidth: document.documentElement.clientWidth,
            }));
            assertCondition(
              metrics.scrollWidth <= metrics.clientWidth,
              `Mobile viewport overflow detected: scrollWidth=${metrics.scrollWidth} > clientWidth=${metrics.clientWidth}`,
            );
            const box = await manageBtn.boundingBox();
            assertCondition(
              box !== null && box.x >= 0 && box.x + box.width <= 390,
              `Manage billing button clipped outside 390px viewport: ${JSON.stringify(box)}`,
            );
          },
        );
      },
    );

    runtime.finish();
  },
);

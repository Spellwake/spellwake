/**
 * Standalone Playwright browser E2E qualification for the Patreon domain.
 *
 * Uses the shared loopback Playwright harness (`playwright-harness.ts`) with
 * in-memory stand-in Auth, Application API, and Patreon OAuth routes.
 *
 * Usage: node apps/play/scripts/qualify-patreon.ts
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
const TIER_ID = makeUuid(77);
const OWNER_TOKEN = "stand-in-patreon-access-token";
const PATREON_CODE = "stand-in-patreon-code-0001";
const PATREON_EXPIRY = "2030-01-01T00:00:00Z";
const APPEARANCE = {
  backId: makeUuid(53),
  sleeveId: makeUuid(54),
  diceId: makeUuid(55),
  playmatId: null,
};

type PatreonFlow = {
  stateKey: string;
  accountId: string;
  isLinked: boolean;
};

type AccountPatreonState = {
  linked: boolean;
  active: boolean;
  supporter: boolean;
  tier: { id: string; name: string; rank: number } | null;
};

type PatreonRedirectMode = "approve" | "refuse";

await withPlayE2eRuntime(
  "patreon",
  { authPort: 59981, apiPort: 59982 },
  async (runtime) => {
    const unlinkedState: AccountPatreonState = {
      linked: false,
      active: false,
      supporter: false,
      tier: null,
    };

    const withPatreonScenario = async (
      options: {
        initialAccountPatreon: AccountPatreonState;
        redirectMode?: PatreonRedirectMode;
        failCompleteWithErrorAfterLinking?: boolean;
        viewport?: { width: number; height: number };
      },
      fn: (harness: {
        page: Awaited<ReturnType<typeof runtime.browser.newPage>>;
        sent: (path: string) => Record<string, unknown>[];
        patreonVisits: URL[];
      }) => Promise<void>,
    ): Promise<void> => {
      const patreonFlows = new Map<string, PatreonFlow>();
      const patreonVisits: URL[] = [];
      const accountPatreon: AccountPatreonState = {
        ...options.initialAccountPatreon,
      };
      const redirectMode: PatreonRedirectMode =
        options.redirectMode ?? "approve";
      let failCompleteOnce = Boolean(options.failCompleteWithErrorAfterLinking);

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
            supporter: accountPatreon.supporter,
          });
        }
        if (method === "GET" && path === "/v1/account/patreon") {
          return apiSuccess({
            accountId: ACCOUNT,
            linked: accountPatreon.linked,
            active: accountPatreon.active,
            supporter: accountPatreon.supporter,
            tier: accountPatreon.tier,
          });
        }
        if (method === "POST" && path.startsWith("/v1/account/patreon/")) {
          const requestId = String(command["requestId"] ?? "");
          const stateKey = String(command["stateKey"] ?? "");
          const flow = patreonFlows.get(requestId);
          const statusPayload = () => {
            const known =
              flow?.stateKey === stateKey && flow.accountId === ACCOUNT
                ? flow
                : null;
            return apiSuccess({
              accountId: ACCOUNT,
              requestId,
              phase: known
                ? known.isLinked
                  ? "linked"
                  : "issued"
                : "unknown",
              expiresAt: known ? PATREON_EXPIRY : null,
              associationId: known?.isLinked ? makeUuid(80) : null,
            });
          };
          if (path.endsWith("/start")) {
            patreonFlows.set(requestId, {
              stateKey,
              accountId: ACCOUNT,
              isLinked: false,
            });
            const authorization = new URL(
              "https://www.patreon.com/oauth2/authorize",
            );
            authorization.search = new URLSearchParams({
              client_id: "stand_in_client",
              redirect_uri: "https://app.spellwake.example/play/patreon/return",
              response_type: "code",
              scope: "identity",
              state: stateKey,
            }).toString();
            return apiAccepted({
              accountId: ACCOUNT,
              requestId,
              expiresAt: PATREON_EXPIRY,
              authorizationUrl: authorization.href,
            });
          }
          if (path.endsWith("/complete")) {
            if (
              !flow ||
              flow.stateKey !== stateKey ||
              flow.accountId !== ACCOUNT ||
              command["code"] !== PATREON_CODE
            ) {
              return apiFail({ request: "expired" });
            }
            flow.isLinked = true;
            accountPatreon.linked = true;
            accountPatreon.active = true;
            accountPatreon.supporter = true;
            accountPatreon.tier = { id: TIER_ID, name: "Supporter", rank: 1 };
            if (failCompleteOnce) {
              failCompleteOnce = false;
              return apiError(
                "Simulated dropped completion acknowledgment.",
                503,
              );
            }
            return statusPayload();
          }
          if (path.endsWith("/status")) {
            return statusPayload();
          }
        }
        if (method === "GET" && path === "/v1/decks") {
          return apiSuccess({ items: [], nextCursor: null });
        }
        if (method === "GET" && path === "/v1/formats") {
          return apiSuccess({ items: [STANDARD] });
        }
        if (method === "GET" && path === "/v1/cosmetics") {
          return apiSuccess({ items: [], nextCursor: null });
        }
        if (method === "GET" && path === "/v1/purchases") {
          return apiSuccess({ items: [], nextCursor: null });
        }
        return apiError("No such route.", 404);
      };

      await runtime.withAuthedPage(
        {
          token: OWNER_TOKEN,
          subject: SUBJECT,
          viewport: options.viewport,
          answer,
          extraRoutes: async (context) => {
            await context.route("https://www.patreon.com/**", (route) => {
              const visited = new URL(route.request().url());
              patreonVisits.push(visited);
              const back =
                redirectMode === "approve"
                  ? new URLSearchParams({
                      code: PATREON_CODE,
                      state: visited.searchParams.get("state") ?? "",
                    })
                  : new URLSearchParams({
                      error: "access_denied",
                      state: visited.searchParams.get("state") ?? "",
                    });
              return route.fulfill({
                status: 302,
                headers: {
                  location: `${runtime.origin}/patreon/return?${back.toString()}`,
                },
              });
            });
          },
        },
        async ({ page, sent }) => {
          await fn({ page, sent, patreonVisits });
        },
      );
    };

    await runtime.runCheck(
      "unlinked account shows 'Not linked.' and enables Connect Patreon",
      async () => {
        await withPatreonScenario(
          { initialAccountPatreon: unlinkedState },
          async ({ page }) => {
            await page.goto(
              `${runtime.origin}/collection/decks?account=patreon`,
            );
            const dialog = page.getByRole("dialog", {
              name: "Account settings",
            });
            await dialog.getByText("Not linked.", { exact: true }).waitFor();
            const connectBtn = dialog.getByRole("button", {
              name: "Connect Patreon",
            });
            assertCondition(
              await connectBtn.isEnabled(),
              "Connect Patreon button should be enabled",
            );
            assertCondition(
              (await dialog.getByText("Active supporter").count()) === 0,
              "Unlinked account must not show Active supporter",
            );
          },
        );
      },
    );

    await runtime.runCheck(
      "foreign state on /patreon/return is refused without calling /complete and strips URL params",
      async () => {
        await withPatreonScenario(
          { initialAccountPatreon: unlinkedState },
          async ({ page, sent }) => {
            const foreign = new URLSearchParams({
              code: "foreign-code-0001",
              state: "BBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBA",
            });
            await page.goto(
              `${runtime.origin}/patreon/return?${foreign.toString()}`,
            );
            const dialog = page.getByRole("dialog", {
              name: "Account settings",
            });
            await dialog.getByText("Couldn’t link Patreon.").first().waitFor();
            const currentUrl = new URL(page.url());
            assertCondition(
              sent("/v1/account/patreon/complete").length === 0,
              "Foreign OAuth return must never call /complete",
            );
            assertCondition(
              currentUrl.searchParams.get("account") === "patreon",
              "Should redirect into Account settings Patreon tab",
            );
            assertCondition(
              !currentUrl.searchParams.has("code") &&
                !currentUrl.searchParams.has("state"),
              "OAuth code and state must be stripped from URL immediately",
            );
          },
        );
      },
    );

    await runtime.runCheck(
      "user refusal (?error=access_denied) clears saved opened request and never calls /complete",
      async () => {
        await withPatreonScenario(
          { initialAccountPatreon: unlinkedState, redirectMode: "refuse" },
          async ({ page, sent, patreonVisits }) => {
            await page.goto(
              `${runtime.origin}/collection/decks?account=patreon`,
            );
            const dialog = page.getByRole("dialog", {
              name: "Account settings",
            });
            const connectBtn = dialog.getByRole("button", {
              name: "Connect Patreon",
            });
            await connectBtn.waitFor();
            await connectBtn.click();
            await dialog.getByText("Couldn’t link Patreon.").first().waitFor();
            await connectBtn.waitFor();
            const saved = await page.evaluate(
              (key) => window.localStorage.getItem(key),
              `spellwake.patreon-link.v1:${runtime.apiOrigin}:${ACCOUNT}`,
            );
            assertCondition(
              patreonVisits.length === 1,
              "Should have visited Patreon once",
            );
            assertCondition(
              sent("/v1/account/patreon/complete").length === 0,
              "Refused authorization must not call /complete",
            );
            assertCondition(
              saved === null,
              `Saved opened request must be cleared after refusal, got: ${String(saved)}`,
            );
            assertCondition(
              await connectBtn.isEnabled(),
              "Connect Patreon must remain retryable",
            );
          },
        );
      },
    );

    await runtime.runCheck(
      "dropped /complete ack recovers via /status without resending the OAuth code",
      async () => {
        await withPatreonScenario(
          {
            initialAccountPatreon: unlinkedState,
            redirectMode: "approve",
            failCompleteWithErrorAfterLinking: true,
          },
          async ({ page, sent }) => {
            await page.goto(
              `${runtime.origin}/collection/decks?account=patreon`,
            );
            const dialog = page.getByRole("dialog", {
              name: "Account settings",
            });
            const connectBtn = dialog.getByRole("button", {
              name: "Connect Patreon",
            });
            await connectBtn.waitFor();
            await connectBtn.click();
            await dialog.getByText("Active supporter").waitFor();
            assertCondition(
              sent("/v1/account/patreon/complete").length === 1,
              "OAuth code must be sent at most once even when /complete errors",
            );
            assertCondition(
              sent("/v1/account/patreon/status").length >= 1,
              "Client must observe /status to recover after /complete error",
            );
            const saved = await page.evaluate(
              (key) => window.localStorage.getItem(key),
              `spellwake.patreon-link.v1:${runtime.apiOrigin}:${ACCOUNT}`,
            );
            const record = JSON.parse(saved ?? "{}") as Record<string, unknown>;
            assertCondition(
              record["phase"] === "linked",
              `Recovered link should persist phase=linked, got: ${String(saved)}`,
            );
          },
        );
      },
    );

    await runtime.runCheck(
      "happy-path connect links account, persists across reload, and stores OAuth code nowhere",
      async () => {
        await withPatreonScenario(
          { initialAccountPatreon: unlinkedState, redirectMode: "approve" },
          async ({ page, sent, patreonVisits }) => {
            await page.goto(
              `${runtime.origin}/collection/decks?account=patreon`,
            );
            const dialog = page.getByRole("dialog", {
              name: "Account settings",
            });
            const connectBtn = dialog.getByRole("button", {
              name: "Connect Patreon",
            });
            await connectBtn.waitFor();
            await connectBtn.click();
            await dialog.getByText("Active supporter").waitFor();
            await dialog.getByText("Linked", { exact: true }).waitFor();
            const manageBtn = dialog.getByRole("button", {
              name: "Manage on Patreon",
            });
            assertCondition(
              await manageBtn.isEnabled(),
              "Manage on Patreon should be enabled",
            );

            const [start] = sent("/v1/account/patreon/start");
            const [complete] = sent("/v1/account/patreon/complete");
            assertCondition(
              patreonVisits.length === 1,
              "Should open Patreon once",
            );
            assertCondition(
              patreonVisits[0]?.searchParams.get("state") ===
                start?.["stateKey"],
              "Patreon authorize URL state must match issued stateKey",
            );
            assertCondition(
              complete?.["requestId"] === start?.["requestId"] &&
                complete?.["code"] === PATREON_CODE,
              "Completion payload must match issued requestId and returned code",
            );

            await page.goto(
              `${runtime.origin}/collection/decks?account=patreon`,
            );
            await dialog.getByText("Active supporter").waitFor();
            const leakedCode = await page.evaluate(
              (code) =>
                window.location.href.includes(code) ||
                [window.localStorage, window.sessionStorage].some((storage) =>
                  Object.keys(storage).some(
                    (key) =>
                      key.includes(code) ||
                      (storage.getItem(key) ?? "").includes(code),
                  ),
                ),
              PATREON_CODE,
            );
            assertCondition(
              !leakedCode,
              "OAuth code must not be stored in URL or browser storage",
            );
          },
        );
      },
    );

    await runtime.runCheck(
      "linked account without active membership shows 'No active membership' instead of Active supporter",
      async () => {
        await withPatreonScenario(
          {
            initialAccountPatreon: {
              linked: true,
              active: false,
              supporter: false,
              tier: null,
            },
          },
          async ({ page }) => {
            await page.goto(
              `${runtime.origin}/collection/decks?account=patreon`,
            );
            const dialog = page.getByRole("dialog", {
              name: "Account settings",
            });
            await dialog.getByText("Linked", { exact: true }).waitFor();
            await dialog
              .getByText("No active membership", { exact: true })
              .waitFor();
            assertCondition(
              (await dialog.getByText("Active supporter").count()) === 0,
              "Inactive membership must never display Active supporter",
            );
            assertCondition(
              await dialog
                .getByRole("button", { name: "Manage on Patreon" })
                .isEnabled(),
              "Manage on Patreon should be enabled for linked inactive account",
            );
          },
        );
      },
    );

    await runtime.runCheck(
      "mobile viewport (390x844) renders Patreon settings without horizontal overflow",
      async () => {
        await withPatreonScenario(
          {
            initialAccountPatreon: unlinkedState,
            viewport: { width: 390, height: 844 },
          },
          async ({ page }) => {
            await page.goto(
              `${runtime.origin}/collection/decks?account=patreon`,
            );
            const dialog = page.getByRole("dialog", {
              name: "Account settings",
            });
            const connectBtn = dialog.getByRole("button", {
              name: "Connect Patreon",
            });
            await connectBtn.waitFor();
            const metrics = await page.evaluate(() => ({
              scrollWidth: document.documentElement.scrollWidth,
              clientWidth: document.documentElement.clientWidth,
            }));
            assertCondition(
              metrics.scrollWidth <= metrics.clientWidth,
              `Mobile viewport overflow detected: scrollWidth=${metrics.scrollWidth} > clientWidth=${metrics.clientWidth}`,
            );
            const box = await connectBtn.boundingBox();
            assertCondition(
              box !== null && box.x >= 0 && box.x + box.width <= 390,
              `Connect Patreon button clipped outside 390px viewport: ${JSON.stringify(box)}`,
            );
          },
        );
      },
    );

    runtime.finish();
  },
);

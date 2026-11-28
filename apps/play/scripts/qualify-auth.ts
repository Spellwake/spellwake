/**
 * Standalone Playwright E2E qualification for the Account & Auth domain.
 *
 * Verifies:
 * 1. Email OTP validation: malformed address is refused client-side without sending a request.
 * 2. Email OTP failure: server failure keeps the address and displays a passive error.
 * 3. Email OTP verification + username onboarding: wrong 6-digit code is cleared, valid code
 *    advances to /username, invalid/taken usernames are refused, free username creates the account.
 * 4. Google OAuth callback handoff lands in the signed-in account and malformed callback fails cleanly.
 * 5. Sign out clears local auth storage and redirects back to Sign in.
 * 6. Account deletion requires confirmation, sends one deletion request, holds the account outside
 *    the app while pending, polls status without bearer credentials, and clears storage once deleted.
 * 7. Mobile viewport (390x844) sign-in renders with zero horizontal overflow.
 *
 * Usage: node apps/play/scripts/qualify-auth.ts
 */
import type { Route } from "@playwright/test";
import {
  apiError,
  apiFail,
  apiSuccess,
  assertCondition,
  CORS_HEADERS,
  makeUuid,
  standInSession,
  withPlayE2eRuntime,
  type ApiReply,
} from "./playwright-harness.ts";

const SUBJECT = makeUuid(1);
const NEW_SUBJECT = makeUuid(4);
const ACCOUNT = makeUuid(2);
const NEW_ACCOUNT = makeUuid(5);
const SESSION_ID = makeUuid(3);
const NEW_SESSION = makeUuid(6);
const STANDARD = { id: makeUuid(10), key: "standard", name: "Standard" };
const GOBLINS = makeUuid(40);
const GOBLINS_REV = makeUuid(60);
const CLASSIC_BACK = makeUuid(53);
const CLASSIC_SLEEVE = makeUuid(54);
const IVORY_DICE = makeUuid(55);
const OWNER_TOKEN = "stand-in-auth-owner-token";
const NEW_TOKEN = "stand-in-auth-new-token";
const GOOD_CODE = "246810";

const APPEARANCE = {
  backId: CLASSIC_BACK,
  sleeveId: CLASSIC_SLEEVE,
  diceId: IVORY_DICE,
  playmatId: null,
};

async function expectEventually(
  predicate: () => boolean,
  timeout = 15_000,
): Promise<void> {
  const deadline = Date.now() + timeout;
  while (!predicate()) {
    if (Date.now() > deadline) {
      throw new Error("Timed out waiting for expected condition.");
    }
    await new Promise((resolve) => setTimeout(resolve, 150));
  }
}

await withPlayE2eRuntime(
  "account-auth",
  { authPort: 59995, apiPort: 59996 },
  async (runtime) => {
    const authRequests: { path: string; body: Record<string, unknown> }[] = [];
    const registered = new Map<string, { accountId: string; username: string }>();
    const deleting = new Map<string, string>();
    const deleted = new Set<string>();
    const observations: { hadBearer: boolean }[] = [];

    const makeServeAuth = () => async (route: Route): Promise<void> => {
      const request = route.request();
      const url = new URL(request.url());
      const cors = {
        ...CORS_HEADERS,
        "access-control-allow-headers": "*",
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
        const back = new URL(url.searchParams.get("redirect_to") ?? "");
        back.searchParams.set("code", "stand-in-provider-code");
        await route.fulfill({ status: 302, headers: { location: back.href } });
        return;
      }
      const body = (request.postDataJSON() ?? {}) as Record<string, unknown>;
      authRequests.push({ path: url.pathname, body });
      if (url.pathname === "/auth/v1/otp") {
        const fail = String(body["email"]).startsWith("fail@");
        return json(fail ? 500 : 200, fail ? { msg: "unavailable" } : {});
      }
      if (url.pathname === "/auth/v1/verify") {
        return body["token"] === GOOD_CODE
          ? json(200, standInSession(NEW_TOKEN, NEW_SUBJECT))
          : json(403, {
              code: 403,
              error_code: "otp_expired",
              msg: "Token has expired or is invalid",
            });
      }
      if (url.pathname === "/auth/v1/token") {
        return json(200, standInSession(OWNER_TOKEN, SUBJECT));
      }
      if (url.pathname === "/auth/v1/logout") {
        return json(204, {});
      }
      return json(404, { msg: "No such route." });
    };

    const answer = (
      method: string,
      url: URL,
      body: unknown,
      bearer: string,
    ): ApiReply => {
      const path = url.pathname;
      const command = (body ?? {}) as Record<string, unknown>;

      if (method === "POST" && path === "/v1/account/deletion/observe") {
        observations.push({ hadBearer: bearer.length > 0 });
        const accountId = String(command["accountId"] ?? "");
        const requestId = String(command["requestId"] ?? "");
        if (deleting.get(accountId) !== requestId) {
          return apiFail({ observer: "unavailable" }, 409);
        }
        return apiSuccess(
          deleted.has(accountId)
            ? {
                accountId,
                requestId,
                phase: "deleted",
                retirementOperationId: makeUuid(70),
                authCleanupReceiptId: makeUuid(71),
              }
            : { accountId, requestId, phase: "pending" },
        );
      }

      if (bearer !== OWNER_TOKEN && bearer !== NEW_TOKEN) {
        return apiFail({ session: "unauthorized" }, 401);
      }

      const isOwner = bearer === OWNER_TOKEN;
      if (isOwner && deleted.has(ACCOUNT)) {
        return apiFail({ session: "unauthorized" }, 401);
      }
      if (method === "GET" && path === "/v1/session") {
        if (isOwner) {
          return apiSuccess({
            subjectId: SUBJECT,
            sessionId: SESSION_ID,
            method: "google",
            state: deleting.has(ACCOUNT) ? "deletion_pending" : "active",
            accountId: ACCOUNT,
          });
        }
        const entry = registered.get(NEW_SUBJECT);
        return apiSuccess({
          subjectId: NEW_SUBJECT,
          sessionId: NEW_SESSION,
          method: "email_code",
          state: entry ? "active" : "needs_registration",
          accountId: entry?.accountId ?? null,
        });
      }

      if (method === "POST" && path === "/v1/account") {
        const username = String(command["username"] ?? "");
        if (username.toLowerCase() === "moonwatcher") {
          return apiFail({ username: "unavailable" }, 409);
        }
        registered.set(NEW_SUBJECT, { accountId: NEW_ACCOUNT, username });
        return apiSuccess({ accountId: NEW_ACCOUNT });
      }

      const activeAccount = isOwner
        ? { accountId: ACCOUNT, username: "Planeswalker" }
        : registered.get(NEW_SUBJECT);
      if (!activeAccount) {
        return apiFail({ account: "not registered" }, 403);
      }

      if (method === "GET" && path === "/v1/account") {
        return apiSuccess({
          accountId: activeAccount.accountId,
          username: activeAccount.username,
          state: deleting.has(activeAccount.accountId)
            ? "deletion_pending"
            : "active",
          appearanceRevision: "1",
          defaults: APPEARANCE,
          wallet: { owned: "14500", held: "0", available: "14500" },
        });
      }

      if (method === "GET" && path === "/v1/account/profile") {
        return apiSuccess({
          accountId: activeAccount.accountId,
          cardArtProfile: null,
          supporter: false,
        });
      }

      if (method === "GET" && path === "/v1/account/patreon") {
        return apiSuccess({
          accountId: activeAccount.accountId,
          linked: false,
          active: false,
          supporter: false,
          tier: null,
        });
      }

      if (method === "POST" && path === "/v1/account/deletion") {
        if (
          typeof command["requestId"] !== "string" ||
          !/^[0-9a-f]{64}$/.test(String(command["statusKeyHash"]))
        ) {
          return apiFail({ request: "invalid" }, 400);
        }
        deleting.set(activeAccount.accountId, command["requestId"]);
        return apiSuccess({ resourceId: activeAccount.accountId });
      }

      if (method === "GET" && path === "/v1/formats") {
        return apiSuccess({ items: [STANDARD], nextCursor: null });
      }

      if (method === "GET" && path === "/v1/decks") {
        return apiSuccess({
          items: isOwner
            ? [
                {
                  id: GOBLINS,
                  name: "Goblins",
                  mode: "collection",
                  currentRevisionId: GOBLINS_REV,
                  format: STANDARD,
                  appearance: APPEARANCE,
                },
              ]
            : [],
          nextCursor: null,
        });
      }

      if (
        method === "GET" &&
        (path === "/v1/cosmetics" ||
          path === "/v1/purchases" ||
          path === "/v1/inventory")
      ) {
        return apiSuccess({ items: [], nextCursor: null });
      }

      if (method === "GET" && path === "/v1/social/status") {
        return apiSuccess({
          accountId: activeAccount.accountId,
          unreadMessages: "0",
          personId: null,
          person: null,
        });
      }

      if (method === "GET" && path === "/v1/social/threads") {
        return apiSuccess({
          accountId: activeAccount.accountId,
          items: [],
          nextCursor: null,
        });
      }

      if (method === "POST" && path === "/v1/catalogue/resolve") {
        return apiSuccess({ cards: [], printings: [] });
      }

      return apiError("No such route.", 404);
    };

    // 1-3. Guest email OTP entry + username onboarding
    await runtime.withGuestPage(
      { answer, serveAuth: makeServeAuth() },
      async ({ page, sent }) => {
        const heading = (name: string) => page.getByRole("heading", { name });
        const email = page.getByLabel("Email");
        const codeField = page.getByLabel("Verification code");
        const username = page.getByLabel("Username");

        await page.goto(`${runtime.origin}/sign-in`);
        await heading("Sign in to Spellwake").waitFor();

        await runtime.runCheck(
          "email otp: malformed email refused locally without auth requests",
          async () => {
            await email.fill("mage@");
            await page.getByRole("button", { name: "Login with Email" }).click();
            await page.getByText("Enter a valid email address.").waitFor();
            assertCondition(
              authRequests.length === 0,
              `Expected 0 auth requests, saw ${authRequests.length}`,
            );
          },
        );

        await runtime.runCheck(
          "email otp: failed send keeps email and shows passive error",
          async () => {
            await email.fill("fail@example.com");
            await page.getByRole("button", { name: "Login with Email" }).click();
            await page.getByText("Couldn’t send code.").waitFor();
            assertCondition(
              (await email.inputValue()) === "fail@example.com",
              "Email field did not retain address after failed send",
            );
          },
        );

        await runtime.runCheck(
          "email otp + onboarding: wrong code cleared, valid code reaches username, taken refused, free creates account",
          async () => {
            await email.fill("  new@example.com ");
            await email.press("Enter");
            await heading("Enter your code").waitFor();
            await page
              .getByText("We sent a 6-digit code to new@example.com")
              .waitFor();

            await codeField.fill("000000");
            await page.getByText("Invalid or expired code.").waitFor();
            assertCondition(
              (await codeField.inputValue()) === "",
              "Expected code input to clear after invalid OTP",
            );

            await codeField.fill(GOOD_CODE);
            await heading("Choose your username").waitFor();
            await page.waitForURL(/\/username$/);

            await username.fill("moon watcher");
            await username.press("Enter");
            await page
              .getByText("Use 3–24 letters, numbers or underscores.")
              .waitFor();
            assertCondition(
              sent("/v1/account").length === 0,
              "Invalid username should not hit /v1/account",
            );

            await username.fill("Moonwatcher");
            await username.press("Enter");
            await page.getByText("That username is taken.").waitFor();

            await username.fill("Stargazer_7");
            await username.press("Enter");
            await page.getByText("No decks yet").waitFor();
            assertCondition(
              sent("/v1/account").length === 2 &&
                sent("/v1/account")[1]?.["username"] === "Stargazer_7",
              "Expected registration request for Stargazer_7",
            );
          },
        );
      },
    );

    // 4. Google OAuth callback handoff & malformed return
    await runtime.withGuestPage(
      { answer, serveAuth: makeServeAuth() },
      async ({ page }) => {
        await runtime.runCheck(
          "oauth: Google handoff lands in Collection and malformed callback fails cleanly",
          async () => {
            await page.goto(`${runtime.origin}/sign-in`);
            await page
              .getByRole("button", { name: "Continue with Google" })
              .click();
            await page.getByRole("heading", { name: "Goblins" }).waitFor();
            assertCondition(
              !page.url().includes("code="),
              "OAuth code leaked in URL after callback",
            );

            await page.goto(`${runtime.origin}/auth/callback?code=only-a-code`);
            await page
              .getByRole("heading", { name: "Couldn’t finish signing in" })
              .waitFor();
          },
        );
      },
    );

    // 5. Sign out clears session storage and prevents returning to signed-in routes
    await runtime.withGuestPage(
      {
        answer,
        serveAuth: makeServeAuth(),
        seedSession: { token: OWNER_TOKEN, subject: SUBJECT },
      },
      async ({ page }) => {
        await runtime.runCheck(
          "sign out: ends session, clears localStorage, and blocks reload access",
          async () => {
            await page.goto(`${runtime.origin}/collection/decks`);
            await page.getByRole("heading", { name: "Goblins" }).waitFor();
            await page.getByRole("button", { name: "Account" }).click();
            await page.getByRole("menuitem", { name: "Sign out" }).click();
            await page
              .getByRole("heading", { name: "Sign in to Spellwake" })
              .waitFor();
            const isCleared = await page.evaluate(
              (key) => window.localStorage.getItem(key) === null,
              `spellwake.auth.v1:${new URL(runtime.authOrigin).host}`,
            );
            assertCondition(isCleared, "Expected auth session cleared from localStorage");
            await page.goto(`${runtime.origin}/collection/decks`);
            await page
              .getByRole("heading", { name: "Sign in to Spellwake" })
              .waitFor();
          },
        );
      },
    );

    // 6. Account deletion confirmation, pending-deletion hold, credential-free polling, completion
    await runtime.withGuestPage(
      {
        answer,
        serveAuth: makeServeAuth(),
        seedSession: { token: OWNER_TOKEN, subject: SUBJECT },
      },
      async ({ page, sent }) => {
        await runtime.runCheck(
          "account deletion: confirms once, holds outside app, polls without bearer, and clears on delete",
          async () => {
            await page.goto(`${runtime.origin}/collection/decks?account=profile`);
            const dialog = page.getByRole("dialog", {
              name: "Account settings",
            });
            await dialog.getByRole("heading", { name: "Planeswalker" }).waitFor();
            await dialog.getByRole("tab", { name: "Account" }).click();

            const confirm = page.getByRole("dialog", {
              name: "Delete your account?",
            });
            await dialog
              .getByRole("button", { name: "Delete account" })
              .click();
            await confirm.getByRole("button", { name: "Cancel" }).click();
            await confirm.waitFor({ state: "hidden" });
            assertCondition(
              sent("/v1/account/deletion").length === 0,
              "Cancelling deletion should send no request",
            );

            await dialog
              .getByRole("button", { name: "Delete account" })
              .click();
            await confirm
              .getByRole("button", { name: "Delete account" })
              .click();
            await dialog
              .getByRole("heading", { name: "Deletion requested" })
              .waitFor();
            assertCondition(
              sent("/v1/account/deletion").length === 1,
              "Expected exactly one deletion request",
            );

            await dialog.getByRole("button", { name: "Close", exact: true }).click();
            await page
              .getByRole("heading", { name: "Deletion requested" })
              .waitFor();
            await expectEventually(() => observations.length > 0);
            assertCondition(
              observations.every((obs) => !obs.hadBearer),
              "Deletion status polling must not send a bearer token",
            );

            deleted.add(ACCOUNT);
            await page
              .getByRole("heading", { name: "Sign in to Spellwake" })
              .waitFor();
          },
        );
      },
    );

    // 7. Mobile viewport responsive check on /sign-in
    await runtime.withGuestPage(
      {
        viewport: { width: 390, height: 844 },
        answer,
        serveAuth: makeServeAuth(),
      },
      async ({ page }) => {
        await runtime.runCheck(
          "mobile 390x844: Sign-in screen renders with zero horizontal overflow",
          async () => {
            await page.goto(`${runtime.origin}/sign-in`);
            await page
              .getByRole("heading", { name: "Sign in to Spellwake" })
              .waitFor();
            const overflow = await page.evaluate(
              () =>
                document.documentElement.scrollWidth >
                document.documentElement.clientWidth,
            );
            assertCondition(
              !overflow,
              "Horizontal overflow detected on /sign-in at 390x844",
            );
          },
        );
      },
    );

    runtime.finish();
  },
);

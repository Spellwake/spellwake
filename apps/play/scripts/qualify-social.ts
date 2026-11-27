/**
 * Standalone Playwright E2E qualification for the Social & Messaging domain.
 *
 * Verifies:
 * 1. Messages panel lists threads and unread badge count from /v1/social/status and /v1/social/threads.
 * 2. Opening Mira's conversation loads history and acknowledges read exactly once via /v1/social/read.
 * 3. Sending a DM whose reply is dropped (503) holds unsent draft text and offers Check delivery.
 * 4. Clicking Check delivery recovers the sent message via /v1/social/commands/disposition without
 *    re-sending, appends the message once, clears the draft box, and clears unread count.
 * 5. Mobile viewport (390x844) Messages dialog renders within horizontal bounds with zero overflow.
 *
 * Usage: node apps/play/scripts/qualify-social.ts
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
const MIRA = makeUuid(12);
const CONVERSATION = makeUuid(13);
const STANDARD = { id: makeUuid(10), key: "standard", name: "Standard" };
const CLASSIC_BACK = makeUuid(53);
const CLASSIC_SLEEVE = makeUuid(54);
const IVORY_DICE = makeUuid(55);
const OWNER_TOKEN = "stand-in-social-token";

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
  "social",
  { authPort: 59973, apiPort: 59974 },
  async (runtime) => {
    let nextId = 200;
    let miraUnread = 1;
    let loseNextSend = false;
    let holdSocialDisposition = false;
    const outcomes = new Map<string, unknown>();

    const chat = [
      {
        id: makeUuid(14),
        senderId: MIRA,
        author: "Mira",
        body: "Want to play later?",
        sentAt: "2026-11-21T11:55:00Z",
      },
    ];

    const mira = () => ({
      id: MIRA,
      username: "Mira",
      isDeleted: false,
      relationship: "friend",
      friendshipId: makeUuid(99),
      isBlocked: false,
      canContact: true,
      conversationId: CONVERSATION,
      unreadMessages: String(miraUnread),
      supporter: false,
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
      if (
        method === "GET" &&
        (path === "/v1/decks" ||
          path === "/v1/cosmetics" ||
          path === "/v1/purchases" ||
          path === "/v1/inventory")
      ) {
        return apiSuccess({ items: [], nextCursor: null });
      }
      if (method === "GET" && path === "/v1/social/threads") {
        return apiSuccess({
          accountId: ACCOUNT,
          items: [mira()],
          nextCursor: null,
        });
      }
      if (method === "GET" && path === "/v1/social/status") {
        const wanted = url.searchParams.get("personId");
        return apiSuccess({
          accountId: ACCOUNT,
          unreadMessages: String(miraUnread),
          personId: wanted,
          person: wanted === MIRA ? mira() : null,
        });
      }
      if (
        method === "GET" &&
        path === `/v1/social/conversations/${CONVERSATION}/messages`
      ) {
        return apiSuccess({
          accountId: ACCOUNT,
          conversationId: CONVERSATION,
          person: mira(),
          items: chat,
          nextCursor: null,
        });
      }
      if (
        method === "POST" &&
        (path === "/v1/social/commands/disposition" ||
          path === "/v1/social/read/disposition")
      ) {
        const inner = (
          (command["command"] ?? command) as { requestId?: string }
        ).requestId;
        const known = holdSocialDisposition
          ? undefined
          : outcomes.get(inner ?? "");
        return apiSuccess(
          known === undefined
            ? { state: "unknown", response: null }
            : { state: "accepted", response: known },
        );
      }
      if (method === "POST" && path === "/v1/social/read") {
        miraUnread = 0;
        const result = apiSuccess({ resourceId: CONVERSATION });
        outcomes.set(String(command["requestId"]), result.body);
        return result;
      }
      if (method === "POST" && path === "/v1/social/commands") {
        const inner = command["command"] as Record<string, string>;
        if (command["kind"] !== "send_dm") {
          return apiFail({ contact: "contact unavailable" }, 409);
        }
        const message = {
          id: makeUuid(nextId++),
          senderId: ACCOUNT,
          author: "Planeswalker",
          body: String(inner["body"]),
          sentAt: new Date().toISOString(),
        };
        chat.unshift(message);
        const result = apiSuccess({ resourceId: message.id });
        outcomes.set(String(inner["requestId"]), result.body);
        if (loseNextSend) {
          loseNextSend = false;
          return apiError("Lost.", 503);
        }
        return result;
      }
      if (method === "POST" && path === "/v1/catalogue/resolve") {
        return apiSuccess({ cards: [], printings: [] });
      }
      return apiError("No such route.", 404);
    };

    await runtime.withAuthedPage(
      { token: OWNER_TOKEN, subject: SUBJECT, answer },
      async ({ page, sent }) => {
        await page.goto(`${runtime.origin}/collection/decks`);

        await runtime.runCheck(
          "messages: opens Mira's thread and acknowledges unread count once",
          async () => {
            await page.getByRole("button", { name: "Messages" }).click();
            const panel = page.getByRole("dialog", { name: "Messages" });
            await panel.getByText("Want to play later?").waitFor();
            await expectEventually(() => sent("/v1/social/read").length === 1);
            assertCondition(miraUnread === 0, "Expected Mira unread count cleared");
          },
        );

        await runtime.runCheck(
          "direct messages: dropped send keeps draft text and recovers via Check delivery with one send",
          async () => {
            const panel = page.getByRole("dialog", { name: "Messages" });
            loseNextSend = true;
            holdSocialDisposition = true;
            const box = panel.getByRole("textbox", { name: "Message Mira" });
            await box.fill("Ready when you are.");
            await box.press("Enter");

            const checkButton = panel.getByRole("button", {
              name: "Check delivery",
            });
            await checkButton.waitFor();
            const keptText = await box.inputValue();
            assertCondition(
              keptText === "Ready when you are.",
              `Expected unsent draft retained, got "${keptText}"`,
            );

            holdSocialDisposition = false;
            await checkButton.click();
            await checkButton.waitFor({ state: "detached" });
            await panel.getByText("Ready when you are.").waitFor();
            const sends = sent("/v1/social/commands").filter(
              (body) => body["kind"] === "send_dm",
            );
            assertCondition(
              sends.length === 1,
              `Expected 1 send_dm command, saw ${sends.length}`,
            );
            assertCondition(
              (await box.inputValue()) === "",
              "Expected message input cleared after delivery confirmed",
            );
          },
        );

        await runtime.runCheck(
          "direct messages: normal follow-up message appends cleanly",
          async () => {
            const panel = page.getByRole("dialog", { name: "Messages" });
            const box = panel.getByRole("textbox", { name: "Message Mira" });
            await box.fill("See you in lobby!");
            await box.press("Enter");
            await panel.getByText("See you in lobby!").waitFor();
            await expectEventually(
              () => chat.filter((m) => m.body === "See you in lobby!").length === 1,
            );
          },
        );
      },
    );

    await runtime.withAuthedPage(
      {
        token: OWNER_TOKEN,
        subject: SUBJECT,
        viewport: { width: 390, height: 844 },
        answer,
      },
      async ({ page }) => {
        await runtime.runCheck(
          "mobile 390x844: Messages panel renders within viewport without horizontal overflow",
          async () => {
            await page.goto(`${runtime.origin}/collection/decks`);
            await page.getByRole("button", { name: "Messages" }).click();
            const panel = page.getByRole("dialog", { name: "Messages" });
            await panel.getByText("Want to play later?").waitFor();
            const box = await panel.boundingBox();
            assertCondition(
              box !== null && box.x >= -1 && box.x + box.width <= 391,
              `Messages dialog clipped horizontally at 390x844: ${JSON.stringify(box)}`,
            );
            const overflow = await page.evaluate(
              () =>
                document.documentElement.scrollWidth >
                document.documentElement.clientWidth,
            );
            assertCondition(
              !overflow,
              "Horizontal overflow detected with Messages open at 390x844",
            );
          },
        );
      },
    );

    runtime.finish();
  },
);

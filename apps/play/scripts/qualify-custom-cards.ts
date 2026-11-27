/**
 * Standalone Playwright E2E qualification for the Custom Cards domain.
 *
 * Verifies:
 * 1. /open-play/custom-cards loads the connected vault (/v1/custom-cards) and runs live
 *    compilation (/v1/custom-cards/compile) showing playable status and AST chips.
 * 2. Editing card name/rules and clicking Save posts to /v1/custom-cards/:id/revisions
 *    with expectedRevisionId and displays the saved toast.
 * 3. Standardize phrasing hits /v1/custom-cards/standardize and updates rules text cleanly.
 * 4. Play with card enforces the Patron play-gate for non-Patrons (opens PatreonSupportModal)
 *    while allowing Patrons to open the Quick Test popover.
 * 5. Mobile viewport (390x844) renders Custom Card Studio without horizontal overflow.
 *
 * Usage: node apps/play/scripts/qualify-custom-cards.ts
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
const CARD_ID = makeUuid(500);
const REV_1 = makeUuid(501);
const REV_2 = makeUuid(502);
const CLASSIC_BACK = makeUuid(53);
const CLASSIC_SLEEVE = makeUuid(54);
const IVORY_DICE = makeUuid(55);
const OWNER_TOKEN = "stand-in-custom-cards-token";
const RULES_SHA = "a".repeat(64);
const RULES_SHA_2 = "b".repeat(64);

const APPEARANCE = {
  backId: CLASSIC_BACK,
  sleeveId: CLASSIC_SLEEVE,
  diceId: IVORY_DICE,
  playmatId: null,
};

await withPlayE2eRuntime(
  "custom-cards",
  { authPort: 59971, apiPort: 59972 },
  async (runtime) => {
    let isPatron = false;
    let currentRevision = {
      revisionId: REV_1,
      customCardId: CARD_ID,
      revisionNumber: "1",
      rulesSha256: RULES_SHA,
      name: "Aetherblade Vanguard",
      layout: "normal" as const,
      rarity: "rare" as const,
      faces: [
        {
          name: "Aetherblade Vanguard",
          manaCost: "{2}{U}{R}",
          typeLine: "Creature — Human Artificer",
          oracleText: "Flying, haste\nWhen Aetherblade Vanguard enters, draw a card.",
          power: "3",
          toughness: "2",
          loyalty: null,
          defense: null,
          colorIndicator: null,
        },
      ],
      visualFrameAccents: ["auto" as const],
      flavorTexts: ["Forged from lightning and glass."],
      collectorNote: "001/250",
      stage1Parsed: true,
      stage2Playable: true,
      compilerRevision: "spellwake-custom-v1",
      art: [],
      createdAt: "2026-11-20T12:00:00Z",
    };

    const makeCompileResult = (sha = RULES_SHA) => ({
      rulesSha256: sha,
      compilerRevision: "spellwake-custom-v1",
      stage1Parsed: true,
      stage2Playable: true,
      diagnostics: [],
      astReading: [
        {
          faceIndex: 0,
          kind: "keyword",
          label: "Flying, haste",
          summary: "Creature has Flying and Haste.",
        },
        {
          faceIndex: 0,
          kind: "triggered",
          label: "ETB Trigger",
          summary: "When this creature enters, draw a card.",
        },
      ],
      colorIdentity: ["U", "R"],
      manaValue: 4,
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
          supporter: isPatron,
        });
      }
      if (method === "GET" && path === "/v1/account/patreon") {
        return apiSuccess({
          accountId: ACCOUNT,
          linked: isPatron,
          active: isPatron,
          supporter: isPatron,
          tier: isPatron
            ? { id: makeUuid(88), name: "Spellwake Supporter", rank: 1 }
            : null,
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
      if (method === "GET" && path === "/v1/custom-cards") {
        return apiSuccess({
          accountId: ACCOUNT,
          activeCount: 1,
          activeLimit: 250,
          items: [
            {
              id: CARD_ID,
              archivedAt: null,
              createdAt: "2026-11-20T12:00:00Z",
              updatedAt: "2026-11-20T12:00:00Z",
              currentRevision,
            },
          ],
          nextCursor: null,
        });
      }
      if (method === "POST" && path === "/v1/custom-cards/compile") {
        return apiSuccess(makeCompileResult(RULES_SHA));
      }
      if (method === "POST" && path === "/v1/custom-cards/standardize") {
        const faces = (command["faces"] as Array<Record<string, unknown>>) ?? [];
        const updatedFaces = faces.map((f) => ({
          ...f,
          oracleText: String(f["oracleText"] ?? "").replace(
            "enters the battlefield",
            "enters",
          ),
        }));
        return apiSuccess({
          faces: updatedFaces,
          changes: [
            {
              faceIndex: 0,
              before: "enters the battlefield",
              after: "enters",
              reason: "Modern Foundations wording",
            },
          ],
          changed: true,
        });
      }
      if (
        method === "POST" &&
        path === `/v1/custom-cards/${CARD_ID}/revisions`
      ) {
        const faces = command["faces"] as typeof currentRevision.faces;
        currentRevision = {
          ...currentRevision,
          revisionId: REV_2,
          revisionNumber: "2",
          rulesSha256: RULES_SHA_2,
          name: faces[0]?.name ?? currentRevision.name,
          faces,
        };
        return apiSuccess({
          customCardId: CARD_ID,
          revisionId: REV_2,
          revisionNumber: "2",
          compile: makeCompileResult(RULES_SHA_2),
        });
      }
      if (method === "POST" && path === "/v1/catalogue/resolve") {
        return apiSuccess({ cards: [], printings: [] });
      }
      return apiError("No such route.", 404);
    };

    // 1-4. Non-patron studio authoring, standardization, save revision, and Patron gate
    await runtime.withAuthedPage(
      { token: OWNER_TOKEN, subject: SUBJECT, answer },
      async ({ page, sent }) => {
        await page.goto(`${runtime.origin}/open-play/custom-cards`);

        await runtime.runCheck(
          "studio vault & compiler: loads vault card and compiles live AST diagnostics",
          async () => {
            await page
              .locator('section[aria-label="Custom Card Studio"]')
              .waitFor();
            await page.getByText("Aetherblade Vanguard").first().waitFor();
            await page.getByRole("button", { name: "Inspect AST" }).click();
            await page.getByText("ETB Trigger").first().waitFor();
            assertCondition(
              sent("/v1/custom-cards/compile").length >= 1,
              "Expected at least one /v1/custom-cards/compile request",
            );
          },
        );

        await runtime.runCheck(
          "standardize phrasing: normalizes legacy ETB wording via /v1/custom-cards/standardize",
          async () => {
            const oracleBox = page.getByPlaceholder(
              /Flying, haste/,
            );
            await oracleBox.fill(
              "Flying, haste\nWhen Aetherblade Vanguard enters the battlefield, draw a card.",
            );
            await page
              .getByRole("button", { name: "Standardize phrasing" })
              .click();
            await page.getByText(/Standardized 1 phrasing item/).waitFor();
            const value = await oracleBox.inputValue();
            assertCondition(
              value.includes("enters, draw a card.") &&
                !value.includes("enters the battlefield"),
              `Expected standardized wording, got: ${value}`,
            );
          },
        );

        await runtime.runCheck(
          "save revision: posts updated card to /v1/custom-cards/:id/revisions with expectedRevisionId",
          async () => {
            const nameInput = page.getByPlaceholder("Aetherblade Vanguard");
            await nameInput.fill("Aetherblade Paragon");
            await page.getByRole("button", { name: "Save", exact: true }).click();
            await page
              .getByText("Saved custom card to your Open Play vault.")
              .waitFor();
            const saves = sent(`/v1/custom-cards/${CARD_ID}/revisions`);
            assertCondition(
              saves.length === 1 && saves[0]?.["expectedRevisionId"] === REV_1,
              `Expected revision save against ${REV_1}, saw ${JSON.stringify(saves)}`,
            );
          },
        );

        await runtime.runCheck(
          "patron play gate: non-Patron clicking Play with card sees PatreonSupportModal",
          async () => {
            await page
              .getByRole("button", { name: "Play with card" })
              .click();
            await page.getByText(/Patreon/i).first().waitFor();
            const popoverCount = await page
              .getByRole("dialog", { name: "Play with card options" })
              .count();
            assertCondition(
              popoverCount === 0,
              "Non-Patron must not see the Patron Quick Test popover",
            );
          },
        );
      },
    );

    // 5. Patron Quick Test popover
    isPatron = true;
    await runtime.withAuthedPage(
      { token: OWNER_TOKEN, subject: SUBJECT, answer },
      async ({ page }) => {
        await runtime.runCheck(
          "patron quick test: active Patron clicking Play with card opens Quick Test vs. Bot options",
          async () => {
            await page.goto(`${runtime.origin}/open-play/custom-cards`);
            await page.getByText("Aetherblade Paragon").first().waitFor();
            await page
              .getByRole("button", { name: "Play with card" })
              .click();
            const popover = page.getByRole("dialog", {
              name: "Play with card options",
            });
            await popover
              .getByRole("button", { name: "Quick Test vs. Bot" })
              .waitFor();
          },
        );
      },
    );

    // 6. Mobile viewport check
    await runtime.withAuthedPage(
      {
        token: OWNER_TOKEN,
        subject: SUBJECT,
        viewport: { width: 390, height: 844 },
        answer,
      },
      async ({ page }) => {
        await runtime.runCheck(
          "mobile 390x844: Custom Card Studio renders with zero horizontal overflow",
          async () => {
            await page.goto(`${runtime.origin}/open-play/custom-cards`);
            await page
              .locator('section[aria-label="Custom Card Studio"]')
              .waitFor();
            const overflow = await page.evaluate(
              () =>
                document.documentElement.scrollWidth >
                document.documentElement.clientWidth,
            );
            assertCondition(
              !overflow,
              "Horizontal overflow detected on /open-play/custom-cards at 390x844",
            );
          },
        );
      },
    );

    runtime.finish();
  },
);

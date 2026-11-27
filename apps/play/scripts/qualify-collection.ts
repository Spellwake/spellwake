/**
 * Standalone Playwright E2E qualification for the Decks & Collection domain.
 *
 * Verifies:
 * 1. Deck Builder revision save, format settings save, and optimistic concurrency conflict refusal.
 * 2. Create deck makes a single Untitled deck and opens it cleanly.
 * 3. Deck Import recovers from a dropped reply (503) via /v1/decks/import/disposition without duplicating.
 * 4. Cards collection aggregates owned copies per printing and filters by set (Zendikar).
 * 5. Booster Pack opening recovers from a dropped reply via Check opening, reveals cards, and decrements count.
 * 6. Mobile viewport (390x844) renders Collection Decks and Cards without horizontal overflow.
 *
 * Usage: node apps/play/scripts/qualify-collection.ts
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
const MODERN = { id: makeUuid(11), key: "modern", name: "Modern" };
const BOLT = makeUuid(20);
const GUIDE = makeUuid(21);
const KRENKO = makeUuid(22);
const TOKEN = makeUuid(23);
const BOLT_PRINT = makeUuid(30);
const GUIDE_PRINT = makeUuid(31);
const GOBLINS = makeUuid(40);
const CLASSIC_BACK = makeUuid(53);
const CLASSIC_SLEEVE = makeUuid(54);
const IVORY_DICE = makeUuid(55);
const RECIPE = makeUuid(71);
const PACK_ASSET = makeUuid(72);
const OPENING = makeUuid(90);
const OWNER_TOKEN = "stand-in-collection-token";

const APPEARANCE = {
  backId: CLASSIC_BACK,
  sleeveId: CLASSIC_SLEEVE,
  diceId: IVORY_DICE,
  playmatId: null,
};

type Entry = {
  definitionId: string;
  printingId: string | null;
  section: string;
  quantity: string;
};

type Deck = {
  id: string;
  name: string;
  mode: "collection" | "open";
  format: typeof STANDARD;
  revision: { id: string; number: string; entries: Entry[] } | null;
  appearance: typeof APPEARANCE;
};

const CARDS = [
  { id: BOLT, name: "Lightning Bolt", oracleRef: "bolt" },
  { id: GUIDE, name: "Goblin Guide", oracleRef: "guide" },
  { id: KRENKO, name: "Krenko, Mob Boss", oracleRef: "krenko" },
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

await withPlayE2eRuntime(
  "decks-collection",
  { authPort: 59997, apiPort: 59998 },
  async (runtime) => {
    let nextId = 100;
    let loseNextImportReply = false;
    let loseNextOpenReply = false;
    let holdOpenDisposition = false;
    const outcomes = new Map<string, unknown>();
    const holdings = { bolt: 3, guide: 1, packs: 2 };

    const decks = new Map<string, Deck>([
      [
        GOBLINS,
        {
          id: GOBLINS,
          name: "Goblins",
          mode: "collection",
          format: STANDARD,
          revision: {
            id: makeUuid(60),
            number: "1",
            entries: [
              {
                definitionId: BOLT,
                printingId: BOLT_PRINT,
                section: "main",
                quantity: "4",
              },
              {
                definitionId: TOKEN,
                printingId: null,
                section: "tokens",
                quantity: "1",
              },
            ],
          },
          appearance: { ...APPEARANCE },
        },
      ],
    ]);

    const summary = (deck: Deck) => ({
      id: deck.id,
      name: deck.name,
      mode: deck.mode,
      currentRevisionId: deck.revision?.id ?? null,
      format: deck.format,
      appearance: deck.appearance,
    });

    const inventoryItem = (
      itemId: number,
      assetId: string,
      quantity: number,
      asset:
        | { kind: "card"; printingId: string }
        | { kind: "pack"; recipeId: string },
    ) => ({
      id: makeUuid(itemId),
      assetId,
      revision: "1",
      asset,
      owned: String(quantity),
      held: "0",
      available: String(quantity),
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
        return apiSuccess({ items: [STANDARD, MODERN], nextCursor: null });
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
          items: [{ id: makeUuid(74), name: "Foundations", code: "fdn" }],
          nextCursor: null,
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
      if (method === "GET" && path === `/v1/packs/openings/${OPENING}`) {
        return apiSuccess({
          accountId: ACCOUNT,
          outcome: {
            id: OPENING,
            inventoryAssetId: PACK_ASSET,
            recipeId: RECIPE,
            operationId: makeUuid(91),
            contents: [
              { printingId: BOLT_PRINT, quantity: "1" },
              { printingId: GUIDE_PRINT, quantity: "2" },
            ],
          },
        });
      }
      if (method === "POST" && path === "/v1/economy/commands/disposition") {
        const inner = (command["command"] ?? {}) as { requestId?: string };
        const known = holdOpenDisposition
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
      if (method === "POST" && path === "/v1/economy/commands") {
        const inner = command["command"] as {
          requestId: string;
          inventoryAssetId: string;
        };
        if (
          command["kind"] !== "pack_open" ||
          inner.inventoryAssetId !== PACK_ASSET
        ) {
          return apiFail({ assets: "not enough available assets" }, 409);
        }
        holdings.packs -= 1;
        holdings.bolt += 1;
        holdings.guide += 2;
        const result = apiSuccess({ resourceId: OPENING });
        outcomes.set(inner.requestId, result.body);
        if (loseNextOpenReply) {
          loseNextOpenReply = false;
          return apiError("Lost.", 503);
        }
        return result;
      }
      if (method === "GET" && path === "/v1/decks") {
        return apiSuccess({
          items: [...decks.values()]
            .sort((a, b) => a.id.localeCompare(b.id))
            .map(summary),
          nextCursor: null,
        });
      }
      if (method === "GET" && path === "/v1/cards") {
        const needle = (url.searchParams.get("query") ?? "").toLowerCase();
        const isExact = url.searchParams.get("exact") === "true";
        return apiSuccess({
          items: CARDS.filter((card) =>
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
        const collectorNumber = url.searchParams.get("collectorNumber");
        const named = CARDS.find((card) => card.name.toLowerCase() === name);
        return apiSuccess({
          items: PRINTINGS.filter(
            (p) =>
              p.definitionId === named?.id &&
              (!setCode || p.set.code === setCode) &&
              (!collectorNumber || p.collectorNumber === collectorNumber),
          ),
          nextCursor: null,
        });
      }
      if (method === "POST" && path === "/v1/catalogue/resolve") {
        const wanted = command as {
          definitionIds: string[];
          printingIds: string[];
        };
        return apiSuccess({
          cards: CARDS.filter((c) => wanted.definitionIds.includes(c.id)),
          printings: PRINTINGS.filter((p) => wanted.printingIds.includes(p.id)),
        });
      }
      if (method === "POST" && path === "/v1/decks/commands/disposition") {
        const inner = (command["command"] ?? {}) as { requestId?: string };
        const known = outcomes.get(inner.requestId ?? "");
        return apiSuccess(
          known === undefined
            ? { state: "unknown", response: null }
            : (known as { status: string }).status === "success"
              ? { state: "accepted", response: known }
              : { state: "rejected", response: known },
        );
      }
      const remember = (reply: ApiReply) => {
        outcomes.set(String(command["requestId"]), reply.body);
        return reply;
      };
      if (method === "POST" && path === "/v1/decks") {
        const created: Deck = {
          id: makeUuid(nextId++),
          name: String(command["name"]),
          mode: command["mode"] === "open" ? "open" : "collection",
          format: command["formatId"] === MODERN.id ? MODERN : STANDARD,
          revision: null,
          appearance: { ...APPEARANCE },
        };
        decks.set(created.id, created);
        return remember(apiSuccess({ resourceId: created.id }));
      }
      if (method === "POST" && path === "/v1/decks/import/disposition") {
        const known = outcomes.get(String(command["requestId"]));
        return apiSuccess(
          known === undefined
            ? { state: "unknown", response: null }
            : (known as { status: string }).status === "success"
              ? { state: "accepted", response: known }
              : { state: "rejected", response: known },
        );
      }
      if (method === "POST" && path === "/v1/decks/import") {
        const revisionId = makeUuid(nextId++);
        const created: Deck = {
          id: makeUuid(nextId++),
          name: String(command["name"]),
          mode: command["mode"] === "open" ? "open" : "collection",
          format: command["formatId"] === MODERN.id ? MODERN : STANDARD,
          revision: {
            id: revisionId,
            number: "1",
            entries: command["entries"] as Entry[],
          },
          appearance: { ...APPEARANCE },
        };
        decks.set(created.id, created);
        const reply = remember(
          apiSuccess({ resourceId: created.id, revisionId }),
        );
        if (loseNextImportReply) {
          loseNextImportReply = false;
          return apiError("Lost.", 503);
        }
        return reply;
      }

      const [, deckId, action] =
        /^\/v1\/decks\/([^/]+)(?:\/(revisions|settings))?$/.exec(path) ?? [];
      const deck = decks.get(deckId ?? "");
      if (!deck) {
        return apiFail({ deck: "deck unavailable" }, 404);
      }
      if (method === "GET" && !action) {
        return apiSuccess({ deck: summary(deck), revision: deck.revision });
      }
      if (method === "POST" && action === "revisions") {
        if (command["expectedRevisionId"] !== (deck.revision?.id ?? null)) {
          return remember(
            apiFail({ revision: "deck changed; reload before saving" }, 409),
          );
        }
        deck.revision = {
          id: makeUuid(nextId++),
          number: String(Number(deck.revision?.number ?? 0) + 1),
          entries: command["entries"] as Entry[],
        };
        return remember(apiSuccess({ resourceId: deck.revision.id }));
      }
      if (method === "POST" && action === "settings") {
        const expected = command["expected"] as {
          name: string;
          formatId: string;
          appearance: typeof APPEARANCE;
        };
        const settings = command["settings"] as {
          name: string;
          formatId: string;
          appearance: typeof APPEARANCE;
        };
        if (
          expected.name !== deck.name ||
          expected.formatId !== deck.format.id ||
          JSON.stringify(expected.appearance) !== JSON.stringify(deck.appearance)
        ) {
          return remember(
            apiFail({ settings: "deck settings changed; reload before saving" }, 409),
          );
        }
        deck.appearance = settings.appearance;
        deck.name = settings.name;
        deck.format = settings.formatId === MODERN.id ? MODERN : STANDARD;
        return remember(apiSuccess({ resourceId: deck.id }));
      }

      return apiError("No such route.", 404);
    };

    // 1-2. Deck Builder save, format change, optimistic conflict refusal, and Create deck
    await runtime.withAuthedPage(
      { token: OWNER_TOKEN, subject: SUBJECT, answer },
      async ({ page, sent }) => {
        const saveAndSettle = async () => {
          const save = page.getByRole("button", { name: "Save", exact: true });
          const reread = page.waitForResponse(
            (response) =>
              response.url() === `${runtime.apiOrigin}/v1/decks/${GOBLINS}` &&
              response.request().method() === "GET",
          );
          await save.click();
          await reread;
          await save.and(page.locator(":disabled")).waitFor();
        };

        await runtime.runCheck(
          "deck builder: saves revision and format change, and refuses stale revision conflicts",
          async () => {
            await page.goto(`${runtime.origin}/decks/${GOBLINS}`);
            await page.getByRole("tab", { name: "Main 4" }).waitFor();
            await page
              .getByRole("searchbox", { name: "Search cards" })
              .fill("gob");
            await page.getByText("Goblin Guide").first().waitFor();
            await page
              .getByRole("button", { name: "Add Goblin Guide to Main" })
              .click();
            await page.getByRole("tab", { name: "Main 5" }).waitFor();
            await saveAndSettle();

            const revSaves = sent(`/v1/decks/${GOBLINS}/revisions`);
            assertCondition(
              revSaves.length === 1 &&
                revSaves[0]?.["expectedRevisionId"] === makeUuid(60),
              "Expected one revision save against revision 60",
            );

            await page.getByRole("combobox", { name: "Format" }).click();
            await page.getByRole("option", { name: "Modern" }).click();
            await saveAndSettle();
            const settingSaves = sent(`/v1/decks/${GOBLINS}/settings`);
            assertCondition(
              settingSaves.length === 1,
              "Expected one deck settings command",
            );

            // Simulate concurrent modification elsewhere
            const deck = decks.get(GOBLINS);
            if (deck?.revision) {
              deck.revision = { ...deck.revision, id: makeUuid(nextId++) };
            }
            await page
              .getByRole("button", { name: "Add Goblin Guide to Main" })
              .click();
            await page.getByRole("tab", { name: "Main 6" }).waitFor();
            await page.getByRole("button", { name: "Save", exact: true }).click();
            await page
              .getByText("This deck changed elsewhere. Reopen it before saving.")
              .waitFor();
          },
        );

        await runtime.runCheck(
          "deck shelf: Create deck creates one Untitled deck and opens empty Main 0",
          async () => {
            await page.goto(`${runtime.origin}/collection/decks`);
            await page
              .getByRole("button", { name: "Create deck" })
              .first()
              .click();
            await page.waitForURL(
              /\/decks\/00000000-0000-4000-8000-0000000001\d\d$/,
            );
            await page.getByRole("tab", { name: "Main 0" }).waitFor();
            assertCondition(
              sent("/v1/decks").length === 1 &&
                sent("/v1/decks")[0]?.["name"] === "Untitled",
              "Expected one Untitled deck creation command",
            );
          },
        );
      },
    );

    // 3. Deck Import recovery from dropped reply
    await runtime.withAuthedPage(
      { token: OWNER_TOKEN, subject: SUBJECT, answer },
      async ({ page, sent }) => {
        await runtime.runCheck(
          "deck import: recovers from dropped reply via disposition without duplicating deck",
          async () => {
            loseNextImportReply = true;
            const before = decks.size;
            await page.goto(`${runtime.origin}/collection/decks`);
            await page.getByRole("heading", { name: "Goblins" }).waitFor();
            await page.getByRole("button", { name: "Import deck" }).click();
            const dialog = page.getByRole("dialog", { name: "Import deck" });
            await dialog
              .getByRole("textbox", { name: "Deck list" })
              .fill(
                "4 Lightning Bolt (MSC) 806\n2 Goblin Guide\n1 Not A Real Card\n\nSideboard\n1 Krenko, Mob Boss",
              );
            await dialog
              .getByRole("button", { name: "Import deck", exact: true })
              .click();
            await page.waitForURL(
              /\/decks\/00000000-0000-4000-8000-0000000001\d\d$/,
            );
            await page.getByRole("tab", { name: "Main 6" }).waitFor();
            assertCondition(
              sent("/v1/decks/import").length === 1 &&
                sent("/v1/decks/import/disposition").length === 1 &&
                decks.size === before + 1,
              "Expected exactly one deck import and one disposition recovery",
            );
          },
        );
      },
    );

    // 4-5. Cards collection filtering & Booster Pack opening recovery
    await runtime.withAuthedPage(
      { token: OWNER_TOKEN, subject: SUBJECT, answer },
      async ({ page, sent }) => {
        await runtime.runCheck(
          "cards collection: lists owned printings and filters accurately by Zendikar set",
          async () => {
            await page.goto(`${runtime.origin}/collection/cards`);
            await page
              .getByRole("button", {
                name: "Lightning Bolt, 3 copies, MSC · 806",
              })
              .waitFor();
            assertCondition(
              (await page
                .getByRole("button", {
                  name: "Goblin Guide, 1 copy, ZEN · 126",
                })
                .count()) === 1,
              "Expected Goblin Guide printing visible in Cards",
            );
            await page.getByRole("button", { name: /^Filters/ }).click();
            const filters = page.getByRole("dialog", { name: "Filters" });
            await filters.getByRole("checkbox", { name: "Zendikar" }).check();
            await page
              .getByRole("button", { name: /^Lightning Bolt, / })
              .waitFor({ state: "detached" });
          },
        );

        await runtime.runCheck(
          "pack opening: recovers from dropped reply via Check opening, reveals cards, and decrements packs",
          async () => {
            await page.goto(`${runtime.origin}/collection/packs`);
            await page.getByText("2 packs").waitFor();
            loseNextOpenReply = true;
            holdOpenDisposition = true;
            await page
              .getByRole("button", { name: "Open a Foundations Play Booster" })
              .click();
            const stage = page.getByRole("dialog", {
              name: "Foundations Play Booster",
            });
            const checkButton = stage.getByRole("button", {
              name: "Check opening",
            });
            await checkButton.waitFor();
            const closeDisabled = await stage
              .getByRole("button", { name: "Close" })
              .isDisabled();
            assertCondition(
              closeDisabled,
              "Close must remain disabled while opening is unresolved",
            );

            holdOpenDisposition = false;
            await checkButton.click();
            await stage.getByRole("button", { name: "Reveal all" }).click();
            await stage.getByRole("button", { name: "Done" }).waitFor();
            await stage.getByRole("button", { name: "Done" }).click();
            await page.getByText("1 pack", { exact: true }).waitFor();
            assertCondition(
              sent("/v1/economy/commands").length === 1,
              "Pack open command must only be sent once",
            );
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
          "mobile 390x844: Collection Decks and Cards render with zero horizontal overflow",
          async () => {
            for (const [path, ready] of [
              ["/collection/decks", "Goblins"],
              ["/collection/cards", "Lightning Bolt"],
            ] as const) {
              await page.goto(`${runtime.origin}${path}`);
              await page.getByText(ready).first().waitFor();
              const overflow = await page.evaluate(
                () =>
                  document.documentElement.scrollWidth >
                  document.documentElement.clientWidth,
              );
              assertCondition(
                !overflow,
                `Horizontal overflow detected on ${path} at 390x844`,
              );
            }
          },
        );
      },
    );

    runtime.finish();
  },
);

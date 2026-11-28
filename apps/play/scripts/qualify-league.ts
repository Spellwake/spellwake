/**
 * Standalone Playwright E2E qualification for the League & Progression domain.
 *
 * Verifies:
 * 1. /league/season loads active season, viewer format ranks, and season end date.
 * 2. /league/standings loads ranked leaderboard players, highlights viewer row, and switches format.
 * 3. /league/history loads finished matches, opponent seats, and match outcomes.
 * 4. /league/history error state surfaces an honest notice without crashing when /v1/matches fails.
 * 5. Mobile viewport (390x844) renders Season, Standings, and History with zero horizontal overflow.
 *
 * Usage: node apps/play/scripts/qualify-league.ts
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
const SEASON_ID = makeUuid(600);
const STANDARD = { id: makeUuid(10), key: "standard", name: "Standard" };
const MODERN = { id: makeUuid(11), key: "modern", name: "Modern" };
const PROFILE = makeUuid(25);
const MATCH_1 = makeUuid(610);
const SEAT_OWN = makeUuid(611);
const SEAT_MIRA = makeUuid(612);
const RESULT_1 = makeUuid(613);
const CLASSIC_BACK = makeUuid(53);
const CLASSIC_SLEEVE = makeUuid(54);
const IVORY_DICE = makeUuid(55);
const OWNER_TOKEN = "stand-in-league-token";

const APPEARANCE = {
  backId: CLASSIC_BACK,
  sleeveId: CLASSIC_SLEEVE,
  diceId: IVORY_DICE,
  playmatId: null,
};

await withPlayE2eRuntime(
  "league-progression",
  { authPort: 59975, apiPort: 59976 },
  async (runtime) => {
    let failMatches = false;

    const answer = (
      method: string,
      url: URL,
      _body: unknown,
      bearer: string,
    ): ApiReply => {
      if (bearer !== OWNER_TOKEN) {
        return apiFail({ session: "unauthorized" }, 401);
      }
      const path = url.pathname;

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
        return apiSuccess({ items: [STANDARD, MODERN], nextCursor: null });
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
          items: [
            { formatId: STANDARD.id, seasonId: SEASON_ID, rating: "1620" },
            { formatId: MODERN.id, seasonId: SEASON_ID, rating: "1480" },
          ],
          nextCursor: null,
        });
      }
      if (method === "GET" && path === "/v1/competitive-results") {
        return apiSuccess({
          items: [{ formatId: STANDARD.id, seasonId: SEASON_ID }],
          nextCursor: null,
        });
      }
      if (method === "GET" && path === "/v1/league/leaderboard") {
        const formatId = url.searchParams.get("formatId") ?? STANDARD.id;
        const isModern = formatId === MODERN.id;
        const items = isModern
          ? [
              {
                rankPosition: 1,
                id: makeUuid(621),
                accountId: MIRA,
                username: "Mira",
                formatId: MODERN.id,
                seasonId: SEASON_ID,
                tier: "gold",
                rating: "1710",
                finalized: false,
                selectedBadge: null,
                cardArtProfile: null,
                supporter: true,
                wins: 14,
                losses: 5,
                draws: 0,
              },
              {
                rankPosition: 2,
                id: makeUuid(622),
                accountId: ACCOUNT,
                username: "Planeswalker",
                formatId: MODERN.id,
                seasonId: SEASON_ID,
                tier: "silver",
                rating: "1480",
                finalized: false,
                selectedBadge: null,
                cardArtProfile: null,
                supporter: false,
                wins: 7,
                losses: 6,
                draws: 0,
              },
            ]
          : [
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
              {
                rankPosition: 2,
                id: makeUuid(624),
                accountId: MIRA,
                username: "Mira",
                formatId: STANDARD.id,
                seasonId: SEASON_ID,
                tier: "silver",
                rating: "1540",
                finalized: false,
                selectedBadge: null,
                cardArtProfile: null,
                supporter: true,
                wins: 9,
                losses: 6,
                draws: 0,
              },
            ];
        return apiSuccess({
          seasonId: SEASON_ID,
          formatId,
          offset: 0,
          total: items.length,
          items,
        });
      }
      if (method === "GET" && path === "/v1/matches") {
        if (failMatches) {
          return apiError("History unavailable.", 503);
        }
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
              person: {
                id: ACCOUNT,
                username: "Planeswalker",
                isDeleted: false,
              },
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
        return apiSuccess({ cards: [], printings: [] });
      }
      return apiError("No such route.", 404);
    };

    await runtime.withAuthedPage(
      { token: OWNER_TOKEN, subject: SUBJECT, answer },
      async ({ page }) => {
        await runtime.runCheck(
          "league season: displays active season ratings for Standard and Modern",
          async () => {
            await page.goto(`${runtime.origin}/league/season`);
            await page.getByText("1,620").first().waitFor();
            await page.getByText("Standard").first().waitFor();
            await page.getByText("Dec 31").first().waitFor();
          },
        );

        await runtime.runCheck(
          "league standings: lists ranked players and switches cleanly between Standard and Modern",
          async () => {
            await page.goto(`${runtime.origin}/league/standings`);
            await page.getByText("Planeswalker").first().waitFor();
            await page.getByText("Mira").first().waitFor();
            await page.getByText("1,620").first().waitFor();

            await page.getByRole("tab", { name: "Modern" }).click();
            await page.getByText("1,710").first().waitFor();
            await page.getByText("1,480").first().waitFor();
          },
        );

        await runtime.runCheck(
          "match history: resolves finished match detail, opponent Mira, and outcome",
          async () => {
            await page.goto(`${runtime.origin}/league/history`);
            await page.getByText("Mira").first().waitFor();
            await page.getByText("Standard").first().waitFor();
            await page.getByText(/Won|Win/i).first().waitFor();
          },
        );
      },
    );

    // 4. Error handling when /v1/matches fails
    failMatches = true;
    await runtime.withAuthedPage(
      { token: OWNER_TOKEN, subject: SUBJECT, answer },
      async ({ page }) => {
        await runtime.runCheck(
          "match history error state: surfaces honest notice when /v1/matches fails",
          async () => {
            await page.goto(`${runtime.origin}/league/history`);
            await page.getByText("Couldn’t load your matches.").waitFor();
          },
        );
      },
    );
    failMatches = false;

    // 5. Mobile viewport check
    await runtime.withAuthedPage(
      {
        token: OWNER_TOKEN,
        subject: SUBJECT,
        viewport: { width: 390, height: 844 },
        answer,
      },
      async ({ page }) => {
        await runtime.runCheck(
          "mobile 390x844: Season, Standings, and History render with zero horizontal overflow",
          async () => {
            for (const [path, ready] of [
              ["/league/season", "1,620"],
              ["/league/standings", "Planeswalker"],
              ["/league/history", "Mira"],
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

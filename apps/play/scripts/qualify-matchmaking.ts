/**
 * Standalone Playwright browser E2E qualification for the Matchmaking & Lobby domain.
 *
 * Uses the shared loopback Playwright harness (`playwright-harness.ts`) with
 * in-memory stand-in Auth and Application API room/invitation endpoints.
 *
 * Covers 6 scenarios across desktop (1440x900) and mobile (390x844):
 * 1. Create room from deck, auto-select deck, toggle Ready -> countdown, and Close lobby cleanly.
 * 2. Copy invite link issues one invitation with token strictly in the URL fragment (#invite=).
 * 3. Invalid invitation token shows "Lobby unavailable" without seating the viewer.
 * 4. Valid invitation token seats the viewer (2/2), loads host Mira, and clears #invite from URL.
 * 5. Unknown/missing room renders "Lobby unavailable" cleanly.
 * 6. Mobile (390x844) lobby layout renders without horizontal overflow.
 *
 * Usage: node apps/play/scripts/qualify-matchmaking.ts
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
const GOBLINS = makeUuid(40);
const GOBLINS_REV = makeUuid(60);
const CLASSIC_BACK = makeUuid(53);
const CLASSIC_SLEEVE = makeUuid(54);
const IVORY_DICE = makeUuid(55);
const PROFILE = makeUuid(94);
const MIRA = makeUuid(95);
const CONFIG_ID = makeUuid(96);
const HOSTED_ROOM = makeUuid(310);
const FRIEND_ROOM = makeUuid(311);
const MISSING_ROOM = makeUuid(399);
const FRIEND_INVITE = `${"F".repeat(42)}Q`;
const WRONG_INVITE = `${"W".repeat(42)}A`;
const OWNER_TOKEN = "stand-in-matchmaking-access-token";

const APPEARANCE = {
  backId: CLASSIC_BACK,
  sleeveId: CLASSIC_SLEEVE,
  diceId: IVORY_DICE,
  playmatId: null,
};

type StandSlot = {
  id: string;
  accountId: string | null;
  seatIndex: string;
  controllerKind: "human" | "ai";
  ready: boolean;
  deckRevisionId: string | null;
};

type StandRoom = {
  id: string;
  hostId: string;
  formatId: string;
  mode: string;
  state: "lobby" | "countdown" | "closed";
  revision: number;
  countdownAt: string | null;
  slots: StandSlot[];
  seats?: number;
};

const standSlot = (n: number, accountId: string | null): StandSlot => ({
  id: makeUuid(n),
  accountId,
  seatIndex: String(n % 10),
  controllerKind: "human",
  ready: false,
  deckRevisionId: null,
});

const roomSummary = (room: StandRoom) => ({
  id: room.id,
  hostId: room.hostId,
  formatId: room.formatId,
  profileId: PROFILE,
  mode: room.mode,
  competitive: false,
  visibility: "private",
  state: room.state,
  revision: String(room.revision),
  structure: "bo1",
  configurationId: CONFIG_ID,
  targetSeatCount: String(room.seats ?? 2),
  countdownAt: room.countdownAt,
});

await withPlayE2eRuntime(
  "matchmaking",
  { authPort: 59985, apiPort: 59986 },
  async (runtime) => {
    const withLobbyScenario = async (
      options: {
        viewport?: { width: number; height: number };
        grantClipboard?: boolean;
      },
      fn: (harness: {
        page: Awaited<ReturnType<typeof runtime.browser.newPage>>;
        sent: (path: string) => Record<string, unknown>[];
        rooms: Map<string, StandRoom>;
        invitations: Map<string, string[]>;
      }) => Promise<void>,
    ): Promise<void> => {
      let nextId = 500;
      const rooms = new Map<string, StandRoom>([
        [
          HOSTED_ROOM,
          {
            id: HOSTED_ROOM,
            hostId: ACCOUNT,
            formatId: STANDARD.id,
            mode: "collection",
            state: "lobby",
            revision: 1,
            countdownAt: null,
            slots: [standSlot(320, ACCOUNT)],
          },
        ],
        [
          FRIEND_ROOM,
          {
            id: FRIEND_ROOM,
            hostId: MIRA,
            formatId: STANDARD.id,
            mode: "collection",
            state: "lobby",
            revision: 1,
            countdownAt: null,
            slots: [standSlot(330, MIRA), standSlot(331, null)],
          },
        ],
      ]);
      const invitations = new Map<string, string[]>([
        [FRIEND_ROOM, [FRIEND_INVITE]],
      ]);
      const outcomes = new Map<string, unknown>();

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
        if (method === "GET" && path === "/v1/table-profiles") {
          return apiSuccess({
            accountId: ACCOUNT,
            items: [
              {
                id: PROFILE,
                key: "free_for_all",
                minimumSeats: "2",
                seatCapacity: "4",
                hasMatchmaking: false,
              },
            ],
            nextCursor: null,
          });
        }
        if (method === "GET" && path === `/v1/players/${MIRA}/profile`) {
          return apiSuccess({
            viewerAccountId: ACCOUNT,
            person: { id: MIRA, username: "Mira", isDeleted: false },
            cardArtProfile: null,
            supporter: false,
          });
        }
        if (method === "POST" && path === "/v1/rooms/commands/disposition") {
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
        if (method === "POST" && path === "/v1/rooms/commands") {
          const kind = command["kind"];
          const inner = (command["command"] ?? {}) as Record<
            string,
            string | null
          >;
          const keep = (reply: ApiReply): ApiReply => {
            outcomes.set(String(inner["requestId"] ?? ""), reply.body);
            return reply;
          };
          if (kind === "create") {
            const room: StandRoom = {
              id: makeUuid(nextId++),
              hostId: ACCOUNT,
              formatId: String(inner["formatId"] ?? STANDARD.id),
              mode: String(inner["mode"] ?? "collection"),
              state: "lobby",
              revision: 1,
              countdownAt: null,
              slots: [
                {
                  id: makeUuid(300),
                  accountId: ACCOUNT,
                  seatIndex: "0",
                  controllerKind: "human",
                  ready: false,
                  deckRevisionId: null,
                },
                {
                  id: makeUuid(301),
                  accountId: MIRA,
                  seatIndex: "1",
                  controllerKind: "human",
                  ready: false,
                  deckRevisionId: null,
                },
              ],
            };
            rooms.set(room.id, room);
            return keep(apiSuccess({ resourceId: room.id }));
          }
          const room = rooms.get(String(inner["roomId"] ?? ""));
          if (!room) return keep(apiFail({ room: "unavailable" }));
          if (inner["expectedRevision"] !== String(room.revision)) {
            return keep(apiFail({ revision: "conflict" }));
          }
          room.revision += 1;
          if (kind === "select_deck") {
            const own = room.slots.find((s) => s.accountId === ACCOUNT);
            if (own) own.deckRevisionId = inner["deckRevisionId"] ?? null;
          } else {
            room.state = "closed";
          }
          return keep(apiSuccess({ resourceId: room.id }));
        }

        const roomPath =
          /^\/v1\/rooms\/([^/]+)(?:\/(ready|unready|configuration|invitations|join))?$/.exec(
            path,
          );
        if (roomPath) {
          const room = rooms.get(roomPath[1] ?? "");
          if (!room) return apiFail({ room: "unavailable" }, 404);
          const action = roomPath[2];
          if (method === "POST" && action === "invitations") {
            if (room.hostId !== ACCOUNT) {
              return apiFail({ room: "unavailable" });
            }
            if (command["expectedRevision"] !== String(room.revision)) {
              return apiFail({ revision: "conflict" });
            }
            const token = `${String(nextId++).padStart(42, "I")}A`;
            invitations.set(room.id, [
              ...(invitations.get(room.id) ?? []),
              token,
            ]);
            return apiSuccess({ resourceId: makeUuid(nextId++), token });
          }
          if (method === "POST" && action === "join") {
            const mine = room.slots.find((s) => s.accountId === ACCOUNT);
            if (mine) {
              return apiSuccess({ resourceId: room.id, slotId: mine.id });
            }
            if (
              !invitations.get(room.id)?.includes(String(command["token"] ?? ""))
            ) {
              return apiFail({ invitation: "unavailable" });
            }
            const open = room.slots.find((s) => s.accountId === null);
            if (!open) return apiFail({ capacity: "full" });
            open.accountId = ACCOUNT;
            room.revision += 1;
            return apiSuccess({ resourceId: room.id, slotId: open.id });
          }
          if (!room.slots.some((s) => s.accountId === ACCOUNT)) {
            return apiFail({ room: "unavailable" }, 404);
          }
          if (method === "GET" && !action) {
            return apiSuccess({
              accountId: ACCOUNT,
              room: roomSummary(room),
              slots: {
                items: room.slots.map((slot) => ({
                  ...slot,
                  deckRevisionId:
                    slot.accountId === ACCOUNT ? slot.deckRevisionId : null,
                })),
                nextCursor: null,
              },
            });
          }
          if (method === "GET" && action === "configuration") {
            return apiSuccess({
              accountId: ACCOUNT,
              roomId: room.id,
              revision: String(room.revision),
              profileId: PROFILE,
              structure: "bo1",
              teams: [],
            });
          }
          if (method === "POST" && (action === "ready" || action === "unready")) {
            if (command["expectedRevision"] !== String(room.revision)) {
              return apiFail({ revision: "conflict" });
            }
            room.revision += 1;
            const ready = action === "ready";
            for (const slot of room.slots) slot.ready = ready;
            room.state = ready ? "countdown" : "lobby";
            room.countdownAt = ready
              ? new Date(Date.now() + 60_000).toISOString()
              : null;
            const reply = apiSuccess({
              resourceId: room.id,
              revision: String(room.revision),
            });
            outcomes.set(String(command["requestId"] ?? ""), reply.body);
            return reply;
          }
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
            if (options.grantClipboard) {
              await context.grantPermissions([
                "clipboard-read",
                "clipboard-write",
              ]);
            }
          },
        },
        async ({ page, sent }) => {
          await fn({ page, sent, rooms, invitations });
        },
      );
    };

    await runtime.runCheck(
      "Play with friends creates a room with the selected deck, Ready enters countdown, and Back closes the lobby",
      async () => {
        await withLobbyScenario({}, async ({ page, sent, rooms }) => {
          await page.goto(`${runtime.origin}/collection/decks`);
          await page.getByRole("button", { name: "Play Goblins" }).click();
          await page
            .getByRole("dialog", { name: "Play Goblins" })
            .getByRole("button", { name: /^Play with friends/ })
            .click();
          await page.waitForURL(/\/collection\/lobby\?.*room=/);
          await page
            .getByRole("button", { name: /^Your deck: Goblins/ })
            .waitFor();
          await page.getByText("Mira", { exact: true }).waitFor();

          const commands = sent("/v1/rooms/commands");
          const create = commands[0];
          const chose = commands.find((b) => b["kind"] === "select_deck")?.[
            "command"
          ] as Record<string, unknown> | undefined;
          const roomId = String(chose?.["roomId"] ?? "");

          await page.getByRole("button", { name: "Ready", exact: true }).click();
          await page.getByText(/^Starting in \d+ seconds$/).waitFor();
          const [ready] = sent(`/v1/rooms/${roomId}/ready`);

          await page.getByRole("button", { name: "Back" }).click();
          const confirm = page.getByRole("dialog", { name: "Close lobby?" });
          await confirm
            .getByRole("button", { name: "Close lobby", exact: true })
            .click();
          await page.waitForURL(/\/collection\/decks$/);

          const leaves = sent("/v1/rooms/commands").filter(
            (b) => b["kind"] === "leave",
          );
          assertCondition(
            create?.["kind"] === "create",
            "Expected room create command",
          );
          assertCondition(
            chose?.["deckRevisionId"] === GOBLINS_REV,
            "Expected auto-selected Goblins deck revision",
          );
          assertCondition(
            ready?.["expectedRevision"] === "2",
            "Expected ready command at revision 2",
          );
          assertCondition(
            leaves.length === 1 && rooms.get(roomId)?.state === "closed",
            "Expected lobby close command to mark room closed",
          );
        });
      },
    );

    await runtime.runCheck(
      "Copy invite link issues one invitation with token strictly in #invite fragment and reuses it",
      async () => {
        await withLobbyScenario(
          { grantClipboard: true },
          async ({ page, sent, invitations }) => {
            await page.goto(
              `${runtime.origin}/collection/lobby?room=${HOSTED_ROOM}`,
            );
            const copy = page.getByRole("button", { name: "Copy invite link" });
            await copy.waitFor();
            await copy.click();
            await page.getByText("Invite link copied").first().waitFor();
            const first = await page.evaluate(() =>
              navigator.clipboard.readText(),
            );
            await copy.click();
            await page.waitForTimeout(350);
            const second = await page.evaluate(() =>
              navigator.clipboard.readText(),
            );
            const issued = sent(`/v1/rooms/${HOSTED_ROOM}/invitations`);
            const parsed = new URL(first);
            const token = invitations.get(HOSTED_ROOM)?.[0];
            assertCondition(
              issued.length === 1,
              `Expected exactly 1 invitation issued, got ${issued.length}`,
            );
            assertCondition(
              Boolean(token) && parsed.hash === `#invite=${token}`,
              `Invite token must ride in fragment only: ${first}`,
            );
            assertCondition(
              !parsed.search.includes(token ?? ""),
              "Invite token must never appear in URL query string",
            );
            assertCondition(
              second === first,
              "Second Copy invite link click should reuse the issued URL",
            );
          },
        );
      },
    );

    await runtime.runCheck(
      "invalid invitation token renders 'Lobby unavailable' without seating the viewer",
      async () => {
        await withLobbyScenario({}, async ({ page, sent, rooms }) => {
          await page.goto(
            `${runtime.origin}/collection/lobby?room=${FRIEND_ROOM}#invite=${WRONG_INVITE}`,
          );
          await page
            .getByRole("heading", { name: "Lobby unavailable" })
            .waitFor();
          assertCondition(
            sent(`/v1/rooms/${FRIEND_ROOM}/join`).length === 1,
            "Expected one join attempt with rejected token",
          );
          assertCondition(
            !rooms
              .get(FRIEND_ROOM)
              ?.slots.some((s) => s.accountId === ACCOUNT),
            "Rejected invite must not seat viewer",
          );
        });
      },
    );

    await runtime.runCheck(
      "valid invitation token seats viewer (2/2), loads host Mira, and clears #invite from URL",
      async () => {
        await withLobbyScenario({}, async ({ page, sent, rooms }) => {
          await page.goto(
            `${runtime.origin}/collection/lobby?room=${FRIEND_ROOM}#invite=${FRIEND_INVITE}`,
          );
          await page.getByText("Mira").first().waitFor();
          await page.getByText("2/2").waitFor();
          await page.waitForURL((u) => u.hash === "");
          const joins = sent(`/v1/rooms/${FRIEND_ROOM}/join`);
          assertCondition(
            joins.length === 1 && joins[0]?.["token"] === FRIEND_INVITE,
            "Expected join with exact invitation token",
          );
          assertCondition(
            Boolean(
              rooms
                .get(FRIEND_ROOM)
                ?.slots.some((s) => s.accountId === ACCOUNT),
            ),
            "Viewer should occupy the open seat",
          );
        });
      },
    );

    await runtime.runCheck(
      "unknown room renders 'Lobby unavailable' cleanly",
      async () => {
        await withLobbyScenario({}, async ({ page }) => {
          await page.goto(
            `${runtime.origin}/collection/lobby?room=${MISSING_ROOM}`,
          );
          await page
            .getByRole("heading", { name: "Lobby unavailable" })
            .waitFor();
        });
      },
    );

    await runtime.runCheck(
      "mobile viewport (390x844) renders hosted lobby without horizontal overflow",
      async () => {
        await withLobbyScenario(
          { viewport: { width: 390, height: 844 } },
          async ({ page }) => {
            await page.goto(
              `${runtime.origin}/collection/lobby?room=${HOSTED_ROOM}`,
            );
            const copy = page.getByRole("button", { name: "Copy invite link" });
            await copy.waitFor();
            const metrics = await page.evaluate(() => ({
              scrollWidth: document.documentElement.scrollWidth,
              clientWidth: document.documentElement.clientWidth,
            }));
            assertCondition(
              metrics.scrollWidth <= metrics.clientWidth,
              `Mobile viewport overflow detected: scrollWidth=${metrics.scrollWidth} > clientWidth=${metrics.clientWidth}`,
            );
            const box = await copy.boundingBox();
            assertCondition(
              box !== null && box.x >= 0 && box.x + box.width <= 390,
              `Copy invite link button clipped outside 390px viewport: ${JSON.stringify(box)}`,
            );
          },
        );
      },
    );

    runtime.finish();
  },
);

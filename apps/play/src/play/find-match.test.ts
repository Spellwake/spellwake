import { describe, expect, test } from "bun:test";
import {
  activeEntry,
  enqueueFailure,
  enqueueRequest,
  queueChoices,
  queueEntries,
  searchTerms,
} from "./find-match";

const id = (n: number) =>
  `${String(n).padStart(8, "0")}-0000-4000-8000-000000000000`;
const MODERN = id(1);
const COMMANDER = id(2);
const DUEL = id(11);
const FREE_FOR_ALL = id(12);
const PRIVATE_ONLY = id(13);

const profile = (profileId: string, hasMatchmaking: boolean) => ({
  id: profileId,
  key: "profile",
  minimumSeats: "2",
  seatCapacity: "4",
  hasMatchmaking,
});
const configuration = (
  index: number,
  key: string,
  formatId: string,
  seatCount: string,
  structure: "bo1" | "bo3",
) => ({ index: String(index), key, formatId, seatCount, structure });

const profiles = [
  profile(DUEL, true),
  profile(FREE_FOR_ALL, true),
  profile(PRIVATE_ONLY, false),
];
const configurations = new Map([
  [
    DUEL,
    [
      configuration(0, "modern-bo1", MODERN, "2", "bo1"),
      configuration(1, "modern-bo3", MODERN, "2", "bo3"),
    ],
  ],
  [FREE_FOR_ALL, [configuration(0, "commander-4", COMMANDER, "4", "bo1")]],
  [PRIVATE_ONLY, [configuration(0, "modern-private", MODERN, "2", "bo1")]],
]);

describe("the tables a format can be queued for", () => {
  test("are its configurations on matchmaking profiles, by match length", () => {
    expect(queueChoices(profiles, configurations, MODERN)).toEqual({
      single: [{ profileId: DUEL, configurationKey: "modern-bo1" }],
      three: [{ profileId: DUEL, configurationKey: "modern-bo3" }],
    });
    expect(queueChoices(profiles, configurations, COMMANDER)).toEqual({
      single: [{ profileId: FREE_FOR_ALL, configurationKey: "commander-4" }],
      three: [],
    });
  });

  test("are none for a format no matchmaking profile carries", () => {
    expect(queueChoices(profiles, configurations, id(9))).toEqual({
      single: [],
      three: [],
    });
  });
});

describe("the request that queues a deck", () => {
  const choices = [{ profileId: DUEL, configurationKey: "modern-bo1" }];

  test("asks for players only, and for rank only from Collection", () => {
    expect(
      enqueueRequest({
        requestId: id(50),
        mode: "collection",
        ranked: true,
        deckRevisionId: id(60),
        choices,
      }),
    ).toEqual({
      requestId: id(50),
      mode: "collection",
      competitive: true,
      alternatives: [
        {
          deckRevisionId: id(60),
          choices: [
            {
              profileId: DUEL,
              configurationKey: "modern-bo1",
              maximumAiSeats: "0",
              maximumSponsoredAiSeats: "0",
            },
          ],
        },
      ],
    });
    expect(
      enqueueRequest({
        requestId: id(50),
        mode: "open",
        ranked: true,
        deckRevisionId: id(60),
        choices,
      })?.competitive,
    ).toBe(false);
  });

  test("is not made where the contract would refuse it", () => {
    expect(
      enqueueRequest({
        requestId: id(50),
        mode: "open",
        ranked: false,
        deckRevisionId: id(60),
        choices: [],
      }),
    ).toBeNull();
    expect(
      enqueueRequest({
        requestId: id(50),
        mode: "open",
        ranked: false,
        deckRevisionId: "not-a-deck",
        choices,
      }),
    ).toBeNull();
  });
});

describe("the search an account has running", () => {
  const entry = (
    n: number,
    state: string,
    intentId: string | null = id(70),
  ) => ({
    id: id(n),
    intentId,
    deckRevisionId: id(60),
    mode: "open",
    competitive: false,
    alternativeIndex: intentId ? "0" : null,
    formatId: intentId ? MODERN : null,
    state,
    claimedRoomId: state === "claimed" || state === "reserved" ? id(80) : null,
    reservedMatchId: state === "reserved" ? id(81) : null,
  });

  test("is read only from entries the contract allows", () => {
    expect(queueEntries([entry(1, "queued"), { id: "x" }])).toHaveLength(1);
  });

  test("is the named request's entry, cancelled ones aside", () => {
    const entries = queueEntries([
      entry(1, "canceled"),
      entry(2, "queued", id(71)),
      entry(3, "reserved"),
    ]);
    expect(activeEntry(entries, id(70))?.id).toBe(id(3));
    expect(activeEntry(entries, id(72))).toBeNull();
  });

  test("is any entry still waiting or just matched where no request is named", () => {
    expect(
      activeEntry(
        queueEntries([entry(1, "reserved"), entry(2, "claimed")]),
        null,
      )?.id,
    ).toBe(id(2));
    expect(activeEntry(queueEntries([entry(1, "reserved")]), null)).toBeNull();
  });
});

describe("the dialog's words", () => {
  test("name the search's terms", () => {
    expect(searchTerms("Modern", "three", true)).toBe(
      "Modern · Best of three · Ranked",
    );
    expect(searchTerms("Modern", "single", false)).toBe("Modern · Single game");
  });

  test("say why a search could not start", () => {
    expect(enqueueFailure({ deck: "owned sealed revision required" })).toBe(
      "Save this deck before finding a match.",
    );
    expect(
      enqueueFailure({ choice: "unsupported format, profile or AI allowance" }),
    ).toBe("This match isn’t available for this format.");
    expect(enqueueFailure({ account: "unavailable" })).toBe(
      "Couldn’t start the search.",
    );
  });
});

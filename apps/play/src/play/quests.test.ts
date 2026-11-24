import { describe, expect, test } from "bun:test";
import {
  dailyWinPageSchema,
  questPageSchema,
} from "@spellwake/application-contracts";
import {
  questText,
  shownQuests,
  swapFailure,
  swappableQuest,
  winsToday,
} from "./quests";

const id = (n: number) =>
  `${String(n).padStart(8, "0")}-0000-4000-8000-000000000000`;
const TODAY = "2026-11-24";

const objective = (
  objectiveRef: string,
  target = "20",
  rewardCoins: "500" | "750" = "500",
) => ({
  definitionKey: "daily",
  revision: "v1",
  objectiveRef,
  target,
  rewardCoins,
});

function quest(
  n: number,
  over: {
    progress?: string;
    completedAt?: string | null;
    replacedById?: string | null;
    objectiveRef?: string | null;
    day?: string;
  } = {},
) {
  return {
    id: id(n),
    definitionId: id(100 + n),
    sourceDay: over.day ?? TODAY,
    assignedAt: `${over.day ?? TODAY}T00:00:05Z`,
    progress: over.progress ?? "0",
    completedAt: over.completedAt ?? null,
    replacedById: over.replacedById ?? null,
    objective:
      over.objectiveRef === null
        ? null
        : objective(over.objectiveRef ?? "native:cast-red-or-green"),
  };
}

function page(
  items: ReturnType<typeof quest>[],
  daily: {
    generatedAssignmentId: string | null;
    rerollsUsed: "0" | "1";
  } | null,
) {
  return questPageSchema.parse({
    accountId: id(1),
    utcDay: TODAY,
    daily: daily && { ...daily, revision: "1" },
    items,
    nextCursor: null,
  });
}

describe("what a quest asks", () => {
  test("is worded for the kinds the game counts", () => {
    expect(questText(objective("native:cast-red-or-green"))).toBe(
      "Cast 20 red or green spells",
    );
    expect(questText(objective("native:cast-black-or-green", "15"))).toBe(
      "Cast 15 black or green spells",
    );
  });

  test("is a plain quest for any other kind, with nothing invented", () => {
    expect(questText(objective("something-new"))).toBe("Daily quest");
  });
});

describe("the quests shown today", () => {
  test("are those still to do and those finished today", () => {
    const shown = shownQuests(
      page(
        [
          quest(1, { progress: "12" }),
          quest(2, { progress: "20", completedAt: "2026-11-24T09:00:00Z" }),
          quest(3, {
            day: "2026-11-23",
            progress: "20",
            completedAt: "2026-11-23T22:00:00Z",
          }),
        ],
        null,
      ),
    );
    expect(shown).toEqual([
      {
        id: id(1),
        text: "Cast 20 red or green spells",
        progress: 12,
        target: 20,
        rewardCoins: 500,
        done: false,
      },
      {
        id: id(2),
        text: "Cast 20 red or green spells",
        progress: 20,
        target: 20,
        rewardCoins: 500,
        done: true,
      },
    ]);
  });

  test("leave out a swapped quest and one whose objective cannot be read", () => {
    expect(
      shownQuests(
        page(
          [
            quest(1, { replacedById: id(2) }),
            quest(2),
            quest(3, { objectiveRef: null }),
          ],
          null,
        ),
      ).map((entry) => entry.id),
    ).toEqual([id(2)]);
  });
});

describe("the quest today's swap may be spent on", () => {
  test("is the one given today while the swap is unused", () => {
    expect(
      swappableQuest(
        page([quest(1), quest(2)], {
          generatedAssignmentId: id(2),
          rerollsUsed: "0",
        }),
      ),
    ).toBe(id(2));
  });

  test("is none once the swap is used, the quest is done, or none was given", () => {
    expect(
      swappableQuest(
        page([quest(2)], { generatedAssignmentId: id(2), rerollsUsed: "1" }),
      ),
    ).toBeNull();
    expect(
      swappableQuest(
        page(
          [quest(2, { progress: "20", completedAt: "2026-11-24T09:00:00Z" })],
          { generatedAssignmentId: id(2), rerollsUsed: "0" },
        ),
      ),
    ).toBeNull();
    expect(
      swappableQuest(
        page([quest(2)], { generatedAssignmentId: null, rerollsUsed: "0" }),
      ),
    ).toBeNull();
    expect(swappableQuest(page([quest(2)], null))).toBeNull();
  });
});

describe("today's wins", () => {
  const wins = dailyWinPageSchema.parse({
    accountId: id(1),
    items: [
      {
        id: id(5),
        utcDay: "2026-11-23",
        wins: "4",
        issuedCoins: "400",
        revision: "4",
      },
      {
        id: id(6),
        utcDay: TODAY,
        wins: "2",
        issuedCoins: "200",
        revision: "2",
      },
    ],
    nextCursor: null,
  });

  test("are the day's own count and coins", () => {
    expect(winsToday(wins, TODAY)).toEqual({ count: 2, coins: 200 });
  });

  test("are none on a day with nothing won yet", () => {
    expect(winsToday(wins, "2026-11-25")).toEqual({ count: 0, coins: 0 });
  });
});

describe("why a swap did not happen", () => {
  test("is said in the row's words", () => {
    expect(swapFailure({ reroll: "already used" })).toBe(
      "Today’s swap is already used.",
    );
    expect(
      swapFailure({ definition: "different available definition required" }),
    ).toBe("No other quest is available today.");
    expect(swapFailure({ day: "not current UTC day" })).toBe(
      "This quest can no longer be swapped.",
    );
    expect(swapFailure({ account: "unavailable" })).toBe(
      "Couldn’t swap the quest.",
    );
  });
});

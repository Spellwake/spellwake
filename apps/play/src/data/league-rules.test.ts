import { describe, expect, test } from "bun:test";
import { previewEntriesOf } from "../league/demo-league";
import {
  competitionPlaces,
  progressOf,
  recordLong,
  recordShort,
  rewardOf,
  seasonOf,
  tierOf,
} from "../league/league-sources";

const record = (wins: number, losses: number, draws = 0) => ({
  wins,
  losses,
  draws,
});

describe("ranks", () => {
  test("tiers follow the numeric bounds", () => {
    expect(
      [0, 1099, 1100, 1300, 1500, 1700, 1899, 1900, 2400].map(
        (rating) => tierOf(rating).name,
      ),
    ).toEqual([
      "Bronze",
      "Bronze",
      "Silver",
      "Gold",
      "Platinum",
      "Diamond",
      "Diamond",
      "Mythic",
      "Mythic",
    ]);
  });

  test("progress spans the tier, so a starting rating is not empty", () => {
    expect(progressOf(1000)).toEqual({
      highest: false,
      lowerBound: 0,
      nextBound: 1100,
      label: "100 to Silver",
    });
    expect(progressOf(1240).label).toBe("60 to Gold");
    expect(progressOf(1820).label).toBe("80 to Mythic");
    expect(progressOf(2040)).toEqual({ highest: true, label: "Highest rank" });
  });

  test("a season reward is the current tier's, and none before playing", () => {
    expect(rewardOf(1040, record(4, 6))).toBe(0);
    expect(rewardOf(1240, record(12, 8))).toBe(250);
    expect(rewardOf(2040, record(12, 7, 1))).toBe(1500);
    expect(rewardOf(1120, record(0, 0))).toBeNull();
  });

  test("records read in words and in short, with draws only when there are any", () => {
    expect(recordLong(record(12, 7, 1))).toBe("12 wins · 7 losses · 1 draw");
    expect(recordLong(record(1, 1))).toBe("1 win · 1 loss");
    expect(recordLong(record(0, 0))).toBe("No matches");
    expect(recordShort(record(40, 12))).toBe("40–12");
    expect(recordShort(record(42, 19, 1))).toBe("42–19–1");
  });

  test("the season is the UTC quarter", () => {
    expect(seasonOf(new Date("2026-11-22T12:00:00Z"))).toEqual({
      label: "Oct–Dec 2026",
      ends: "Jan 1",
    });
    expect(seasonOf(new Date("2027-02-28T23:59:59Z"))).toEqual({
      label: "Jan–Mar 2027",
      ends: "Apr 1",
    });
  });
});

describe("standings", () => {
  test("equal ratings share a place and the next place skips", () => {
    expect(competitionPlaces([2240, 2240, 1420, 1420, 1420, 1000])).toEqual([
      1, 1, 3, 3, 3, 6,
    ]);
  });

  test("you sit where your rating puts you, and not at all before playing", () => {
    const modern = previewEntriesOf("Modern");
    const at = modern.findIndex((entry) => entry.isYou);
    expect(modern[at - 1]!.rating).toBeGreaterThanOrEqual(1240);
    expect(modern[at + 1]!.rating).toBeLessThan(1240);
    expect(previewEntriesOf("Pioneer").some((entry) => entry.isYou)).toBe(
      false,
    );
    expect(previewEntriesOf("Premodern")).toEqual([]);
  });
});

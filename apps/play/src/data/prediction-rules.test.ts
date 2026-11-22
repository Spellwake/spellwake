import { describe, expect, test } from "bun:test";
import { publishPredictionSchema } from "@spellwake/application-contracts";
import { betFromPositions } from "../market/connected-predictions";
import { previewSideQuote } from "../market/demo-predictions";
import {
  amountProblem,
  betStatus,
  canonicalTarget,
  dateProblem,
  entryOf,
  matchPredictions,
  publicationTimes,
  questionOf,
  questionStatus,
  returnedOf,
  scheduleFrom,
  targetProblem,
  usd,
  type Prediction,
} from "../market/prediction-sources";

const base: Prediction = {
  id: "p",
  card: {
    printingId: "x",
    name: "Lightning Bolt",
    setCode: "M11",
    finish: "Nonfoil",
    faceUrl: null,
  },
  targetUsd: "3.50",
  condition: "reached_by",
  deadline: "2027-01-31",
  closesAt: "2026-11-13T12:00:00Z",
  state: "open",
  accepting: true,
  outcome: null,
  evidence: null,
  voidReason: null,
  mine: { kind: "none" },
  authored: false,
};
const bet = { kind: "bet" as const, side: "yes" as const, cost: 200, win: 500 };

describe("the question", () => {
  test("names the card, the price and the end date as the cards do", () => {
    expect(questionOf(base)).toBe("Will Lightning Bolt reach $3.50 by Jan 31?");
    expect(
      questionOf({ ...base, condition: "at_deadline", targetUsd: "4" }),
    ).toBe("Will Lightning Bolt be at least $4 on Jan 31?");
    expect(usd("2.0")).toBe("$2");
    expect(usd("0.125")).toBe("$0.125");
    expect(usd("3.5")).toBe("$3.50");
  });

  test("search matches the card, its set and the question", () => {
    const sol = {
      ...base,
      id: "s",
      targetUsd: "2",
      card: { ...base.card, name: "Sol Ring", setCode: "CMM" },
    };
    expect(matchPredictions([base, sol], "ring").map((p) => p.id)).toEqual([
      "s",
    ]);
    expect(matchPredictions([base, sol], "m11").map((p) => p.id)).toEqual([
      "p",
    ]);
    expect(matchPredictions([base, sol], "$3.50").map((p) => p.id)).toEqual([
      "p",
    ]);
  });
});

describe("one bet per prediction", () => {
  test("a card offers a bet only while betting and none is placed", () => {
    expect(entryOf(base)).toBe("bet");
    expect(entryOf({ ...base, mine: bet })).toBe("view");
    expect(entryOf({ ...base, mine: { kind: "unverified" } })).toBe("view");
    expect(entryOf({ ...base, accepting: false })).toBe("view");
    expect(entryOf({ ...base, state: "resolved", outcome: "no" })).toBe("view");
  });

  test("a bet's status and return follow the public outcome and its side", () => {
    expect(betStatus(base, bet)).toBe("Active");
    expect(betStatus({ ...base, state: "closed", accepting: false }, bet)).toBe(
      "Awaiting result",
    );
    const resolved = { ...base, state: "resolved" as const };
    expect(betStatus({ ...resolved, outcome: "yes" }, bet)).toBe("Won");
    expect(betStatus({ ...resolved, outcome: "no" }, bet)).toBe("Lost");
    expect(betStatus({ ...resolved, outcome: "void" }, bet)).toBe("Void");
    expect(returnedOf(bet, "yes")).toBe(500);
    expect(returnedOf(bet, "no")).toBe(0);
    expect(returnedOf(bet, "void")).toBe(200);
  });

  test("a creator's question shows its own status, never winnings", () => {
    expect(questionStatus(base)).toBe("Betting open");
    expect(questionStatus({ ...base, state: "resolved", outcome: "no" })).toBe(
      "No",
    );
  });

  test("positions make one bet, or none is claimed", () => {
    expect(betFromPositions([])).toEqual({ kind: "none" });
    expect(
      betFromPositions([
        { side: "no", quantity: "2", actualCost: "120" },
        { side: "no", quantity: "1", actualCost: "65" },
      ]),
    ).toEqual({ kind: "bet", side: "no", cost: 185, win: 300 });
    expect(
      betFromPositions([
        { side: "no", quantity: "1", actualCost: "60" },
        { side: "yes", quantity: "1", actualCost: "40" },
      ]),
    ).toEqual({ kind: "unverified" });
    expect(
      betFromPositions([
        { side: "yes", quantity: "1", actualCost: "9".repeat(20) },
      ]),
    ).toEqual({ kind: "unverified" });
  });
});

describe("betting amounts", () => {
  test("a bet is a positive whole number the wallet covers", () => {
    expect(amountProblem("", 8000)).toBe("Enter an amount.");
    expect(amountProblem("0", 8000)).toBe("Enter an amount.");
    expect(amountProblem("200.5", 8000)).toBe("Enter a whole number.");
    expect(amountProblem("-3", 8000)).toBe("Enter a whole number.");
    expect(amountProblem("9000", 8000)).toBe("Not enough coins.");
    expect(amountProblem("200", 8000)).toBeNull();
  });

  test("the preview takes the whole amount or names why not", () => {
    const liquidity = { yes: 40, no: null, limit: 3000 };
    expect(previewSideQuote(liquidity, "yes", 200)).toMatchObject({
      kind: "ready",
      win: 500,
    });
    expect(previewSideQuote(liquidity, "no", 200).kind).toBe("unavailable");
    expect(previewSideQuote(liquidity, "yes", 3001).kind).toBe("too-much");
  });
});

describe("creating a prediction", () => {
  test("a target is a positive decimal of dollars", () => {
    for (const bad of ["", "0", "0.00", "abc", "-1", "1.2.3"])
      expect(targetProblem(bad)).toBe("Enter a price above $0.");
    expect(targetProblem("3.50")).toBeNull();
    expect(targetProblem("$0.01")).toBeNull();
    expect(canonicalTarget("$03.50")).toBe("3.50");
  });

  test("betting closes in seven days; the end date follows the next full day", () => {
    const schedule = scheduleFrom(new Date("2026-11-06T15:30:12.345Z"));
    expect(schedule.closesAt).toBe("2026-11-13T15:30:12Z");
    expect(schedule.earliest).toBe("2026-11-14");
    expect(dateProblem("2026-11-13", schedule)).toBe("Choose Nov 14 or later.");
    expect(dateProblem("2026-11-14", schedule)).toBeNull();
    expect(dateProblem(schedule.latest, schedule)).toBeNull();
    expect(dateProblem("2027-12-31", schedule)).toBe(
      "Choose Nov 13, 2027 or earlier.",
    );
  });

  test("the publication terms are ones the contract accepts, at the latest date too", () => {
    const schedule = scheduleFrom(new Date("2026-11-06T15:30:12Z"));
    for (const day of [schedule.earliest, schedule.latest]) {
      const parsed = publishPredictionSchema.safeParse({
        requestId: "00000000-0000-4000-8000-000000000001",
        printingId: "00000000-0000-4000-8000-000000000002",
        probeId: "00000000-0000-4000-8000-000000000003",
        targetUsd: "3.50",
        condition: "reached_by",
        ...publicationTimes(day, schedule),
      });
      expect(parsed.success).toBe(true);
    }
  });
});

import { describe, expect, test } from "bun:test";
import {
  playerChoiceSchema,
  type PlayerDomain,
  type PlayerTarget,
} from "@spellwake/application-contracts";
import {
  attackCostPayment,
  counterLeft,
  counterReply,
  counterStep,
  modesReady,
  modesReply,
  modeTargetOpen,
  moved,
  preventionLeft,
  preventionReply,
  preventionStep,
  targetListReady,
  toggledMode,
  toggledModeTarget,
  toggledTarget,
  type CounterDraft,
  type ModesDraft,
  type PreventionDraft,
} from "./decision-drafts";

type Domain<Kind extends PlayerDomain["kind"]> = Extract<
  PlayerDomain,
  { kind: Kind }
>;
const object = (handle: string): PlayerTarget => ({ kind: "object", handle });
const accepted = (reply: unknown): boolean =>
  playerChoiceSchema.safeParse(reply).success;

describe("ordering", () => {
  test("moves one item and leaves the rest in order", () => {
    expect(moved(["a", "b", "c"], 2, 0)).toEqual(["c", "a", "b"]);
    expect(moved(["a", "b", "c"], 0, 1)).toEqual(["b", "a", "c"]);
  });
  test("a move past either end changes nothing", () => {
    expect(moved(["a", "b"], 0, -1)).toEqual(["a", "b"]);
    expect(moved(["a", "b"], 1, 2)).toEqual(["a", "b"]);
  });
});

describe("a list of targets", () => {
  const domain: Domain<"target-list"> = {
    kind: "target-list",
    targets: [object("a"), object("b"), object("c")],
    minimum: "1",
    maximum: "2",
  };
  test("holds no more than the most allowed, and only offered targets", () => {
    let chosen: PlayerTarget[] = [];
    for (const handle of ["a", "b", "c", "z"])
      chosen = toggledTarget(domain, chosen, object(handle));
    expect(chosen).toEqual([object("a"), object("b")]);
    expect(targetListReady(domain, chosen)).toBe(true);
    expect(accepted({ kind: "target-list", targets: chosen })).toBe(true);
  });
  test("is not ready below the least, and a second press takes a target away", () => {
    expect(targetListReady(domain, [])).toBe(false);
    expect(toggledTarget(domain, [object("a")], object("a"))).toEqual([]);
  });
});

describe("removing counters", () => {
  const domain: Domain<"counter-removal"> = {
    kind: "counter-removal",
    permanent: "p",
    maximum: 3,
    available: [
      { kind: "plus-one", count: 2 },
      { kind: "charge", count: 4 },
    ],
  };
  test("never exceeds what the permanent has or the most that may go", () => {
    let draft: CounterDraft = {};
    for (let i = 0; i < 5; i += 1)
      draft = counterStep(domain, draft, "plus-one", 1);
    for (let i = 0; i < 5; i += 1)
      draft = counterStep(domain, draft, "charge", 1);
    expect(draft).toEqual({ "plus-one": 2, charge: 1 });
    expect(counterLeft(domain, draft)).toBe(0);
    const reply = counterReply(draft);
    expect(accepted(reply)).toBe(true);
    expect(reply.counters.reduce((sum, entry) => sum + entry.count, 0)).toBe(3);
  });
  test("a kind stepped back to none is left out of the answer", () => {
    const draft = counterStep(
      domain,
      counterStep(domain, {}, "charge", 1),
      "charge",
      -1,
    );
    expect(counterReply(draft).counters).toEqual([]);
    expect(counterStep(domain, {}, "charge", -1)).toEqual({});
  });
});

describe("preventing damage", () => {
  const domain: Domain<"prevention"> = {
    kind: "prevention",
    attempts: [
      { attempt: "x", maximum: "2", label: "Bolt on Guide" },
      { attempt: "y", maximum: "5", label: "Shock on you" },
    ],
    required: "3",
  };
  test("places exactly the required amount within each attempt's most", () => {
    let draft: PreventionDraft = {};
    for (let i = 0; i < 4; i += 1)
      draft = preventionStep(domain, draft, "x", 1);
    for (let i = 0; i < 4; i += 1)
      draft = preventionStep(domain, draft, "y", 1);
    expect(draft).toEqual({ x: 2, y: 1 });
    expect(preventionLeft(domain, draft)).toBe(0);
    const reply = preventionReply(draft);
    expect(accepted(reply)).toBe(true);
    expect(reply.allocations).toEqual([
      { attempt: "x", amount: "2" },
      { attempt: "y", amount: "1" },
    ]);
  });
  test("says how much is still to place", () => {
    expect(preventionLeft(domain, { x: 1 })).toBe(2);
  });
});

describe("a trigger's modes", () => {
  const domain: Domain<"trigger-modes"> = {
    kind: "trigger-modes",
    minimum: 1,
    maximum: 2,
    distinctTargets: true,
    modes: [
      { index: 0, targetRequired: true, targets: [object("a"), object("b")] },
      { index: 1, targetRequired: false, targets: [] },
      {
        index: 2,
        targetRequired: true,
        multiple: [1, 2],
        targets: [object("a"), object("b"), object("c")],
      },
    ],
  };
  test("is ready only once each chosen mode has the targets it takes", () => {
    let draft: ModesDraft = toggledMode(domain, [], 0);
    expect(modesReady(domain, draft)).toBe(false);
    draft = toggledModeTarget(domain, draft, 0, object("a"));
    expect(modesReady(domain, draft)).toBe(true);
    expect(modesReply(domain, draft)).toEqual({
      kind: "trigger-modes",
      modes: [{ mode: 0, target: object("a") }],
    });
    expect(accepted(modesReply(domain, draft))).toBe(true);
  });
  test("a single target swaps, and a mode with none sends none", () => {
    let draft: ModesDraft = toggledMode(domain, [], 0);
    draft = toggledModeTarget(domain, draft, 0, object("a"));
    draft = toggledModeTarget(domain, draft, 0, object("b"));
    draft = toggledMode(domain, draft, 1);
    expect(modesReply(domain, draft).modes).toEqual([
      { mode: 0, target: object("b") },
      { mode: 1, target: null },
    ]);
    expect(accepted(modesReply(domain, draft))).toBe(true);
  });
  test("holds no more modes than allowed, and no target twice where they must differ", () => {
    let draft: ModesDraft = toggledMode(domain, [], 0);
    draft = toggledMode(domain, draft, 2);
    expect(toggledMode(domain, draft, 1)).toEqual(draft);
    draft = toggledModeTarget(domain, draft, 0, object("a"));
    expect(modeTargetOpen(domain, draft, 2, object("a"))).toBe(false);
    draft = toggledModeTarget(domain, draft, 2, object("a"));
    draft = toggledModeTarget(domain, draft, 2, object("b"));
    draft = toggledModeTarget(domain, draft, 2, object("c"));
    expect(modesReply(domain, draft).modes).toEqual([
      { mode: 0, target: object("a") },
      { mode: 2, target: null, targets: [object("b"), object("c")] },
    ]);
    expect(modesReady(domain, draft)).toBe(true);
    expect(accepted(modesReply(domain, draft))).toBe(true);
  });
  test("a second press takes a mode away with its targets", () => {
    const draft = toggledModeTarget(
      domain,
      toggledMode(domain, [], 0),
      0,
      object("a"),
    );
    expect(toggledMode(domain, draft, 0)).toEqual([]);
  });
});

describe("an attack cost", () => {
  const domain: Domain<"attack-cost"> = {
    kind: "attack-cost",
    generic: "2",
    whitePhyrexian: "1",
  };
  test("pays the white symbol with mana and the rest from other colours first", () => {
    const reply = attackCostPayment(
      domain,
      [
        { color: "white", amount: 2 },
        { color: "red", amount: 1 },
      ],
      0,
    );
    expect(reply).toEqual({
      kind: "pay-attack-cost",
      lifeSymbols: "0",
      payment: {
        spend: [
          { color: "white", amount: "2" },
          { color: "red", amount: "1" },
        ],
      },
    });
    expect(accepted(reply)).toBe(true);
  });
  test("pays the white symbol with life when asked", () => {
    const reply = attackCostPayment(domain, [{ color: "red", amount: 2 }], 1);
    expect(reply?.lifeSymbols).toBe("1");
    expect(reply?.payment.spend).toEqual([{ color: "red", amount: "2" }]);
    expect(accepted(reply)).toBe(true);
  });
  test("is not offered when the pool cannot cover it", () => {
    expect(
      attackCostPayment(domain, [{ color: "red", amount: 2 }], 0),
    ).toBeNull();
    expect(
      attackCostPayment(domain, [{ color: "red", amount: 1 }], 1),
    ).toBeNull();
    expect(attackCostPayment(domain, [], 2)).toBeNull();
  });
});

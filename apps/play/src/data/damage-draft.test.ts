import { describe, expect, test } from "bun:test";
import {
  playerSnapshotSchema,
  validPlayerChoice,
} from "@spellwake/application-contracts";
import {
  adjustDamage,
  amountsOf,
  canConfirmDamage,
  canMinus,
  canPlus,
  damageAssignments,
  reconcileDamage,
  selectSource,
  sourceComplete,
  startDamage,
  suggestDamage,
  type DamageDomain,
} from "../match/damage-draft";
import {
  PREVIEW_SEATS,
  previewResolveTop,
  previewScene,
  type PreviewScene,
} from "../match/preview-table";

const object = (handle: string) => ({ kind: "object" as const, handle });
const alex = PREVIEW_SEATS[1].seatId;

/** One attacker with four damage, blocked by two creatures. */
const split: DamageDomain = {
  kind: "combat-damage",
  sources: [
    { source: "angel", targets: [object("a"), object("b")], total: "4" },
  ],
};
/** Two attackers, each blocked by two creatures of a different player. */
const two: DamageDomain = {
  kind: "combat-damage",
  sources: [
    { source: "angel", targets: [object("a"), object("b")], total: "4" },
    { source: "bears", targets: [object("c"), object("d")], total: "2" },
  ],
};
/** A six-power trampler blocked by a creature that four damage destroys. */
const trample: DamageDomain = {
  kind: "combat-damage",
  sources: [
    {
      source: "dreadmaw",
      targets: [object("a"), { kind: "seat", seatId: alex }],
      total: "6",
      trampleLethal: [{ blocker: "a", amount: "4" }],
    },
  ],
};
const press = (
  domain: DamageDomain,
  presses: readonly (readonly [
    source: string,
    target: number,
    delta: 1 | -1,
  ])[],
  from = startDamage(domain),
) =>
  presses.reduce(
    (draft, [source, target, delta]) =>
      adjustDamage(draft, domain, source, target, delta),
    from,
  );
const amounts = (
  domain: DamageDomain,
  draft: ReturnType<typeof startDamage>,
  source = 0,
) => [...amountsOf(draft, domain.sources[source]!)];

describe("the suggested split", () => {
  test("puts everything on the first target, ready to confirm", () => {
    const draft = startDamage(split);
    expect(amounts(split, draft)).toEqual([4, 0]);
    expect(draft.active).toBe("angel");
    expect(canConfirmDamage(draft, split)).toBe(true);
  });

  test("a trampler gives each blocker lethal and the player the rest", () => {
    expect(suggestDamage(trample.sources[0]!)).toEqual([4, 2]);
    // Short of lethal, nothing reaches the player.
    expect(suggestDamage({ ...trample.sources[0]!, total: "3" })).toEqual([
      3, 0,
    ]);
  });
});

describe("pressing minus and plus", () => {
  test("plus on a full split moves one point from another target (DA-02)", () => {
    const once = press(split, [["angel", 1, 1]]);
    expect(amounts(split, once)).toEqual([3, 1]);
    expect(amounts(split, press(split, [["angel", 1, 1]], once))).toEqual([
      2, 2,
    ]);
  });

  test("minus leaves a point unassigned, and Confirm waits for it (DA-03)", () => {
    const draft = press(split, [["angel", 0, -1]]);
    expect(amounts(split, draft)).toEqual([3, 0]);
    expect(canConfirmDamage(draft, split)).toBe(false);
    // Plus takes from the unassigned point before any other target.
    expect(amounts(split, press(split, [["angel", 1, 1]], draft))).toEqual([
      3, 1,
    ]);
    expect(amounts(split, press(split, [["angel", 0, 1]], draft))).toEqual([
      4, 0,
    ]);
  });

  test("minus stops at zero and plus stops when nothing can be spared", () => {
    const draft = startDamage(split);
    const source = split.sources[0]!;
    expect(canMinus(draft, source, 1)).toBe(false);
    expect(canPlus(draft, source, 0)).toBe(false);
    expect(press(split, [["angel", 1, -1]], draft)).toBe(draft);
    expect(press(split, [["angel", 0, 1]], draft)).toBe(draft);
  });

  test("the last target that can spare a point gives it", () => {
    const three: DamageDomain = {
      kind: "combat-damage",
      sources: [
        {
          source: "angel",
          targets: [object("a"), object("b"), object("c")],
          total: "4",
        },
      ],
    };
    const second = press(three, [["angel", 1, 1]]);
    expect(amounts(three, second)).toEqual([3, 1, 0]);
    // Raising the third takes from the second, the last with a point to spare.
    expect(amounts(three, press(three, [["angel", 2, 1]], second))).toEqual([
      3, 0, 1,
    ]);
  });
});

describe("several sources", () => {
  test("each keeps its own split, and no point crosses between them (DA-04 to DA-06)", () => {
    const start = startDamage(two);
    const bears = selectSource(start, two, "bears");
    expect(bears.active).toBe("bears");
    const edited = press(two, [["bears", 1, 1]], bears);
    expect(amounts(two, edited, 0)).toEqual([4, 0]);
    expect(amounts(two, edited, 1)).toEqual([1, 1]);
    // Going back to the first source keeps both.
    const back = selectSource(edited, two, "angel");
    expect(amounts(two, back, 1)).toEqual([1, 1]);
    expect(canConfirmDamage(back, two)).toBe(true);
  });

  test("Confirm waits while any source is incomplete", () => {
    const draft = press(two, [["bears", 0, -1]]);
    expect(sourceComplete(draft, two.sources[0]!)).toBe(true);
    expect(sourceComplete(draft, two.sources[1]!)).toBe(false);
    expect(canConfirmDamage(draft, two)).toBe(false);
  });

  test("a source that is not offered is never selected", () => {
    const draft = startDamage(two);
    expect(selectSource(draft, two, "elsewhere")).toBe(draft);
  });
});

describe("trample (DA-07)", () => {
  const source = trample.sources[0]!;
  test("the player may take more only from damage beyond lethal", () => {
    const draft = startDamage(trample);
    // The blocker holds exactly lethal, so nothing can move to the player.
    expect(canPlus(draft, source, 1)).toBe(false);
    // More may go back onto the blocker from the player.
    expect(amounts(trample, press(trample, [["dreadmaw", 0, 1]]))).toEqual([
      5, 1,
    ]);
  });

  test("a split that tramples over less than lethal cannot be confirmed", () => {
    const under = press(trample, [["dreadmaw", 0, -1]]);
    expect(amounts(trample, under)).toEqual([3, 2]);
    expect(canConfirmDamage(under, trample)).toBe(false);
    // The freed point may only go back onto the blocker.
    expect(canPlus(under, source, 1)).toBe(false);
    expect(canPlus(under, source, 0)).toBe(true);
  });
});

describe("what Confirm sends", () => {
  /** Whether the player-view contract accepts a scene's split as its answer. */
  const accepted = (
    scene: PreviewScene,
    presses: Parameters<typeof press>[1] = [],
  ): boolean => {
    const table = previewScene(scene);
    const decision = table.decisions[0];
    if (decision?.domain.kind !== "combat-damage") return false;
    const domain = decision.domain;
    const named = presses.map(
      ([at, target, delta]) =>
        [domain.sources[Number(at)]!.source, target, delta] as const,
    );
    return validPlayerChoice(
      decision,
      {
        requestId: "00000000-0000-4000-8000-000000000301",
        basis: decision.basis,
        action: {
          kind: "choose",
          decision: decision.decision,
          response: {
            kind: "combat-damage",
            assignments: damageAssignments(press(domain, named), domain),
          },
        },
      },
      table.viewerSeatId,
      table.players.map((player) => player.seatId),
    );
  };

  test("every source together, with no row for a target given nothing", () => {
    const draft = press(two, [["bears", 1, 1]]);
    expect(damageAssignments(draft, two)).toEqual([
      { source: "angel", target: object("a"), amount: "4" },
      { source: "bears", target: object("c"), amount: "1" },
      { source: "bears", target: object("d"), amount: "1" },
    ]);
  });

  test("the suggested and the adjusted splits are ones the contract accepts", () => {
    expect(accepted("damage")).toBe(true);
    expect(accepted("damage", [["0", 1, 1]])).toBe(true);
    expect(accepted("damage-two", [["1", 1, 1]])).toBe(true);
    expect(accepted("trample")).toBe(true);
    expect(accepted("trample", [["0", 0, 1]])).toBe(true);
  });

  test("a split left incomplete is one the contract refuses", () => {
    expect(accepted("damage", [["0", 0, -1]])).toBe(false);
  });
});

describe("the game changing under the same decision", () => {
  test("a source offered as before keeps its split", () => {
    const draft = press(two, [["angel", 1, 1]]);
    const again: DamageDomain = structuredClone(two);
    expect(reconcileDamage(draft, two, again)).toEqual({
      draft,
      changed: false,
    });
  });

  test("a source whose targets changed starts from its suggestion", () => {
    const draft = selectSource(press(two, [["angel", 1, 1]]), two, "bears");
    const next: DamageDomain = {
      kind: "combat-damage",
      sources: [{ source: "angel", targets: [object("a")], total: "4" }],
    };
    const result = reconcileDamage(draft, two, next);
    expect(result.changed).toBe(true);
    expect(amounts(next, result.draft)).toEqual([4]);
    // The source that left is no longer the active one.
    expect(result.draft.active).toBe("angel");
  });
});

describe("the preview scenes", () => {
  const scenes: PreviewScene[] = [
    "attack",
    "block",
    "damage",
    "damage-two",
    "trample",
    "stack",
    "stack-long",
    "stack-waiting",
  ];
  test.each(scenes)("%s is a snapshot the contract accepts", (scene) => {
    const result = playerSnapshotSchema.safeParse(previewScene(scene));
    expect(result.error?.issues ?? []).toEqual([]);
  });

  test("damage scenes offer the sources the designs show", () => {
    const sources = (scene: PreviewScene) => {
      const domain = previewScene(scene).decisions[0]?.domain;
      return domain?.kind === "combat-damage" ? domain.sources : [];
    };
    expect(sources("damage").map((source) => source.total)).toEqual(["4"]);
    expect(sources("damage-two").map((source) => source.total)).toEqual([
      "4",
      "2",
    ]);
    expect(suggestDamage(sources("trample")[0]!)).toEqual([1, 5]);
  });

  test("resolving takes the newest item, and its damage lands", () => {
    const table = previewScene("stack");
    expect(table.stack.map((entry) => entry.label)).toEqual([
      "Lightning Bolt",
      "Shock",
    ]);
    const you = table.viewerSeatId;
    const life = (snapshot: typeof table) =>
      snapshot.players.find((player) => player.seatId === you)?.life;
    const shocked = previewResolveTop(table);
    expect(shocked.stack.map((entry) => entry.label)).toEqual([
      "Lightning Bolt",
    ]);
    expect(Number(life(shocked))).toBe(Number(life(table)) - 2);
    // The Bolt destroys the creature it targets; both go to graveyards.
    const target = table.stack[0]!.targets[0]!;
    const bolted = previewResolveTop(shocked);
    expect(bolted.stack).toEqual([]);
    expect(target.kind).toBe("object");
    if (target.kind !== "object") return;
    expect(
      bolted.visibleObjects.find((entry) => entry.handle === target.handle)
        ?.zone,
    ).toBe("zone-graveyard-s1");
    expect(playerSnapshotSchema.safeParse(bolted).success).toBe(true);
    expect(playerSnapshotSchema.safeParse(shocked).success).toBe(true);
  });
});

import { describe, expect, test } from "bun:test";
import { playerSnapshotSchema } from "@spellwake/application-contracts";
import {
  attackTarget,
  attackTargets,
  attackWithCreature,
  blockAttacker,
  blockWithCreature,
  canAttack,
  canBlock,
  canSkipAttack,
  canSkipBlock,
  cancelPending,
  EMPTY_ATTACK,
  EMPTY_BLOCK,
  publishedAttackers,
  reconcileAttack,
  reconcileBlock,
  type AttackDomain,
  type BlockDomain,
} from "../match/combat-draft";
import {
  PREVIEW_SEATS,
  previewAttackDecision,
  previewBlockDecision,
  previewSnapshot,
} from "../match/preview-table";

const alex = { kind: "seat" as const, seatId: PREVIEW_SEATS[1].seatId };
const morgan = { kind: "seat" as const, seatId: PREVIEW_SEATS[2].seatId };
const walker = { kind: "object" as const, handle: "walker" };
const attack: AttackDomain = {
  kind: "attackers",
  choices: [
    { attacker: "bear", defenders: [alex, morgan, walker] },
    { attacker: "angel", defenders: [alex, morgan] },
  ],
  required: [],
};

describe("declaring attackers", () => {
  test("select, then a player: one line, and the selection clears", () => {
    const selected = attackWithCreature(EMPTY_ATTACK, attack, "bear");
    expect(selected.pending).toBe("bear");
    expect(attackTargets(selected, attack)).toHaveLength(3);
    expect(canAttack(selected, attack)).toBe(false);
    const assigned = attackTarget(selected, attack, alex);
    expect(assigned.pending).toBeNull();
    expect(assigned.assigned.get("bear")).toEqual(alex);
    expect(canAttack(assigned, attack)).toBe(true);
  });

  test("a second attacker keeps the first; each has one defender", () => {
    let draft = attackTarget(
      attackWithCreature(EMPTY_ATTACK, attack, "bear"),
      attack,
      alex,
    );
    draft = attackWithCreature(draft, attack, "angel");
    // Attack waits while a creature has no defender.
    expect(canAttack(draft, attack)).toBe(false);
    draft = attackTarget(draft, attack, morgan);
    expect([...draft.assigned.keys()]).toEqual(["bear", "angel"]);
    expect(canAttack(draft, attack)).toBe(true);
  });

  test("clicking an assigned creature removes only its assignment", () => {
    let draft = attackTarget(
      attackWithCreature(EMPTY_ATTACK, attack, "bear"),
      attack,
      alex,
    );
    draft = attackTarget(
      attackWithCreature(draft, attack, "angel"),
      attack,
      morgan,
    );
    draft = attackWithCreature(draft, attack, "angel");
    expect([...draft.assigned.keys()]).toEqual(["bear"]);
    expect(draft.pending).toBeNull();
  });

  test("clicking the selected creature again, or Escape, cancels the selection", () => {
    const selected = attackWithCreature(EMPTY_ATTACK, attack, "bear");
    expect(attackWithCreature(selected, attack, "bear").pending).toBeNull();
    expect(cancelPending(selected).pending).toBeNull();
  });

  test("an unoffered creature or defender changes nothing", () => {
    expect(attackWithCreature(EMPTY_ATTACK, attack, "wall")).toBe(EMPTY_ATTACK);
    const selected = attackWithCreature(EMPTY_ATTACK, attack, "angel");
    // The planeswalker is offered to the bear, not the angel.
    expect(attackTarget(selected, attack, walker)).toBe(selected);
    expect(attackTarget(EMPTY_ATTACK, attack, alex)).toBe(EMPTY_ATTACK);
  });

  test("a card can be the defender, kept apart from its controller", () => {
    const draft = attackTarget(
      attackWithCreature(EMPTY_ATTACK, attack, "bear"),
      attack,
      walker,
    );
    expect(draft.assigned.get("bear")).toEqual(walker);
  });

  test("a creature that must attack blocks Attack and hides No Attacks", () => {
    const forced: AttackDomain = { ...attack, required: ["angel"] };
    const draft = attackTarget(
      attackWithCreature(EMPTY_ATTACK, forced, "bear"),
      forced,
      alex,
    );
    expect(canAttack(draft, forced)).toBe(false);
    expect(canSkipAttack(forced)).toBe(false);
    expect(canSkipAttack(attack)).toBe(true);
  });

  test("an update keeps what is still offered and never picks a new defender", () => {
    let draft = attackTarget(
      attackWithCreature(EMPTY_ATTACK, attack, "bear"),
      attack,
      morgan,
    );
    draft = attackTarget(
      attackWithCreature(draft, attack, "angel"),
      attack,
      alex,
    );
    const narrowed: AttackDomain = {
      ...attack,
      choices: [
        { attacker: "bear", defenders: [alex] },
        { attacker: "angel", defenders: [alex, morgan] },
      ],
    };
    const result = reconcileAttack(draft, narrowed);
    expect(result.changed).toBe(true);
    expect([...result.draft.assigned.keys()]).toEqual(["angel"]);
    expect(reconcileAttack(result.draft, narrowed).changed).toBe(false);
  });
});

const block: BlockDomain = {
  kind: "blockers",
  choices: [
    {
      blocker: "angel",
      attackers: ["ogre", "wolf"],
      minimum: "0",
      maximum: "1",
    },
    { blocker: "elf", attackers: ["ogre", "wolf"], minimum: "0", maximum: "1" },
    {
      blocker: "wall",
      attackers: ["ogre", "wolf"],
      minimum: "0",
      maximum: "2",
    },
  ],
  required: [],
};

describe("declaring blockers", () => {
  test("select, then the attacking card: one link, selection clears", () => {
    const selected = blockWithCreature(EMPTY_BLOCK, block, "angel");
    expect(canBlock(selected, block)).toBe(false);
    const linked = blockAttacker(selected, block, "ogre");
    expect(linked.pending).toBeNull();
    expect(linked.assigned.get("angel")).toEqual(["ogre"]);
    expect(canBlock(linked, block)).toBe(true);
  });

  test("two blockers may share one attacker; removing one keeps the other", () => {
    let draft = blockAttacker(
      blockWithCreature(EMPTY_BLOCK, block, "angel"),
      block,
      "ogre",
    );
    draft = blockWithCreature(draft, block, "elf");
    expect(canBlock(draft, block)).toBe(false);
    draft = blockAttacker(draft, block, "ogre");
    expect(draft.assigned.size).toBe(2);
    draft = blockWithCreature(draft, block, "elf");
    expect([...draft.assigned.keys()]).toEqual(["angel"]);
  });

  test("a blocker of several stays selected while its links toggle", () => {
    let draft = blockWithCreature(EMPTY_BLOCK, block, "wall");
    draft = blockAttacker(draft, block, "ogre");
    expect(draft.pending).toBe("wall");
    // Block is allowed with a valid set, even while it stays selected.
    expect(canBlock(draft, block)).toBe(true);
    draft = blockAttacker(draft, block, "wolf");
    expect(draft.assigned.get("wall")).toEqual(["ogre", "wolf"]);
    draft = blockAttacker(draft, block, "wolf");
    expect(draft.assigned.get("wall")).toEqual(["ogre"]);
    // Escape leaves selection and keeps the links.
    expect(cancelPending(draft).assigned.get("wall")).toEqual(["ogre"]);
    // Clicking the blocker clears its whole set.
    expect(blockWithCreature(draft, block, "wall").assigned.size).toBe(0);
  });

  test("an ordinary blocker never gets a second link", () => {
    const linked = blockAttacker(
      blockWithCreature(EMPTY_BLOCK, block, "angel"),
      block,
      "ogre",
    );
    expect(blockAttacker(linked, block, "wolf")).toBe(linked);
  });

  test("the source's counts govern Block and No Blocks", () => {
    const forced: BlockDomain = { ...block, required: ["elf"] };
    const draft = blockAttacker(
      blockWithCreature(EMPTY_BLOCK, forced, "angel"),
      forced,
      "ogre",
    );
    expect(canBlock(draft, forced)).toBe(false);
    expect(canSkipBlock(forced)).toBe(false);
    // Menace: an attacker blocked at all needs two blockers.
    const menace: BlockDomain = {
      ...block,
      minimumIfBlocked: [{ attacker: "ogre", minimum: "2" }],
    };
    expect(canBlock(draft, menace)).toBe(false);
    const two = blockAttacker(
      blockWithCreature(draft, menace, "elf"),
      menace,
      "ogre",
    );
    expect(canBlock(two, menace)).toBe(true);
  });

  test("the attacks against you are read from what may be blocked", () => {
    expect(publishedAttackers(block)).toEqual(["ogre", "wolf"]);
  });

  test("an update drops links to attackers no longer offered", () => {
    const draft = blockAttacker(
      blockWithCreature(EMPTY_BLOCK, block, "angel"),
      block,
      "wolf",
    );
    const narrowed: BlockDomain = {
      ...block,
      choices: block.choices.map((choice) => ({
        ...choice,
        attackers: ["ogre"],
      })),
    };
    const result = reconcileBlock(draft, narrowed);
    expect(result.changed).toBe(true);
    expect(result.draft.assigned.size).toBe(0);
  });
});

describe("the preview's combat", () => {
  test("its decisions are ones the player-view contract accepts", () => {
    const table = previewSnapshot();
    const attackDecision = previewAttackDecision(table)!;
    const blockDecision = previewBlockDecision(table, PREVIEW_SEATS[1].seatId)!;
    for (const decision of [attackDecision, blockDecision])
      expect(
        playerSnapshotSchema.safeParse({ ...table, decisions: [decision] })
          .success,
      ).toBe(true);
    expect(attackDecision.domain.kind).toBe("attackers");
    expect(blockDecision.domain.kind).toBe("blockers");
  });
});

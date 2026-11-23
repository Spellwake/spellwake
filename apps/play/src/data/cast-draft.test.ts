import { describe, expect, test } from "bun:test";
import {
  playerCommandSchema,
  playerSnapshotSchema,
  validPlayerChoice,
} from "@spellwake/application-contracts";
import {
  adjustShare,
  backCast,
  beginCast,
  canAddShare,
  canConfirmModes,
  canConfirmShares,
  canGoBack,
  canShareMinus,
  canSharePlus,
  castAction,
  castTargets,
  chooseCastMode,
  chooseCastTarget,
  confirmCastModes,
  confirmShares,
  costSymbols,
  currentStep,
  payCast,
  reconcileCast,
  sharedOut,
  toggleShare,
  type CastDraft,
  type CastOffer,
  type CastStep,
} from "../match/cast-draft";
import {
  PREVIEW_SEATS,
  previewCast,
  previewCastOffers,
  previewDivisionDecision,
  previewResolveTop,
  previewScene,
  previewSnapshot,
} from "../match/preview-table";
import { printedModes } from "../match/printed-modes";
import { seatBoard } from "../match/table-model";

const alex = { kind: "seat" as const, seatId: PREVIEW_SEATS[1].seatId };
const angel = { kind: "object" as const, handle: "angel" };
const bolt: CastOffer = {
  source: "bolt",
  targets: [alex, angel],
  cost: "{R}",
  payment: "automatic",
  paySources: ["mountain"],
};
const krenko: CastOffer = {
  source: "krenko",
  targets: null,
  cost: "{2}{R}{R}",
  payment: "automatic",
  paySources: [],
};
const paused = (offer: CastOffer): CastOffer => ({
  ...offer,
  payment: "paused",
});
const draftOf = (step: CastStep): CastDraft => {
  if (step.kind === "ignored") throw new Error("The step was ignored.");
  return step.draft;
};

describe("starting a cast", () => {
  test("a card the game does not offer is not started (BC-08)", () => {
    expect(beginCast([bolt], "mountain")).toEqual({ kind: "ignored" });
    expect(beginCast([], "bolt")).toEqual({ kind: "ignored" });
  });

  test("a spell that needs a target waits for one (BC-02)", () => {
    const step = beginCast([bolt, krenko], "bolt");
    expect(step).toEqual({
      kind: "draft",
      draft: {
        source: "bolt",
        stage: "target",
        modes: [],
        step: 0,
        targets: [null],
        shares: [],
      },
    });
    expect(castTargets([bolt], draftOf(step))).toEqual([alex, angel]);
  });

  test("a spell that needs a target and is offered none is not started", () => {
    expect(beginCast([{ ...bolt, targets: [] }], "bolt")).toEqual({
      kind: "ignored",
    });
  });

  test("a spell with no target is sent at once (BC-07)", () => {
    expect(beginCast([krenko], "krenko")).toMatchObject({
      kind: "submit",
      action: { kind: "cast-untargeted", source: "krenko", payment: null },
    });
  });
});

describe("choosing the target", () => {
  const draft = draftOf(beginCast([bolt], "bolt"));

  test("the last choice sends the cast: there is no Cast button (BC-06)", () => {
    expect(chooseCastTarget([bolt], draft, alex)).toMatchObject({
      kind: "submit",
      action: {
        kind: "cast-targeted",
        source: "bolt",
        target: alex,
        payment: null,
      },
    });
  });

  test("a card is chosen by its own handle, never by its name (BC-09)", () => {
    expect(chooseCastTarget([bolt], draft, angel)).toMatchObject({
      kind: "submit",
      action: { target: angel },
    });
  });

  test("what was not offered is not a target", () => {
    expect(
      chooseCastTarget([bolt], draft, { kind: "object", handle: "pacifism" }),
    ).toEqual({ kind: "ignored" });
    expect(
      chooseCastTarget([bolt], draft, {
        kind: "seat",
        seatId: PREVIEW_SEATS[2].seatId,
      }),
    ).toEqual({ kind: "ignored" });
  });
});

describe("a cost the game pauses for (BC-04)", () => {
  const offers = [paused(bolt), paused(krenko)];
  const targeting = draftOf(beginCast(offers, "bolt"));
  const paying = draftOf(chooseCastTarget(offers, targeting, alex));

  test("the chosen target is kept while payment is waited for", () => {
    expect(paying).toEqual({
      source: "bolt",
      stage: "payment",
      modes: [],
      step: 0,
      targets: [alex],
      shares: [],
    });
    // Nothing is offered as a target once one is chosen.
    expect(castTargets(offers, paying)).toEqual([]);
  });

  test("paying sends the cast with its target", () => {
    expect(payCast(offers, paying)).toMatchObject({
      kind: "submit",
      action: { kind: "cast-targeted", source: "bolt", target: alex },
    });
  });

  test("a spell with no target goes straight to payment", () => {
    expect(beginCast(offers, "krenko")).toEqual({
      kind: "draft",
      draft: {
        source: "krenko",
        stage: "payment",
        modes: [],
        step: 0,
        targets: [],
        shares: [],
      },
    });
  });

  test("only a draft waiting for payment can be paid", () => {
    expect(payCast(offers, targeting)).toEqual({ kind: "ignored" });
  });
});

describe("the game changing under a draft", () => {
  const targeting = draftOf(beginCast([bolt], "bolt"));
  const paying = draftOf(chooseCastTarget([paused(bolt)], targeting, angel));

  test("a draft still offered as it was is kept", () => {
    expect(reconcileCast([bolt], targeting)).toBe(targeting);
    expect(reconcileCast([paused(bolt)], paying)).toBe(paying);
  });

  test("a card no longer offered drops its draft", () => {
    expect(reconcileCast([krenko], targeting)).toBeNull();
  });

  test("a chosen target that is gone drops the draft; none is picked for it", () => {
    expect(
      reconcileCast([{ ...paused(bolt), targets: [alex] }], paying),
    ).toBeNull();
  });
});

test("a printed cost is read in order", () => {
  expect(costSymbols("{2}{R}{R}")).toEqual(["2", "R", "R"]);
  expect(costSymbols("")).toEqual([]);
});

describe("the preview's cast offers", () => {
  const table = previewSnapshot();
  const offers = previewCastOffers(table, true);
  const nameOf = (handle: string) => {
    const object = table.visibleObjects.find(
      (entry) => entry.handle === handle,
    );
    return object?.identity.kind === "known"
      ? object.identity.characteristics.name
      : "";
  };
  const offered = (name: string) =>
    offers.find((offer) => nameOf(offer.source) === name);

  test("offer what the untapped Mountains can pay for, and no land (BC-01)", () => {
    expect(offers.map((offer) => nameOf(offer.source)).sort()).toEqual([
      "Abrade",
      "Arc Lightning",
      "Common Bond",
      "Cryptic Command",
      "Goblin Matron",
      "Goblin Piledriver",
      "Krenko, Mob Boss",
      "Lightning Bolt",
      "Shock",
    ]);
  });

  test("burn may go at every creature and every player; a creature has no target", () => {
    const targets = offered("Lightning Bolt")?.targets ?? [];
    expect(targets.filter((target) => target.kind === "seat")).toHaveLength(6);
    expect(targets.filter((target) => target.kind === "object")).toHaveLength(
      16,
    );
    expect(offered("Krenko, Mob Boss")?.targets).toBeNull();
  });

  test("payment is automatic with Auto-tap mana, and paused without it", () => {
    expect(offers.every((offer) => offer.payment === "automatic")).toBe(true);
    const manual = previewCastOffers(table, false);
    expect(manual.every((offer) => offer.payment === "paused")).toBe(true);
    // The rightmost untapped Mountain is the one a payment would use.
    const lands = seatBoard(table, table.viewerSeatId).lands;
    expect(
      manual.find((offer) => nameOf(offer.source) === "Shock")?.paySources,
    ).toEqual([lands.at(-1)!.handle]);
  });

  test("nothing is offered while another player acts or a decision waits", () => {
    expect(previewCastOffers(previewScene("stack-waiting"), true)).toEqual([]);
    expect(previewCastOffers(previewScene("attack"), true)).toEqual([]);
  });

  test("only instants are offered while something waits on the stack", () => {
    const names = previewCastOffers(previewScene("stack"), true).map(
      (offer) => {
        const object = previewScene("stack").visibleObjects.find(
          (entry) => entry.handle === offer.source,
        );
        return object?.identity.kind === "known"
          ? object.identity.characteristics.name
          : "";
      },
    );
    expect(names.sort()).toEqual([
      "Abrade",
      "Common Bond",
      "Cryptic Command",
      "Lightning Bolt",
      "Shock",
    ]);
  });
});

describe("casting in the preview", () => {
  const table = previewSnapshot();
  const you = table.viewerSeatId;
  const offers = previewCastOffers(table, true);
  const bolted = offers.find((offer) => offer.targets !== null)!;
  const held = seatBoard(table, you).hand.length;
  const step = chooseCastTarget(
    offers,
    draftOf(beginCast(offers, bolted.source)),
    alex,
  );

  test("what is sent is a command the contract accepts", () => {
    if (step.kind !== "submit") throw new Error("Not sent.");
    expect(
      playerCommandSchema.safeParse({
        requestId: "00000000-0000-4000-8000-000000000301",
        basis: table.basis,
        action: step.action,
      }).success,
    ).toBe(true);
  });

  test("the card leaves the hand for the stack and one Mountain taps (BC-06)", () => {
    const after = previewCast(table, bolted.source, [alex]);
    expect(playerSnapshotSchema.safeParse(after).error?.issues ?? []).toEqual(
      [],
    );
    expect(seatBoard(after, you).hand).toHaveLength(held - 1);
    expect(after.stack.map((entry) => entry.sourceObject)).toEqual([
      bolted.source,
    ]);
    expect(after.stack[0]!.targets).toEqual([alex]);
    expect(
      seatBoard(after, you).lands.filter((land) => land.tapped),
    ).toHaveLength(1);
    // Nothing has resolved: every life total is as it was.
    expect(after.players.map((player) => player.life)).toEqual(
      table.players.map((player) => player.life),
    );
  });

  test("a creature spell resolves onto its caster's board (BC-07)", () => {
    const creature = offers.find(
      (offer) =>
        offer.targets === null && !offer.modes && !offer.slots && !offer.divide,
    )!;
    const cast = previewCast(table, creature.source, []);
    const before = seatBoard(table, you).front.length;
    expect(seatBoard(cast, you).front).toHaveLength(before);
    const resolved = previewResolveTop(cast);
    expect(playerSnapshotSchema.safeParse(resolved).success).toBe(true);
    expect(resolved.stack).toEqual([]);
    expect(seatBoard(resolved, you).front).toHaveLength(before + 1);
  });

  test("a card that is not in hand is not cast", () => {
    expect(previewCast(table, "elsewhere", [])).toBe(table);
  });
});

describe("a modal spell (MC-01 to MC-06)", () => {
  const ball = { kind: "object" as const, handle: "crystal-ball" };
  const abrade: CastOffer = {
    source: "abrade",
    targets: null,
    modes: [
      { mode: 0, label: "Abrade deals 3 damage.", targets: [angel] },
      { mode: 1, label: "Destroy target artifact.", targets: [ball] },
    ],
    cost: "{1}{R}",
    payment: "automatic",
    paySources: [],
  };
  const noArtifact: CastOffer = {
    ...abrade,
    modes: [
      abrade.modes![0]!,
      {
        ...abrade.modes![1]!,
        targets: [],
        unavailableReason: "No legal targets",
      },
    ],
  };
  const choosing = draftOf(beginCast([abrade], "abrade"));

  test("waits for its mode before anything else", () => {
    expect(choosing).toEqual({
      source: "abrade",
      stage: "mode",
      modes: [],
      step: 0,
      targets: [],
      shares: [],
    });
    expect(castTargets([abrade], choosing)).toEqual([]);
  });

  test("choosing a mode goes straight on to that mode's own targets", () => {
    const damage = draftOf(chooseCastMode([abrade], choosing, 0));
    expect(damage).toMatchObject({ stage: "target", modes: [0] });
    expect(castTargets([abrade], damage)).toEqual([angel]);
    const destroy = draftOf(chooseCastMode([abrade], choosing, 1));
    expect(castTargets([abrade], destroy)).toEqual([ball]);
    // The other mode's target is not offered.
    expect(chooseCastTarget([abrade], destroy, angel)).toEqual({
      kind: "ignored",
    });
  });

  test("the target sends the cast with the mode and its target together", () => {
    const destroy = draftOf(chooseCastMode([abrade], choosing, 1));
    const step = chooseCastTarget([abrade], destroy, ball);
    expect(step).toMatchObject({
      kind: "submit",
      action: {
        kind: "cast-with-resources",
        source: "abrade",
        modes: [{ mode: 1, target: ball }],
      },
    });
    if (step.kind !== "submit") return;
    expect(
      playerCommandSchema.safeParse({
        requestId: "00000000-0000-4000-8000-000000000301",
        basis: previewSnapshot().basis,
        action: step.action,
      }).success,
    ).toBe(true);
    expect(castAction(abrade, step.draft)).toEqual(step.action);
  });

  test("a mode with no legal target cannot be chosen (MC-02)", () => {
    const draft = draftOf(beginCast([noArtifact], "abrade"));
    expect(chooseCastMode([noArtifact], draft, 1)).toEqual({ kind: "ignored" });
    expect(chooseCastMode([noArtifact], draft, 0).kind).toBe("draft");
    // With no mode that can be chosen, the spell is not started at all.
    expect(
      beginCast(
        [
          {
            ...noArtifact,
            modes: noArtifact.modes!.map((mode) => ({
              ...mode,
              unavailableReason: "No legal targets",
            })),
          },
        ],
        "abrade",
      ),
    ).toEqual({ kind: "ignored" });
  });

  test("a mode is chosen only while one is waited for, and only an offered one", () => {
    expect(chooseCastMode([abrade], choosing, 7)).toEqual({ kind: "ignored" });
    const chosen = draftOf(chooseCastMode([abrade], choosing, 0));
    expect(chooseCastMode([abrade], chosen, 1)).toEqual({ kind: "ignored" });
  });

  test("a chosen mode the game stops offering drops the draft", () => {
    const destroy = draftOf(chooseCastMode([abrade], choosing, 1));
    expect(reconcileCast([abrade], destroy)).toBe(destroy);
    expect(reconcileCast([noArtifact], destroy)).toBeNull();
    expect(reconcileCast([abrade], choosing)).toBe(choosing);
  });

  test("the preview offers Abrade with both modes, and resolves each", () => {
    const table = previewSnapshot();
    const offer = previewCastOffers(table, true).find(
      (entry) => entry.modes && !entry.choose,
    );
    expect(offer?.modes?.map((mode) => mode.targets?.length)).toEqual([16, 2]);
    expect(
      previewCastOffers(previewScene("no-artifacts"), true).find(
        (entry) => entry.modes && !entry.choose,
      )?.modes?.[1],
    ).toMatchObject({ targets: [], unavailableReason: "No legal targets" });
    const artifact = offer!.modes![1]!.targets![0]!;
    const cast = previewCast(table, offer!.source, [artifact], [1]);
    expect(playerSnapshotSchema.safeParse(cast).error?.issues ?? []).toEqual(
      [],
    );
    expect(cast.stack[0]?.modes).toEqual([1]);
    const resolved = previewResolveTop(cast);
    expect(playerSnapshotSchema.safeParse(resolved).success).toBe(true);
    if (artifact.kind !== "object") throw new Error("Not a card.");
    // Destroyed: the artifact is in its owner's graveyard, not on the table.
    expect(
      resolved.visibleObjects.find((entry) => entry.handle === artifact.handle)
        ?.zone,
    ).toMatch(/^zone-graveyard-/);
  });

  test("its printed mode regions are known by printing, in printed order", () => {
    const printed = printedModes({ set: "trc", number: "449" });
    expect(printed?.regions).toHaveLength(2);
    expect(printed!.regions[0]!.y).toBeLessThan(printed!.regions[1]!.y);
    expect(printedModes({ set: "TRC", number: "1" })).toBeNull();
    expect(printedModes(null)).toBeNull();
  });
});

const accepted = (step: CastStep) => {
  if (step.kind !== "submit") throw new Error("Not sent.");
  return playerCommandSchema.safeParse({
    requestId: "00000000-0000-4000-8000-000000000301",
    basis: previewSnapshot().basis,
    action: step.action,
  }).success;
};
const elf = { kind: "object" as const, handle: "elf" };
const grizzly = { kind: "object" as const, handle: "grizzly" };
const island = { kind: "object" as const, handle: "island" };
const boltSpell = { kind: "stack" as const, handle: "stack-bolt" };
const growthSpell = { kind: "stack" as const, handle: "stack-growth" };

describe("a spell that asks for two modes (MM-00 to MM-07)", () => {
  const cryptic = (spells: readonly (typeof boltSpell)[]): CastOffer => ({
    source: "cryptic",
    targets: null,
    choose: 2,
    modes: [
      {
        mode: 0,
        label: "Counter target spell.",
        ask: "Choose a spell",
        targets: spells,
        ...(spells.length === 0
          ? { unavailableReason: "No legal targets" }
          : {}),
      },
      {
        mode: 1,
        label: "Return target permanent to its owner’s hand.",
        ask: "Choose a permanent",
        targets: [island, angel],
      },
      { mode: 2, label: "Tap all creatures.", targets: null },
      { mode: 3, label: "Draw a card.", targets: null },
    ],
    cost: "{1}{U}{U}{U}",
    payment: "automatic",
    paySources: [],
  });
  const quiet = [cryptic([])];
  const choosing = draftOf(beginCast(quiet, "cryptic"));
  const mark = (offers: CastOffer[], draft: CastDraft, ...modes: number[]) =>
    modes.reduce(
      (current, mode) => draftOf(chooseCastMode(offers, current, mode)),
      draft,
    );

  test("pressing a mode marks it and does not go on (MM-02)", () => {
    const one = mark(quiet, choosing, 3);
    expect(one).toMatchObject({ stage: "mode", modes: [3] });
    expect(canConfirmModes(quiet, one)).toBe(false);
    expect(confirmCastModes(quiet, one)).toEqual({ kind: "ignored" });
  });

  test("with two marked, a third cannot be added until one is removed (MM-03, MM-05)", () => {
    const two = mark(quiet, choosing, 3, 2);
    // Kept in printed order whatever order they were pressed in.
    expect(two.modes).toEqual([2, 3]);
    expect(canConfirmModes(quiet, two)).toBe(true);
    expect(chooseCastMode(quiet, two, 1)).toEqual({ kind: "ignored" });
    const back = mark(quiet, two, 2);
    expect(back.modes).toEqual([3]);
    expect(mark(quiet, back, 1).modes).toEqual([1, 3]);
  });

  test("a mode with no legal target cannot be marked", () => {
    expect(chooseCastMode(quiet, choosing, 0)).toEqual({ kind: "ignored" });
  });

  test("two modes with no target are cast by Next, with no further confirmation", () => {
    const step = confirmCastModes(quiet, mark(quiet, choosing, 2, 3));
    expect(step).toMatchObject({
      kind: "submit",
      action: {
        kind: "cast-with-resources",
        modes: [
          { mode: 2, target: null },
          { mode: 3, target: null },
        ],
      },
    });
    expect(accepted(step)).toBe(true);
  });

  test("Return and Draw go on to one permanent, then cast with both modes (MM-06)", () => {
    const targeting = draftOf(
      confirmCastModes(quiet, mark(quiet, choosing, 1, 3)),
    );
    expect(targeting).toMatchObject({ stage: "target", step: 0 });
    expect(currentStep(quiet, targeting)?.ask).toBe("Choose a permanent");
    expect(canGoBack(targeting)).toBe(false);
    const step = chooseCastTarget(quiet, targeting, island);
    expect(step).toMatchObject({
      kind: "submit",
      action: {
        modes: [
          { mode: 1, target: island },
          { mode: 3, target: null },
        ],
      },
    });
    expect(accepted(step)).toBe(true);
  });

  test("too few modes that can be chosen: the spell is not started", () => {
    const one = cryptic([]);
    const offer: CastOffer = {
      ...one,
      modes: one.modes!.map((mode) =>
        mode.mode === 3 ? mode : { ...mode, unavailableReason: "No" },
      ),
    };
    expect(beginCast([offer], "cryptic")).toEqual({ kind: "ignored" });
  });

  describe("Counter and Return: a spell, then a permanent", () => {
    const loud = [cryptic([boltSpell, growthSpell])];
    const first = draftOf(
      confirmCastModes(
        loud,
        mark(loud, draftOf(beginCast(loud, "cryptic")), 1, 0),
      ),
    );

    test("the spell is asked for first, in printed order, whatever was pressed first", () => {
      expect(first.modes).toEqual([0, 1]);
      expect(currentStep(loud, first)?.ask).toBe("Choose a spell");
      expect(castTargets(loud, first)).toEqual([boltSpell, growthSpell]);
      // A permanent is not a target yet.
      expect(chooseCastTarget(loud, first, island)).toEqual({
        kind: "ignored",
      });
    });

    test("then the permanent; Back returns to the spell with the old choice kept", () => {
      const second = draftOf(chooseCastTarget(loud, first, boltSpell));
      expect(second).toMatchObject({ step: 1, targets: [boltSpell, null] });
      expect(currentStep(loud, second)?.ask).toBe("Choose a permanent");
      const again = draftOf(backCast(second));
      expect(again).toMatchObject({ step: 0, targets: [boltSpell, null] });
      expect(canGoBack(again)).toBe(false);
      // Another spell replaces the first choice and goes on.
      expect(draftOf(chooseCastTarget(loud, again, growthSpell))).toMatchObject(
        { step: 1, targets: [growthSpell, null] },
      );
    });

    test("the last target casts, each mode with its own target", () => {
      const second = draftOf(chooseCastTarget(loud, first, growthSpell));
      const step = chooseCastTarget(loud, second, angel);
      expect(step).toMatchObject({
        kind: "submit",
        action: {
          modes: [
            { mode: 0, target: growthSpell },
            { mode: 1, target: angel },
          ],
        },
      });
      expect(accepted(step)).toBe(true);
    });

    test("a chosen spell that leaves the stack drops the draft", () => {
      const second = draftOf(chooseCastTarget(loud, first, boltSpell));
      expect(reconcileCast(loud, second)).toBe(second);
      expect(reconcileCast([cryptic([growthSpell])], second)).toBeNull();
    });
  });
});

describe("two instructions, each with its own target (MT-00 to MT-06)", () => {
  const creatures = [elf, grizzly, angel];
  const bond: CastOffer = {
    source: "bond",
    targets: null,
    slots: [
      { ask: "Choose a creature", targets: creatures },
      { ask: "Choose a creature", targets: creatures },
    ],
    cost: "{1}{G}{W}",
    payment: "automatic",
    paySources: [],
  };
  const first = draftOf(beginCast([bond], "bond"));

  test("starts at the first with no way back (MT-01)", () => {
    expect(first).toMatchObject({ stage: "target", step: 0 });
    expect(first.targets).toEqual([null, null]);
    expect(canGoBack(first)).toBe(false);
    expect(backCast(first)).toEqual({ kind: "ignored" });
  });

  test("the first choice goes straight on, and stays offered for the second", () => {
    const second = draftOf(chooseCastTarget([bond], first, grizzly));
    expect(second).toMatchObject({ step: 1, targets: [grizzly, null] });
    expect(castTargets([bond], second)).toEqual(creatures);
  });

  test("the same creature may be chosen twice, and its order is kept", () => {
    const second = draftOf(chooseCastTarget([bond], first, grizzly));
    const step = chooseCastTarget([bond], second, grizzly);
    expect(step).toMatchObject({
      kind: "submit",
      action: {
        kind: "cast-with-resources",
        fixedTargets: [grizzly, grizzly],
      },
    });
    expect(accepted(step)).toBe(true);
    expect(chooseCastTarget([bond], second, elf)).toMatchObject({
      action: { fixedTargets: [grizzly, elf] },
    });
  });

  test("Back keeps the old first choice until another replaces it (MT-03, MT-05)", () => {
    const second = draftOf(chooseCastTarget([bond], first, angel));
    const editing = draftOf(backCast(second));
    expect(editing).toMatchObject({ step: 0, targets: [angel, null] });
    // Choosing the old creature again goes on just the same.
    expect(draftOf(chooseCastTarget([bond], editing, angel))).toEqual(second);
    expect(draftOf(chooseCastTarget([bond], editing, elf)).targets).toEqual([
      elf,
      null,
    ]);
  });

  test("a paused cost waits with both targets kept", () => {
    const offers = [paused(bond)];
    const second = draftOf(chooseCastTarget(offers, first, elf));
    const paying = draftOf(chooseCastTarget(offers, second, angel));
    expect(paying).toMatchObject({ stage: "payment", targets: [elf, angel] });
    expect(payCast(offers, paying)).toMatchObject({
      action: { fixedTargets: [elf, angel] },
    });
  });
});

describe("a divided amount (DC-00 to DC-11)", () => {
  const players = [alex];
  const arc: CastOffer = {
    source: "arc",
    targets: null,
    divide: {
      ask: "Divide 3 damage",
      total: 3,
      minimum: 1,
      most: 3,
      targets: [...players, elf, grizzly, angel],
    },
    cost: "{2}{R}",
    payment: "automatic",
    paySources: [],
  };
  const offers = [arc];
  const zero = draftOf(beginCast(offers, "arc"));
  const press = (draft: CastDraft, ...targets: (typeof elf | typeof alex)[]) =>
    targets.reduce(
      (current, target) => draftOf(toggleShare(offers, current, target)),
      draft,
    );
  const amounts = (draft: CastDraft) =>
    draft.shares.map((share) => share.amount);

  test("starts with nothing given out and cannot be confirmed (DC-01)", () => {
    expect(zero).toMatchObject({ stage: "divide", shares: [] });
    expect(sharedOut(zero)).toBe(0);
    expect(canConfirmShares(offers, zero)).toBe(false);
    expect(confirmShares(offers, zero)).toEqual({ kind: "ignored" });
  });

  test("the first target takes everything; each further one takes one from a target that can spare it (DC-02 to DC-06)", () => {
    const one = press(zero, elf);
    expect(amounts(one)).toEqual([3]);
    const two = press(one, grizzly);
    expect(amounts(two)).toEqual([2, 1]);
    const three = press(two, angel);
    expect(amounts(three)).toEqual([1, 1, 1]);
    // A fourth target is not offered.
    expect(canAddShare(offers, three, alex)).toBe(false);
    expect(toggleShare(offers, three, alex)).toEqual({ kind: "ignored" });
  });

  test("pressing a chosen target removes it and keeps the other amounts (DC-07, DC-08)", () => {
    const three = press(zero, elf, grizzly, angel);
    const removed = press(three, angel);
    expect(amounts(removed)).toEqual([1, 1]);
    expect(sharedOut(removed)).toBe(2);
    expect(canConfirmShares(offers, removed)).toBe(false);
    const two = press(zero, elf, grizzly);
    const alone = press(two, elf);
    expect(alone.shares).toEqual([{ target: grizzly, amount: 1 }]);
    // Chosen again, it takes what was left over.
    expect(press(alone, elf).shares).toEqual([
      { target: grizzly, amount: 1 },
      { target: elf, amount: 2 },
    ]);
    expect(press(press(zero, elf), elf).shares).toEqual([]);
  });

  test("minus returns one; plus takes what is left before it takes from another (DC-04)", () => {
    const two = press(zero, elf, grizzly);
    expect(canShareMinus(offers, two, grizzly)).toBe(false);
    expect(canSharePlus(offers, two, elf)).toBe(false);
    expect(adjustShare(offers, two, elf, 1)).toEqual({ kind: "ignored" });
    expect(amounts(draftOf(adjustShare(offers, two, grizzly, 1)))).toEqual([
      1, 2,
    ]);
    const less = draftOf(adjustShare(offers, two, elf, -1));
    expect(amounts(less)).toEqual([1, 1]);
    expect(sharedOut(less)).toBe(2);
    expect(amounts(draftOf(adjustShare(offers, less, grizzly, 1)))).toEqual([
      1, 2,
    ]);
    const three = press(zero, elf, grizzly, angel);
    for (const target of [elf, grizzly, angel]) {
      expect(canShareMinus(offers, three, target)).toBe(false);
      expect(canSharePlus(offers, three, target)).toBe(false);
    }
  });

  test("Confirm casts at the chosen targets and carries the shares for the game's question (DC-10)", () => {
    const step = confirmShares(offers, press(zero, grizzly, alex));
    expect(step).toMatchObject({
      kind: "submit",
      // In the game's own order of targets, not the order pressed.
      action: { kind: "cast-with-resources", fixedTargets: [alex, grizzly] },
      division: [
        { target: alex, amount: "1" },
        { target: grizzly, amount: "2" },
      ],
    });
    expect(accepted(step)).toBe(true);
  });

  test("a chosen target the game stops offering drops the draft", () => {
    const two = press(zero, elf, grizzly);
    expect(reconcileCast(offers, two)).toBe(two);
    expect(
      reconcileCast(
        [{ ...arc, divide: { ...arc.divide!, targets: [elf, angel] } }],
        two,
      ),
    ).toBeNull();
  });
});

describe("the preview's two-mode, two-target and divided spells", () => {
  const table = previewSnapshot();
  const you = table.viewerSeatId;
  const find = (snapshot: typeof table, name: string) =>
    previewCastOffers(snapshot, true).find((offer) => {
      const object = snapshot.visibleObjects.find(
        (entry) => entry.handle === offer.source,
      );
      return (
        object?.identity.kind === "known" &&
        object.identity.characteristics.name === name
      );
    })!;
  const valid = (snapshot: typeof table) =>
    playerSnapshotSchema.safeParse(snapshot).error?.issues ?? [];

  test("Cryptic Command cannot counter with nothing on the stack, and can with spells there", () => {
    expect(find(table, "Cryptic Command").modes?.[0]).toMatchObject({
      targets: [],
      unavailableReason: "No legal targets",
    });
    const replies = previewScene("stack");
    const offer = find(replies, "Cryptic Command");
    expect(offer.choose).toBe(2);
    // Newest first, as the pile shows them.
    expect(offer.modes?.[0]?.targets).toEqual(
      [...replies.stack]
        .reverse()
        .map((entry) => ({ kind: "stack", handle: entry.handle })),
    );
    expect(offer.modes?.[0]?.unavailableReason).toBeUndefined();
  });

  test("Counter and Return: the spell leaves the stack and the permanent goes to its owner's hand", () => {
    const replies = previewScene("stack");
    const offer = find(replies, "Cryptic Command");
    const spell = offer.modes![0]!.targets![0]!;
    const vial = seatBoard(replies, you).others[0]!;
    const cast = previewCast(
      replies,
      offer.source,
      [spell, { kind: "object", handle: vial.handle }],
      [0, 1],
    );
    expect(valid(cast)).toEqual([]);
    expect(cast.stack).toHaveLength(3);
    const held = seatBoard(cast, you).hand.length;
    const resolved = previewResolveTop(cast);
    expect(valid(resolved)).toEqual([]);
    expect(resolved.stack).toHaveLength(1);
    expect(seatBoard(resolved, you).hand).toHaveLength(held + 1);
    expect(seatBoard(resolved, you).others).toHaveLength(0);
  });

  test("Tap and Draw: opposing creatures tap and a card is drawn", () => {
    const offer = find(table, "Cryptic Command");
    const cast = previewCast(table, offer.source, [], [2, 3]);
    const held = seatBoard(cast, you).hand.length;
    const resolved = previewResolveTop(cast);
    expect(valid(resolved)).toEqual([]);
    expect(seatBoard(resolved, you).hand).toHaveLength(held + 1);
    const creatures = resolved.visibleObjects.filter(
      (object) =>
        object.zone === "zone-battlefield" &&
        object.publicProperties?.cardTypes.includes("creature"),
    );
    expect(
      creatures.every(
        (object) => object.tapped === (object.controllerSeatId !== you),
      ),
    ).toBe(true);
  });

  test("Common Bond puts a counter on each creature named, twice on one named twice", () => {
    const offer = find(table, "Common Bond");
    const [guide, chieftain] = offer.slots![0]!.targets;
    const twice = previewResolveTop(
      previewCast(table, offer.source, [guide!, guide!]),
    );
    expect(valid(twice)).toEqual([]);
    const grown = (snapshot: typeof table, target: typeof guide) =>
      snapshot.visibleObjects.find(
        (object) =>
          target?.kind === "object" && object.handle === target.handle,
      );
    expect(grown(twice, guide)?.counters).toEqual([
      { name: "+1/+1", count: "2" },
    ]);
    expect(grown(twice, guide)?.publicProperties?.power).toBe("4");
    const each = previewResolveTop(
      previewCast(table, offer.source, [guide!, chieftain!]),
    );
    expect(grown(each, guide)?.counters).toEqual([
      { name: "+1/+1", count: "1" },
    ]);
    expect(grown(each, chieftain)?.counters).toEqual([
      { name: "+1/+1", count: "1" },
    ]);
  });

  test("Arc Lightning is cast, then asked how it is divided, and deals each share", () => {
    const offer = find(table, "Arc Lightning");
    expect(offer.divide).toMatchObject({ total: 3, minimum: 1, most: 3 });
    const zero = draftOf(beginCast([offer], offer.source));
    const guide = offer.divide!.targets[0]!;
    const one = draftOf(toggleShare([offer], zero, guide));
    const step = confirmShares(
      [offer],
      draftOf(toggleShare([offer], one, alex)),
    );
    if (step.kind !== "submit" || !step.division) throw new Error("Not sent.");
    if (step.action.kind !== "cast-with-resources") throw new Error("Wrong.");
    const cast = previewCast(
      table,
      offer.source,
      step.action.fixedTargets ?? [],
    );
    const asked = previewDivisionDecision(cast)!;
    expect(valid({ ...cast, decisions: [asked] })).toEqual([]);
    // The shares already chosen are an answer the game accepts.
    expect(
      validPlayerChoice(
        asked,
        {
          requestId: "00000000-0000-4000-8000-000000000301",
          basis: asked.basis,
          action: {
            kind: "choose",
            decision: asked.decision,
            response: {
              kind: "distribution",
              allocations: [...step.division],
            },
          },
        },
        you,
        table.players.map((player) => player.seatId),
      ),
    ).toBe(true);
    const resolved = previewResolveTop(cast, step.division);
    expect(valid(resolved)).toEqual([]);
    const life = (snapshot: typeof table) =>
      Number(
        snapshot.players.find((player) => player.seatId === alex.seatId)?.life,
      );
    expect(life(resolved)).toBe(life(table) - 1);
    // Two damage kills the 2/2 it was given to.
    if (guide.kind !== "object") throw new Error("Not a card.");
    expect(
      resolved.visibleObjects.find((object) => object.handle === guide.handle)
        ?.zone,
    ).toMatch(/^zone-graveyard-/);
  });

  test("the three faces' printed regions are known by printing", () => {
    expect(printedModes({ set: "TRC", number: "435" })?.regions).toHaveLength(
      4,
    );
    expect(printedModes({ set: "RTR", number: "151" })?.regions).toHaveLength(
      2,
    );
    expect(printedModes({ set: "KTK", number: "97" })?.regions).toEqual([]);
  });
});

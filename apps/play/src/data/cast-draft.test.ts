import { describe, expect, test } from "bun:test";
import {
  playerCommandSchema,
  playerSnapshotSchema,
} from "@spellwake/application-contracts";
import {
  beginCast,
  castTargets,
  chooseCastTarget,
  costSymbols,
  payCast,
  reconcileCast,
  type CastDraft,
  type CastOffer,
  type CastStep,
} from "../match/cast-draft";
import {
  PREVIEW_SEATS,
  previewCast,
  previewCastOffers,
  previewResolveTop,
  previewScene,
  previewSnapshot,
} from "../match/preview-table";
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
      draft: { source: "bolt", stage: "target", target: null },
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
    expect(paying).toEqual({ source: "bolt", stage: "payment", target: alex });
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
      draft: { source: "krenko", stage: "payment", target: null },
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
    expect(names.sort()).toEqual(["Lightning Bolt", "Shock"]);
  });
});

describe("casting in the preview", () => {
  const table = previewSnapshot();
  const you = table.viewerSeatId;
  const offers = previewCastOffers(table, true);
  const bolted = offers.find((offer) => offer.targets !== null)!;
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
    const after = previewCast(table, bolted.source, alex);
    expect(playerSnapshotSchema.safeParse(after).error?.issues ?? []).toEqual(
      [],
    );
    expect(seatBoard(after, you).hand).toHaveLength(6);
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
    const creature = offers.find((offer) => offer.targets === null)!;
    const cast = previewCast(table, creature.source, null);
    const before = seatBoard(table, you).front.length;
    expect(seatBoard(cast, you).front).toHaveLength(before);
    const resolved = previewResolveTop(cast);
    expect(playerSnapshotSchema.safeParse(resolved).success).toBe(true);
    expect(resolved.stack).toEqual([]);
    expect(seatBoard(resolved, you).front).toHaveLength(before + 1);
  });

  test("a card that is not in hand is not cast", () => {
    expect(previewCast(table, "elsewhere", null)).toBe(table);
  });
});

import type {
  PlayerDecision,
  PlayerObject,
  PlayerSnapshot,
} from "@spellwake/application-contracts";

/**
 * The preview match: six seats around one table, each with a small Goblin
 * board built from the preview catalogue's cards, on the viewer's first main
 * phase. It is a fixed starting position, not a rules engine.
 */
const seat = (n: number) =>
  `00000000-0000-4000-8000-0000000001${String(n).padStart(2, "0")}`;
export const PREVIEW_SEATS = [
  { seatId: seat(1), name: "You" },
  { seatId: seat(2), name: "Alex" },
  { seatId: seat(3), name: "Morgan" },
  { seatId: seat(4), name: "Sam" },
  { seatId: seat(5), name: "Riley" },
  { seatId: seat(6), name: "Jordan" },
] as const;
export const PREVIEW_VIEWER = PREVIEW_SEATS[0].seatId;
export const PREVIEW_GAME = "00000000-0000-4000-8000-000000000201";

type Kind = "land" | "creature" | "artifact" | "instant" | "sorcery";
const CARDS: Record<
  string,
  {
    type: string;
    cost: string | null;
    kind: Kind;
    stats?: [string, string];
    rules: string;
  }
> = {
  Mountain: {
    type: "Basic Land — Mountain",
    cost: null,
    kind: "land",
    rules: "({T}: Add {R}.)",
  },
  "Goblin Guide": {
    type: "Creature — Goblin Scout",
    cost: "{R}",
    kind: "creature",
    stats: ["2", "2"],
    rules:
      "Haste\nWhenever Goblin Guide attacks, defending player reveals the top card of their library. If it’s a land card, that player puts it into their hand.",
  },
  "Krenko, Mob Boss": {
    type: "Legendary Creature — Goblin Warrior",
    cost: "{2}{R}{R}",
    kind: "creature",
    stats: ["3", "3"],
    rules:
      "{T}: Create X 1/1 red Goblin creature tokens, where X is the number of Goblins you control.",
  },
  "Goblin Chieftain": {
    type: "Creature — Goblin",
    cost: "{1}{R}{R}",
    kind: "creature",
    stats: ["2", "2"],
    rules:
      "Haste\nOther Goblin creatures you control get +1/+1 and have haste.",
  },
  "Goblin Matron": {
    type: "Creature — Goblin",
    cost: "{2}{R}",
    kind: "creature",
    stats: ["1", "1"],
    rules:
      "When Goblin Matron enters, you may search your library for a Goblin card, reveal that card, put it into your hand, then shuffle.",
  },
  "Mogg War Marshal": {
    type: "Creature — Goblin Warrior",
    cost: "{1}{R}",
    kind: "creature",
    stats: ["1", "1"],
    rules:
      "Echo {1}{R}\nWhen Mogg War Marshal enters or dies, create a 1/1 red Goblin creature token.",
  },
  "Goblin Piledriver": {
    type: "Creature — Goblin Warrior",
    cost: "{1}{R}",
    kind: "creature",
    stats: ["1", "2"],
    rules:
      "Protection from blue\nWhenever Goblin Piledriver attacks, it gets +2/+0 until end of turn for each other attacking Goblin.",
  },
  "Legion Loyalist": {
    type: "Creature — Goblin Soldier",
    cost: "{R}",
    kind: "creature",
    stats: ["1", "1"],
    rules:
      "Haste\nBattalion — Whenever Legion Loyalist and at least two other creatures attack, creatures you control gain first strike and trample until end of turn.",
  },
  "Skirk Prospector": {
    type: "Creature — Goblin",
    cost: "{R}",
    kind: "creature",
    stats: ["1", "1"],
    rules: "Sacrifice a Goblin: Add {R}.",
  },
  "Aether Vial": {
    type: "Artifact",
    cost: "{1}",
    kind: "artifact",
    rules:
      "At the beginning of your upkeep, you may put a charge counter on Aether Vial.\n{T}: You may put a creature card with mana value equal to the number of charge counters on Aether Vial from your hand onto the battlefield.",
  },
  "Lightning Bolt": {
    type: "Instant",
    cost: "{R}",
    kind: "instant",
    rules: "Lightning Bolt deals 3 damage to any target.",
  },
  Shock: {
    type: "Instant",
    cost: "{R}",
    kind: "instant",
    rules: "Shock deals 2 damage to any target.",
  },
  "Krenko’s Command": {
    type: "Sorcery",
    cost: "{1}{R}",
    kind: "sorcery",
    rules: "Create two 1/1 red Goblin creature tokens.",
  },
};
/** The catalogue spells this name with a plain apostrophe. */
export const catalogueName = (name: string): string =>
  name.replaceAll("’", "'");

function card(
  handle: string,
  name: string,
  owner: string,
  zone: string,
  tapped = false,
): PlayerObject {
  const facts = CARDS[name]!;
  const isPermanent =
    facts.kind === "land" ||
    facts.kind === "creature" ||
    facts.kind === "artifact";
  return {
    handle,
    zone,
    ownerSeatId: owner,
    controllerSeatId: isPermanent && zone === "zone-battlefield" ? owner : null,
    identity: {
      kind: "known",
      definitionId: null,
      printingId: null,
      characteristics: {
        name,
        typeLine: facts.type,
        rulesText: facts.rules,
        manaCost: facts.cost,
        printedPower: facts.stats?.[0] ?? null,
        printedToughness: facts.stats?.[1] ?? null,
        printedLoyalty: null,
      },
      currentNames: [name],
    },
    publicProperties: {
      cardTypes: [facts.kind],
      supertypes: [
        ...(name === "Mountain" ? (["basic"] as const) : []),
        ...(name.startsWith("Krenko,") ? (["legendary"] as const) : []),
      ],
      landTypes: name === "Mountain" ? ["Mountain"] : [],
      creatureTypes: facts.kind === "creature" ? ["Goblin"] : [],
      colors: facts.kind === "land" || facts.kind === "artifact" ? [] : ["red"],
      keywords: facts.rules.startsWith("Haste") ? ["haste"] : [],
      power: facts.stats?.[0] ?? null,
      toughness: facts.stats?.[1] ?? null,
    },
    tapped,
    counters: [],
    abilities:
      name === "Mountain"
        ? [{ handle: `${handle}:mana`, kind: "mana", label: "{T}: Add {R}." }]
        : [],
    damageMarked: "0",
    attachedTo: null,
  };
}

/** What each seat has on the table. The viewer's hand is known; the rest are counted. */
const BOARDS: {
  creatures: string[];
  lands: number;
  tapped?: number;
  artifacts?: string[];
  graveyard?: string[];
  hand: number;
  life: string;
}[] = [
  {
    creatures: ["Goblin Guide", "Goblin Chieftain", "Mogg War Marshal"],
    lands: 4,
    artifacts: ["Aether Vial"],
    graveyard: ["Shock"],
    hand: 0,
    life: "20",
  },
  {
    creatures: ["Krenko, Mob Boss", "Goblin Matron", "Skirk Prospector"],
    lands: 4,
    tapped: 2,
    hand: 5,
    life: "17",
  },
  {
    creatures: ["Goblin Piledriver", "Legion Loyalist"],
    lands: 3,
    artifacts: ["Aether Vial"],
    hand: 6,
    life: "20",
  },
  {
    creatures: ["Goblin Chieftain", "Goblin Guide", "Mogg War Marshal"],
    lands: 5,
    tapped: 1,
    graveyard: ["Lightning Bolt", "Shock"],
    hand: 4,
    life: "14",
  },
  {
    creatures: ["Skirk Prospector", "Goblin Matron"],
    lands: 3,
    hand: 7,
    life: "20",
  },
  {
    creatures: ["Legion Loyalist", "Krenko, Mob Boss", "Goblin Piledriver"],
    lands: 4,
    hand: 5,
    life: "19",
  },
];
const VIEWER_HAND = [
  "Lightning Bolt",
  "Mountain",
  "Krenko, Mob Boss",
  "Goblin Matron",
  "Shock",
  "Krenko’s Command",
  "Goblin Piledriver",
];

/** The table at the start of the preview. */
export function previewSnapshot(): PlayerSnapshot {
  const objects: PlayerObject[] = [];
  const battlefield: string[] = [];
  const zones: PlayerSnapshot["zones"] = [];
  PREVIEW_SEATS.forEach(({ seatId }, index) => {
    const board = BOARDS[index]!;
    const tag = `s${index + 1}`;
    const put = (name: string, at: number, tapped = false) => {
      const handle = `obj-${tag}-${name.toLowerCase().replaceAll(/[^a-z]+/g, "-")}-${at}`;
      objects.push(card(handle, name, seatId, "zone-battlefield", tapped));
      battlefield.push(handle);
    };
    board.creatures.forEach((name, at) => put(name, at));
    for (let at = 0; at < board.lands; at += 1)
      put("Mountain", at, at < (board.tapped ?? 0));
    (board.artifacts ?? []).forEach((name, at) => put(name, at));
    const graveyard = (board.graveyard ?? []).map((name, at) => {
      const handle = `obj-${tag}-gy-${at}`;
      objects.push(card(handle, name, seatId, `zone-graveyard-${tag}`));
      return handle;
    });
    const isViewer = seatId === PREVIEW_VIEWER;
    const hand = isViewer
      ? VIEWER_HAND.map((name, at) => {
          const handle = `obj-hand-${at}`;
          objects.push(card(handle, name, seatId, "zone-hand-you"));
          return handle;
        })
      : [];
    zones.push(
      isViewer
        ? {
            handle: "zone-hand-you",
            zoneType: "hand",
            ownerSeatId: seatId,
            visibility: "visible",
            objects: hand,
            orderKnown: false,
          }
        : {
            handle: `zone-hand-${tag}`,
            zoneType: "hand",
            ownerSeatId: seatId,
            visibility: "hidden",
            hiddenCount: String(board.hand),
          },
      {
        handle: `zone-library-${tag}`,
        zoneType: "library",
        ownerSeatId: seatId,
        visibility: "hidden",
        hiddenCount: "40",
      },
      {
        handle: `zone-graveyard-${tag}`,
        zoneType: "graveyard",
        ownerSeatId: seatId,
        visibility: "visible",
        objects: graveyard,
        orderKnown: true,
      },
    );
  });
  return {
    basis: {
      gameId: PREVIEW_GAME,
      contextId: "00000000-0000-4000-8000-000000000202",
      epoch: "1",
      revision: "12",
    },
    viewerSeatId: PREVIEW_VIEWER,
    players: PREVIEW_SEATS.map(({ seatId }, index) => ({
      seatId,
      life: BOARDS[index]!.life,
      poison: "0",
      counters: [],
      manaPool: [],
      handCount: String(
        seatId === PREVIEW_VIEWER ? VIEWER_HAND.length : BOARDS[index]!.hand,
      ),
      libraryCount: "40",
      eliminated: false,
    })),
    turn: {
      number: "5",
      activeSeatIds: [PREVIEW_VIEWER],
      prioritySeatIds: [PREVIEW_VIEWER],
      phase: "precombat-main",
      step: "precombat-main",
    },
    visibleObjects: objects,
    zones: [
      {
        handle: "zone-battlefield",
        zoneType: "battlefield",
        ownerSeatId: null,
        visibility: "visible",
        objects: battlefield,
        orderKnown: false,
      },
      {
        handle: "zone-stack",
        zoneType: "stack",
        ownerSeatId: null,
        visibility: "visible",
        objects: [],
        orderKnown: true,
      },
      {
        handle: "zone-exile",
        zoneType: "exile",
        ownerSeatId: null,
        visibility: "visible",
        objects: [],
        orderKnown: false,
      },
      {
        handle: "zone-command",
        zoneType: "command",
        ownerSeatId: null,
        visibility: "visible",
        objects: [],
        orderKnown: false,
      },
      ...zones,
    ],
    stack: [],
    decisions: [],
    throughEventSequence: "12",
  };
}

const creaturesOf = (snapshot: PlayerSnapshot, seatId: string) =>
  snapshot.visibleObjects.filter(
    (object) =>
      object.zone === "zone-battlefield" &&
      object.controllerSeatId === seatId &&
      !object.tapped &&
      (object.publicProperties?.cardTypes.includes("creature") ?? false),
  );
const decision = (
  snapshot: PlayerSnapshot,
  kind: "declare-attackers" | "declare-blockers",
  domain: PlayerDecision["domain"],
): PlayerDecision => ({
  decision: `decision-${kind}-${snapshot.turn?.number ?? "0"}`,
  basis: snapshot.basis,
  respondingSeatIds: [snapshot.viewerSeatId],
  visibility: "responding-seat",
  kind,
  domain,
  deadlineAt: null,
});

/** The viewer's attack: each untapped creature may be sent at any other player. */
export function previewAttackDecision(
  snapshot: PlayerSnapshot,
): PlayerDecision | null {
  const attackers = creaturesOf(snapshot, snapshot.viewerSeatId);
  if (attackers.length === 0) return null;
  const defenders = snapshot.players
    .filter((player) => player.seatId !== snapshot.viewerSeatId)
    .map((player) => ({ kind: "seat" as const, seatId: player.seatId }));
  return decision(snapshot, "declare-attackers", {
    kind: "attackers",
    choices: attackers.map((object) => ({
      attacker: object.handle,
      defenders,
    })),
    required: [],
  });
}

/** Another seat's creatures attack the viewer, who may block each with one creature. */
export function previewBlockDecision(
  snapshot: PlayerSnapshot,
  attackerSeat: string,
): PlayerDecision | null {
  const attackers = creaturesOf(snapshot, attackerSeat)
    .slice(0, 2)
    .map((object) => object.handle);
  const blockers = creaturesOf(snapshot, snapshot.viewerSeatId);
  if (attackers.length === 0 || blockers.length === 0) return null;
  return decision(snapshot, "declare-blockers", {
    kind: "blockers",
    defendingSeatId: snapshot.viewerSeatId,
    choices: blockers.map((object) => ({
      blocker: object.handle,
      attackers,
      minimum: "0",
      maximum: "1",
    })),
    required: [],
  });
}

/** The preview table part-way through combat, for the development probe. */
export function previewScene(scene: "attack" | "block"): PlayerSnapshot {
  const table = previewSnapshot();
  if (scene === "attack") {
    const combat: PlayerSnapshot = {
      ...table,
      turn: { ...table.turn!, phase: "combat", step: "declare-attackers" },
    };
    const offered = previewAttackDecision(combat);
    return offered ? { ...combat, decisions: [offered] } : combat;
  }
  const attackerSeat = PREVIEW_SEATS[1].seatId;
  const combat: PlayerSnapshot = {
    ...table,
    turn: {
      ...table.turn!,
      number: "6",
      activeSeatIds: [attackerSeat],
      phase: "combat",
      step: "declare-blockers",
    },
  };
  const offered = previewBlockDecision(combat, attackerSeat);
  if (!offered || offered.domain.kind !== "blockers") return combat;
  const attacking = new Set(
    offered.domain.choices.flatMap((choice) => choice.attackers),
  );
  return {
    ...combat,
    visibleObjects: combat.visibleObjects.map((object) =>
      attacking.has(object.handle) ? { ...object, tapped: true } : object,
    ),
    decisions: [offered],
  };
}

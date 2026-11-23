import type {
  PlayerDecision,
  PlayerObject,
  PlayerSnapshot,
} from "@spellwake/application-contracts";
import type { CastOffer } from "./cast-draft";

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
  Abrade: {
    type: "Instant",
    cost: "{1}{R}",
    kind: "instant",
    rules:
      "Choose one —\n• Abrade deals 3 damage to target creature.\n• Destroy target artifact.",
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
  "Abrade",
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
  kind: "declare-attackers" | "declare-blockers" | "assign-combat-damage",
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

const powerOf = (object: PlayerObject): number =>
  Number(object.publicProperties?.power ?? "0") || 0;
const withPower = (
  snapshot: PlayerSnapshot,
  handle: string,
  power: string,
): PlayerSnapshot => ({
  ...snapshot,
  visibleObjects: snapshot.visibleObjects.map((object) =>
    object.handle === handle && object.publicProperties
      ? {
          ...object,
          publicProperties: { ...object.publicProperties, power },
        }
      : object,
  ),
});

/**
 * One of the viewer's attackers is blocked by two of the defender's creatures,
 * so the viewer divides its damage between them. Null when the defender has
 * fewer than two creatures to block with.
 */
export function previewDamageDecision(
  snapshot: PlayerSnapshot,
  blocked: readonly { attacker: string; defenderSeat: string }[],
): PlayerDecision | null {
  const sources = blocked.flatMap(({ attacker, defenderSeat }) => {
    const source = snapshot.visibleObjects.find(
      (object) => object.handle === attacker,
    );
    const blockers = creaturesOf(snapshot, defenderSeat).slice(0, 2);
    if (!source || blockers.length < 2 || powerOf(source) === 0) return [];
    return [
      {
        source: attacker,
        targets: blockers.map((object) => ({
          kind: "object" as const,
          handle: object.handle,
        })),
        total: String(powerOf(source)),
      },
    ];
  });
  return sources.length > 0
    ? decision(snapshot, "assign-combat-damage", {
        kind: "combat-damage",
        sources,
      })
    : null;
}

type StackTarget = PlayerSnapshot["stack"][number]["targets"][number];
/** Put a spell card on the stack, newest last, as the game would report it. */
function castOnto(
  snapshot: PlayerSnapshot,
  name: string,
  controller: string,
  target: StackTarget,
): PlayerSnapshot {
  const at = snapshot.stack.length + 1;
  const tag = `${snapshot.throughEventSequence}-${at}`;
  const handle = `obj-spell-${tag}`;
  return {
    ...snapshot,
    visibleObjects: [
      ...snapshot.visibleObjects,
      card(handle, name, controller, "zone-stack"),
    ],
    zones: snapshot.zones.map((zone) =>
      zone.handle === "zone-stack" && zone.visibility === "visible"
        ? { ...zone, objects: [...zone.objects, handle] }
        : zone,
    ),
    stack: [
      ...snapshot.stack,
      {
        handle: `stack-${tag}`,
        controllerSeatId: controller,
        sourceObject: handle,
        label: name,
        targets: [target],
        unavailableTargets: "0",
      },
    ],
  };
}
/** A triggered ability of a permanent already on the table. */
function triggerOnto(
  snapshot: PlayerSnapshot,
  source: PlayerObject,
  effect: string,
): PlayerSnapshot {
  return {
    ...snapshot,
    stack: [
      ...snapshot.stack,
      {
        handle: `stack-${snapshot.throughEventSequence}-${snapshot.stack.length + 1}`,
        controllerSeatId: source.controllerSeatId ?? source.ownerSeatId ?? "",
        sourceObject: source.handle,
        label: `Triggered ability: ${effect}`,
        targets: [],
        unavailableTargets: "0",
      },
    ],
  };
}

/** Alex's burn spell at one of the viewer's creatures, and Morgan's reply at the viewer. */
export function previewResponses(snapshot: PlayerSnapshot): PlayerSnapshot {
  const mine = creaturesOf(snapshot, snapshot.viewerSeatId)[1];
  const bolt = castOnto(
    snapshot,
    "Lightning Bolt",
    PREVIEW_SEATS[1].seatId,
    mine
      ? { kind: "object", handle: mine.handle }
      : { kind: "seat", seatId: snapshot.viewerSeatId },
  );
  return castOnto(bolt, "Shock", PREVIEW_SEATS[2].seatId, {
    kind: "seat",
    seatId: snapshot.viewerSeatId,
  });
}

const DAMAGE: Record<string, number> = { "Lightning Bolt": 3, Shock: 2 };

const kindOf = (object: PlayerObject): Kind | undefined =>
  object.identity.kind === "known"
    ? CARDS[object.identity.characteristics.name]?.kind
    : undefined;
const costOf = (object: PlayerObject): string =>
  object.identity.kind === "known"
    ? (object.identity.characteristics.manaCost ?? "")
    : "";
/** A cost's size in mana: every card here costs red and generic mana only. */
const manaValue = (cost: string): number =>
  Array.from(cost.matchAll(/\{([^}]+)\}/g)).reduce(
    (total, match) => total + (Number(match[1]) || 1),
    0,
  );
/** The viewer's untapped Mountains, rightmost first, as an automatic payment would take them. */
const untappedLands = (snapshot: PlayerSnapshot): string[] =>
  snapshot.visibleObjects
    .filter(
      (object) =>
        object.zone === "zone-battlefield" &&
        object.controllerSeatId === snapshot.viewerSeatId &&
        !object.tapped &&
        kindOf(object) === "land",
    )
    .map((object) => object.handle)
    .reverse();

/**
 * What the preview offers to cast, standing in for offers the game does not
 * send yet: burn at any creature or player whenever the viewer may act, and
 * creatures on the viewer's own main phase with nothing waiting. A card is
 * offered only when untapped Mountains cover its cost.
 */
export function previewCastOffers(
  snapshot: PlayerSnapshot,
  /** With Auto-tap mana off, the preview pauses for payment. */
  autoTap: boolean,
): CastOffer[] {
  const viewer = snapshot.viewerSeatId;
  const turn = snapshot.turn;
  if (!turn?.prioritySeatIds.includes(viewer) || snapshot.decisions.length > 0)
    return [];
  const lands = untappedLands(snapshot);
  const ownMain =
    turn.activeSeatIds.includes(viewer) &&
    (turn.phase === "precombat-main" || turn.phase === "postcombat-main") &&
    snapshot.stack.length === 0;
  const anyTarget = [
    ...snapshot.visibleObjects
      .filter(
        (object) =>
          object.zone === "zone-battlefield" && kindOf(object) === "creature",
      )
      .map((object) => ({ kind: "object" as const, handle: object.handle })),
    ...snapshot.players
      .filter((player) => !player.eliminated)
      .map((player) => ({ kind: "seat" as const, seatId: player.seatId })),
  ];
  const permanents = (kind: Kind) =>
    snapshot.visibleObjects
      .filter(
        (object) =>
          object.zone === "zone-battlefield" && kindOf(object) === kind,
      )
      .map((object) => ({ kind: "object" as const, handle: object.handle }));
  const artifacts = permanents("artifact");
  return snapshot.visibleObjects.flatMap((object): CastOffer[] => {
    if (object.zone !== "zone-hand-you") return [];
    const kind = kindOf(object);
    const cost = costOf(object);
    const needed = manaValue(cost);
    const burn = kind === "instant";
    if (!(burn || (kind === "creature" && ownMain)) || needed > lands.length)
      return [];
    const modal =
      object.identity.kind === "known" &&
      object.identity.characteristics.name === "Abrade";
    return [
      {
        source: object.handle,
        ...(modal
          ? {
              modes: [
                {
                  mode: 0,
                  label: "Abrade deals 3 damage to target creature.",
                  targets: permanents("creature"),
                },
                {
                  mode: 1,
                  label: "Destroy target artifact.",
                  targets: artifacts,
                  ...(artifacts.length === 0
                    ? { unavailableReason: "No legal targets" }
                    : {}),
                },
              ],
            }
          : {}),
        targets: modal ? null : burn ? anyTarget : null,
        cost,
        payment: autoTap ? "automatic" : "paused",
        paySources: lands.slice(0, needed),
      },
    ];
  });
}

/**
 * The viewer casts a card from hand: its cost taps Mountains, and the card
 * moves to the stack with the target it was given.
 */
export function previewCast(
  snapshot: PlayerSnapshot,
  source: string,
  target: StackTarget | null,
  /** A modal spell's chosen modes, by the game's mode numbers. */
  modes?: readonly number[],
): PlayerSnapshot {
  const spell = snapshot.visibleObjects.find(
    (object) => object.handle === source && object.zone === "zone-hand-you",
  );
  if (!spell || spell.identity.kind !== "known") return snapshot;
  const paid = new Set(
    untappedLands(snapshot).slice(0, manaValue(costOf(spell))),
  );
  const sequence = String(Number(snapshot.throughEventSequence) + 1);
  return {
    ...snapshot,
    throughEventSequence: sequence,
    players: snapshot.players.map((player) =>
      player.seatId === snapshot.viewerSeatId
        ? { ...player, handCount: String(Number(player.handCount) - 1) }
        : player,
    ),
    visibleObjects: snapshot.visibleObjects.map((object) =>
      object.handle === source
        ? { ...object, zone: "zone-stack" }
        : paid.has(object.handle)
          ? { ...object, tapped: true }
          : object,
    ),
    zones: snapshot.zones.map((zone) => {
      if (zone.visibility !== "visible") return zone;
      if (zone.handle === "zone-hand-you")
        return {
          ...zone,
          objects: zone.objects.filter((handle) => handle !== source),
        };
      if (zone.handle === "zone-stack")
        return { ...zone, objects: [...zone.objects, source] };
      return zone;
    }),
    stack: [
      ...snapshot.stack,
      {
        handle: `stack-${sequence}-${snapshot.stack.length + 1}`,
        controllerSeatId: snapshot.viewerSeatId,
        sourceObject: source,
        label: spell.identity.characteristics.name,
        targets: target ? [target] : [],
        unavailableTargets: "0",
        ...(modes?.length ? { modes: [...modes] } : {}),
      },
    ],
  };
}
/**
 * The newest stack item resolves: a burn spell deals its damage and goes to
 * its owner's graveyard, and a creature dealt lethal damage follows it. This
 * is the preview's own small script, not the rules engine.
 */
export function previewResolveTop(snapshot: PlayerSnapshot): PlayerSnapshot {
  const top = snapshot.stack.at(-1);
  if (!top) return snapshot;
  const spell = snapshot.visibleObjects.find(
    (object) =>
      object.handle === top.sourceObject && object.zone === "zone-stack",
  );
  const spellName =
    spell?.identity.kind === "known" ? spell.identity.characteristics.name : "";
  // Abrade's second mode destroys an artifact instead of dealing damage.
  const destroys = spellName === "Abrade" && top.modes?.[0] === 1;
  const amount =
    spellName === "Abrade" ? (destroys ? 0 : 3) : (DAMAGE[spellName] ?? 0);
  const target = top.targets[0];
  const struck =
    target?.kind === "object"
      ? snapshot.visibleObjects.find(
          (object) => object.handle === target.handle,
        )
      : undefined;
  const dies =
    struck !== undefined &&
    (destroys ||
      Number(struck.damageMarked) + amount >=
        (Number(struck.publicProperties?.toughness ?? "0") || Infinity));
  const leaving = new Map<string, string>();
  const graveyardOf = (seatId: string | null) =>
    `zone-graveyard-s${PREVIEW_SEATS.findIndex((seat) => seat.seatId === seatId) + 1}`;
  // A creature spell becomes a permanent; any other spell is put away.
  const enters = spell !== undefined && kindOf(spell) === "creature";
  if (spell)
    leaving.set(
      spell.handle,
      enters ? "zone-battlefield" : graveyardOf(spell.ownerSeatId),
    );
  if (struck && dies)
    leaving.set(struck.handle, graveyardOf(struck.ownerSeatId));
  return {
    ...snapshot,
    throughEventSequence: String(Number(snapshot.throughEventSequence) + 1),
    stack: snapshot.stack.slice(0, -1),
    players: snapshot.players.map((player) =>
      target?.kind === "seat" && target.seatId === player.seatId
        ? { ...player, life: String(Number(player.life) - amount) }
        : player,
    ),
    visibleObjects: snapshot.visibleObjects.map((object) => {
      const to = leaving.get(object.handle);
      if (to)
        return {
          ...object,
          zone: to,
          controllerSeatId:
            to === "zone-battlefield" ? object.ownerSeatId : null,
          tapped: false,
          damageMarked: "0",
        };
      return object === struck
        ? {
            ...object,
            damageMarked: String(Number(object.damageMarked) + amount),
          }
        : object;
    }),
    zones: snapshot.zones.map((zone) => {
      if (zone.visibility !== "visible") return zone;
      const kept = zone.objects.filter((handle) => !leaving.has(handle));
      const arriving = [...leaving]
        .filter(([, to]) => to === zone.handle)
        .map(([handle]) => handle);
      return kept.length === zone.objects.length && arriving.length === 0
        ? zone
        : { ...zone, objects: [...kept, ...arriving] };
    }),
  };
}

/**
 * The viewer concedes and stays to watch: their cards leave the table, they
 * are out of the game, and a turn that was theirs passes to Alex.
 */
export function previewConcede(snapshot: PlayerSnapshot): PlayerSnapshot {
  const viewer = snapshot.viewerSeatId;
  const gone = new Set(
    snapshot.visibleObjects
      .filter((object) => object.ownerSeatId === viewer)
      .map((object) => object.handle),
  );
  const next = PREVIEW_SEATS[1].seatId;
  const others = (seats: readonly string[]) => {
    const kept = seats.filter((seat) => seat !== viewer);
    return kept.length > 0 ? kept : [next];
  };
  return {
    ...snapshot,
    throughEventSequence: String(Number(snapshot.throughEventSequence) + 1),
    players: snapshot.players.map((player) =>
      player.seatId === viewer
        ? {
            ...player,
            eliminated: true,
            handCount: "0",
            libraryCount: "0",
            manaPool: [],
          }
        : player,
    ),
    turn: snapshot.turn && {
      ...snapshot.turn,
      activeSeatIds: others(snapshot.turn.activeSeatIds),
      prioritySeatIds: others(snapshot.turn.prioritySeatIds),
    },
    visibleObjects: snapshot.visibleObjects.filter(
      (object) => !gone.has(object.handle),
    ),
    zones: snapshot.zones.map((zone) =>
      zone.visibility === "visible"
        ? {
            ...zone,
            objects: zone.objects.filter((handle) => !gone.has(handle)),
          }
        : zone.ownerSeatId === viewer
          ? { ...zone, hiddenCount: "0" }
          : zone,
    ),
    stack: snapshot.stack.filter((entry) => entry.controllerSeatId !== viewer),
    decisions: [],
  };
}

export type PreviewScene =
  | "attack"
  | "block"
  | "damage"
  | "damage-two"
  | "trample"
  | "stack"
  | "stack-long"
  | "stack-waiting"
  | "watching"
  | "no-artifacts";

/** Combat damage being assigned on the viewer's turn. */
function damageScene(
  scene: "damage" | "damage-two" | "trample",
): PlayerSnapshot {
  const table = previewSnapshot();
  const [guide, chieftain] = creaturesOf(table, PREVIEW_VIEWER);
  const alex = PREVIEW_SEATS[1].seatId;
  const morgan = PREVIEW_SEATS[2].seatId;
  if (!guide || !chieftain) return table;
  // The Chieftain has been pumped, so there is damage worth dividing.
  const pumped = withPower(
    table,
    chieftain.handle,
    scene === "trample" ? "6" : "4",
  );
  const attacking = new Set(
    scene === "damage-two"
      ? [chieftain.handle, guide.handle]
      : [chieftain.handle],
  );
  const combat: PlayerSnapshot = {
    ...pumped,
    turn: { ...pumped.turn!, phase: "combat", step: "combat-damage" },
    visibleObjects: pumped.visibleObjects.map((object) =>
      attacking.has(object.handle) ? { ...object, tapped: true } : object,
    ),
  };
  if (scene === "trample") {
    const blocker = creaturesOf(combat, alex)[1];
    if (!blocker) return combat;
    return {
      ...combat,
      decisions: [
        decision(combat, "assign-combat-damage", {
          kind: "combat-damage",
          sources: [
            {
              source: chieftain.handle,
              targets: [
                { kind: "object", handle: blocker.handle },
                { kind: "seat", seatId: alex },
              ],
              total: "6",
              trampleLethal: [{ blocker: blocker.handle, amount: "1" }],
            },
          ],
        }),
      ],
    };
  }
  const offered = previewDamageDecision(
    combat,
    scene === "damage-two"
      ? [
          { attacker: chieftain.handle, defenderSeat: alex },
          { attacker: guide.handle, defenderSeat: morgan },
        ]
      : [{ attacker: chieftain.handle, defenderSeat: alex }],
  );
  return offered ? { ...combat, decisions: [offered] } : combat;
}

/** Spells waiting to resolve on Alex's main phase. */
function stackScene(
  scene: "stack" | "stack-long" | "stack-waiting",
): PlayerSnapshot {
  const table = previewSnapshot();
  const alex = PREVIEW_SEATS[1].seatId;
  const main: PlayerSnapshot = {
    ...table,
    turn: {
      ...table.turn!,
      number: "6",
      activeSeatIds: [alex],
      prioritySeatIds: [scene === "stack-waiting" ? alex : PREVIEW_VIEWER],
    },
  };
  if (scene !== "stack-long") return previewResponses(main);
  // A deeper pile: four more replies around the table, and a trigger on top.
  const sam = PREVIEW_SEATS[3].seatId;
  let pile = castOnto(main, "Lightning Bolt", alex, {
    kind: "seat",
    seatId: PREVIEW_VIEWER,
  });
  const replies: [string, string, StackTarget][] = [
    ["Shock", PREVIEW_SEATS[2].seatId, { kind: "seat", seatId: alex }],
    ["Lightning Bolt", sam, { kind: "seat", seatId: PREVIEW_SEATS[2].seatId }],
    ["Shock", PREVIEW_SEATS[4].seatId, { kind: "seat", seatId: sam }],
    ["Lightning Bolt", PREVIEW_SEATS[5].seatId, { kind: "seat", seatId: alex }],
  ];
  for (const [name, controller, target] of replies)
    pile = castOnto(pile, name, controller, target);
  const marshal = creaturesOf(pile, sam)[2];
  return marshal
    ? triggerOnto(pile, marshal, "Create a 1/1 red Goblin creature token.")
    : pile;
}

/** The preview table part-way through a turn, for the development probe. */
export function previewScene(scene: PreviewScene): PlayerSnapshot {
  if (scene === "damage" || scene === "damage-two" || scene === "trample")
    return damageScene(scene);
  if (scene === "stack" || scene === "stack-long" || scene === "stack-waiting")
    return stackScene(scene);
  if (scene === "watching") return previewConcede(previewSnapshot());
  if (scene === "no-artifacts") {
    // No artifact is on the table, so a spell that destroys one has no target.
    const table = previewSnapshot();
    const gone = new Set(
      table.visibleObjects
        .filter(
          (object) =>
            object.zone === "zone-battlefield" && kindOf(object) === "artifact",
        )
        .map((object) => object.handle),
    );
    return {
      ...table,
      visibleObjects: table.visibleObjects.filter(
        (object) => !gone.has(object.handle),
      ),
      zones: table.zones.map((zone) =>
        zone.visibility === "visible"
          ? {
              ...zone,
              objects: zone.objects.filter((handle) => !gone.has(handle)),
            }
          : zone,
      ),
    };
  }
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

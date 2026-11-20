/** Projected public records; expected facts still require a qualified native projector. */
import { z } from "zod";
import { amountSchema, identifierSchema } from "./primitives.ts";
import {
  playerBasisSchema,
  playerHandleSchema,
  playerDisplayTextSchema,
  playerEnglishLabelSchema,
  rulesCountSchema,
  rulesIntegerSchema,
  type PlayerBasis,
} from "./player-primitives.ts";
import {
  PLAYER_ITEMS,
  playerManaColorSchema,
  playerManaSpendSchema,
  playerTargetSchema,
  playerObjectTargetSchema,
  playerSeatTargetSchema,
  uniquePlayerValues,
  type PlayerTarget,
} from "./player-actions.ts";
import {
  playerDecisionSchema,
  playerDeadlineSchema,
  samePlayerBasis,
  validPlayerDecision,
  playerDecisionReferences,
  type PlayerDecision,
} from "./player-decisions.ts";
/** Current participant view request; viewer, artifacts and disclosure are server-owned. */
export const playerViewRequestSchema = z.strictObject({
  gameId: identifierSchema,
  contextId: identifierSchema,
});
export type PlayerViewRequest = z.infer<typeof playerViewRequestSchema>;

export const playerZoneKindSchema = z.enum([
  "library",
  "hand",
  "graveyard",
  "battlefield",
  "stack",
  "exile",
  "command",
]);
export const playerCardTypeSchema = z.enum([
  "land",
  "artifact",
  "creature",
  "enchantment",
  "kindred",
  "instant",
  "sorcery",
]);
export const playerSupertypeSchema = z.enum([
  "basic",
  "legendary",
  "ongoing",
  "snow",
  "world",
]);
export const playerLandTypeSchema = z.enum([
  "Cave",
  "Desert",
  "Forest",
  "Gate",
  "Island",
  "Lair",
  "Locus",
  "Mine",
  "Mountain",
  "Plains",
  "Planet",
  "PowerPlant",
  "Sphere",
  "Swamp",
  "Tower",
  "Town",
  "Urzas",
]);
export const playerAbilityKindSchema = z.enum([
  "mana",
  "activated",
  "triggered",
  "continuous",
]);
export const playerAbilityViewSchema = z.strictObject({
  handle: playerHandleSchema,
  kind: playerAbilityKindSchema,
  label: playerDisplayTextSchema(1024).refine((v) => v.length > 0),
});
export const playerKeywordSchema = z.enum(["haste", "vigilance"]);
export const playerPhaseSchema = z.enum([
  "beginning",
  "precombat-main",
  "combat",
  "postcombat-main",
  "ending",
]);
export const playerStepSchema = z.enum([
  "untap",
  "upkeep",
  "draw",
  "precombat-main",
  "beginning-combat",
  "declare-attackers",
  "declare-blockers",
  "combat-damage",
  "end-combat",
  "postcombat-main",
  "end",
  "cleanup",
]);
export const playerCounterSchema = z.strictObject({
  name: playerDisplayTextSchema(128).refine((v) => v.length > 0),
  count: rulesCountSchema,
});
const counters = z
  .array(playerCounterSchema)
  .max(PLAYER_ITEMS)
  .refine((xs) => uniquePlayerValues(xs, (x) => x.name));
const handles = z
  .array(playerHandleSchema)
  .max(PLAYER_ITEMS)
  .refine((xs) => uniquePlayerValues(xs, (x) => x));
export const playerCharacteristicsSchema = z.strictObject({
  name: playerDisplayTextSchema(256).refine((v) => v.length > 0),
  typeLine: playerDisplayTextSchema(1024),
  rulesText: playerDisplayTextSchema(8192),
  manaCost: playerDisplayTextSchema(256).nullable(),
  printedPower: playerDisplayTextSchema(128).nullable(),
  printedToughness: playerDisplayTextSchema(128).nullable(),
  printedLoyalty: playerDisplayTextSchema(128).nullable(),
});
export const playerObjectIdentitySchema = z.discriminatedUnion("kind", [
  z.strictObject({
    kind: z.literal("known"),
    definitionId: identifierSchema.nullable(),
    printingId: identifierSchema.nullable(),
    characteristics: playerCharacteristicsSchema,
    currentNames: z
      .array(playerEnglishLabelSchema)
      .max(PLAYER_ITEMS)
      .refine((xs) => uniquePlayerValues(xs, (x) => x))
      // Root ObjectNames is a BTreeSet ordered by Unicode scalar values.
      .transform((xs) =>
        xs.toSorted((a, b) => {
          const left = [...a];
          const right = [...b];
          for (let i = 0; i < Math.min(left.length, right.length); i++) {
            const x = left[i]?.codePointAt(0) ?? 0;
            const y = right[i]?.codePointAt(0) ?? 0;
            if (x !== y) return x - y;
          }
          return left.length - right.length;
        }),
      ),
  }),
  z.strictObject({ kind: z.literal("concealed") }),
]);
export const playerPublicPropertiesSchema = z.strictObject({
  cardTypes: z
    .array(playerCardTypeSchema)
    .max(7)
    .refine((xs) => uniquePlayerValues(xs, (x) => x)),
  supertypes: z
    .array(playerSupertypeSchema)
    .max(5)
    .refine((xs) => uniquePlayerValues(xs, (x) => x)),
  landTypes: z
    .array(playerLandTypeSchema)
    .max(17)
    .refine((xs) => uniquePlayerValues(xs, (x) => x)),
  creatureTypes: z
    .array(playerEnglishLabelSchema)
    .max(PLAYER_ITEMS)
    .refine((xs) => uniquePlayerValues(xs, (x) => x)),
  colors: z
    .array(playerManaColorSchema)
    .max(5)
    .refine(
      (xs) => uniquePlayerValues(xs, (x) => x) && !xs.includes("colourless"),
    ),
  keywords: z
    .array(playerKeywordSchema)
    .max(2)
    .refine((xs) => uniquePlayerValues(xs, (x) => x)),
  power: rulesIntegerSchema.nullable(),
  toughness: rulesIntegerSchema.nullable(),
});
export const playerVisibleSubjectSchema = z.union([
  playerObjectTargetSchema,
  playerSeatTargetSchema,
]);
export const playerObjectSchema = z.strictObject({
  handle: playerHandleSchema,
  zone: playerHandleSchema,
  ownerSeatId: identifierSchema,
  controllerSeatId: identifierSchema.nullable(),
  identity: playerObjectIdentitySchema,
  publicProperties: playerPublicPropertiesSchema.nullable(),
  tapped: z.boolean(),
  counters,
  abilities: z
    .array(playerAbilityViewSchema)
    .max(PLAYER_ITEMS)
    .refine((xs) => uniquePlayerValues(xs, (x) => x.handle)),
  damageMarked: rulesCountSchema,
  attachedTo: playerVisibleSubjectSchema.nullable(),
});
const zoneCommon = {
  handle: playerHandleSchema,
  zoneType: playerZoneKindSchema,
  ownerSeatId: identifierSchema.nullable(),
};
export const playerZoneSchema = z.discriminatedUnion("visibility", [
  z.strictObject({
    ...zoneCommon,
    visibility: z.literal("hidden"),
    hiddenCount: rulesCountSchema,
  }),
  z.strictObject({
    ...zoneCommon,
    visibility: z.literal("visible"),
    objects: handles,
    orderKnown: z.boolean(),
  }),
  z.strictObject({
    ...zoneCommon,
    visibility: z.literal("mixed"),
    objects: handles.refine((xs) => xs.length > 0),
    hiddenCount: rulesCountSchema.refine((v) => BigInt(v) > 0n),
    orderKnown: z.boolean(),
  }),
]);
export const playerSeatViewSchema = z.strictObject({
  seatId: identifierSchema,
  life: rulesIntegerSchema,
  poison: rulesCountSchema,
  counters,
  manaPool: z
    .array(playerManaSpendSchema)
    .max(6)
    .refine((xs) => uniquePlayerValues(xs, (x) => x.color)),
  handCount: rulesCountSchema,
  libraryCount: rulesCountSchema,
  eliminated: z.boolean(),
});
export const playerTurnSchema = z.strictObject({
  number: rulesCountSchema.refine((v) => BigInt(v) > 0n),
  activeSeatIds: z
    .array(identifierSchema)
    .max(6)
    .refine((xs) => uniquePlayerValues(xs, (x) => x)),
  prioritySeatIds: z
    .array(identifierSchema)
    .max(6)
    .refine((xs) => uniquePlayerValues(xs, (x) => x)),
  priorityDeadlineAt: playerDeadlineSchema.optional(),
  phase: playerPhaseSchema,
  step: playerStepSchema,
});
export const playerStackEntrySchema = z.strictObject({
  handle: playerHandleSchema,
  controllerSeatId: identifierSchema,
  sourceObject: playerHandleSchema.nullable(),
  label: playerDisplayTextSchema(1024).refine((v) => v.length > 0),
  targets: z.array(playerTargetSchema).max(PLAYER_ITEMS),
  // Count bound occurrences without a current permitted reference, not illegal
  // targets. Current visible targets retain their original order and duplicates.
  unavailableTargets: rulesCountSchema,
});
export const playerSnapshotSchema = z.strictObject({
  basis: playerBasisSchema,
  viewerSeatId: identifierSchema,
  players: z.array(playerSeatViewSchema).min(1).max(6),
  turn: playerTurnSchema.nullable(),
  visibleObjects: z.array(playerObjectSchema).max(PLAYER_ITEMS),
  zones: z.array(playerZoneSchema).max(22),
  stack: z.array(playerStackEntrySchema).max(PLAYER_ITEMS),
  decisions: z.array(playerDecisionSchema).max(PLAYER_ITEMS),
  throughEventSequence: amountSchema,
});
export type PlayerSnapshot = z.infer<typeof playerSnapshotSchema>;
/** Success payload of POST /v1/games/{game}/view, inside the standard API envelope. */
export const playerViewSchema = z.strictObject({
  snapshot: playerSnapshotSchema,
});
export type PlayerView = z.infer<typeof playerViewSchema>;
export type PlayerObject = z.infer<typeof playerObjectSchema>;
export type PlayerZone = z.infer<typeof playerZoneSchema>;
export type PlayerTurn = z.infer<typeof playerTurnSchema>;
/** Trusted expected inputs only, not a serializable player claim or source proof. */
export type PlayerProjectionFacts = {
  basis: PlayerBasis;
  viewer: string;
  roster: readonly string[];
  throughSequence: string;
  visibleObjects: readonly string[];
  knownIdentities: readonly string[];
  orderedZones: readonly string[];
  authorizedFullDisclosure: boolean;
  creatureTypeMembers: readonly string[];
  abilityBindings: readonly {
    source: string;
    handle: string;
    kind: z.infer<typeof playerAbilityKindSchema>;
  }[];
};
export function playerTargetIn(
  target: PlayerTarget,
  objects: ReadonlySet<string>,
  stack: ReadonlySet<string>,
  roster: readonly string[],
): boolean {
  switch (target.kind) {
    case "object":
      return objects.has(target.handle);
    case "stack":
      return stack.has(target.handle);
    case "seat":
      return roster.includes(target.seatId);
  }
}
function zoneObjects(zone: PlayerZone): readonly string[] {
  return zone.visibility === "hidden" ? [] : zone.objects;
}
function zoneCount(zone: PlayerZone): bigint {
  return (
    BigInt(zoneObjects(zone).length) +
    (zone.visibility === "visible" ? 0n : BigInt(zone.hiddenCount))
  );
}
const personal = (kind: PlayerZone["zoneType"]): boolean =>
  kind === "hand" || kind === "library" || kind === "graveyard";
function sortedHandles(values: readonly string[]): boolean {
  return values.every((value, index) => {
    const previous = values[index - 1];
    return index === 0 || (previous !== undefined && previous < value);
  });
}
function zoneValid(
  z: PlayerZone,
  roster: readonly string[],
  ordered: ReadonlySet<string>,
): boolean {
  if (
    personal(z.zoneType)
      ? z.ownerSeatId === null || !roster.includes(z.ownerSeatId)
      : z.ownerSeatId !== null
  )
    return false;
  const xs = zoneObjects(z);
  const known = z.visibility !== "hidden" && z.orderKnown;
  return known ? ordered.has(z.handle) : sortedHandles(xs);
}
const stepPhase: Record<
  z.infer<typeof playerStepSchema>,
  z.infer<typeof playerPhaseSchema>
> = {
  untap: "beginning",
  upkeep: "beginning",
  draw: "beginning",
  "precombat-main": "precombat-main",
  "beginning-combat": "combat",
  "declare-attackers": "combat",
  "declare-blockers": "combat",
  "combat-damage": "combat",
  "end-combat": "combat",
  "postcombat-main": "postcombat-main",
  end: "ending",
  cleanup: "ending",
};
export function validPlayerTurn(
  turn: PlayerTurn,
  roster: readonly string[],
): boolean {
  return (
    stepPhase[turn.step] === turn.phase &&
    [...turn.activeSeatIds, ...turn.prioritySeatIds].every((x) =>
      roster.includes(x),
    )
  );
}
/** Validate parsed projected records against independently supplied accepted-state facts. */
export function validPlayerSnapshot(
  v: PlayerSnapshot,
  f: PlayerProjectionFacts,
): boolean {
  if (
    !samePlayerBasis(v.basis, f.basis) ||
    v.viewerSeatId !== f.viewer ||
    v.throughEventSequence !== f.throughSequence ||
    f.roster.length < 1 ||
    f.roster.length > 6 ||
    !uniquePlayerValues(f.roster, (x) => x) ||
    !f.roster.every((x) => identifierSchema.safeParse(x).success) ||
    !f.roster.includes(f.viewer) ||
    v.players.length !== f.roster.length ||
    !v.players.every((x, i) => x.seatId === f.roster[i])
  )
    return false;
  if (
    v.zones.length !== 3 * f.roster.length + 4 ||
    !uniquePlayerValues(f.visibleObjects, (x) => x) ||
    !uniquePlayerValues(f.knownIdentities, (x) => x) ||
    !uniquePlayerValues(f.orderedZones, (x) => x)
  )
    return false;
  if (
    f.abilityBindings.length > PLAYER_ITEMS ||
    !uniquePlayerValues(f.abilityBindings, (x) =>
      JSON.stringify([x.source, x.handle]),
    )
  )
    return false;
  const expectedAbilities = new Set(
    f.abilityBindings.map((x) => JSON.stringify([x.source, x.handle, x.kind])),
  );
  const actualAbilities = new Set(
    v.visibleObjects.flatMap((o) =>
      o.abilities.map((x) => JSON.stringify([o.handle, x.handle, x.kind])),
    ),
  );
  if (
    expectedAbilities.size !== actualAbilities.size ||
    ![...expectedAbilities].every((x) => actualAbilities.has(x))
  )
    return false;
  const objects = new Set(v.visibleObjects.map((x) => x.handle)),
    known = new Set(f.knownIdentities),
    ordered = new Set(f.orderedZones),
    stack = new Set(v.stack.map((x) => x.handle)),
    zones = new Map(v.zones.map((x) => [x.handle, x]));
  if (
    objects.size !== v.visibleObjects.length ||
    objects.size !== f.visibleObjects.length ||
    !f.visibleObjects.every((x) => objects.has(x)) ||
    !f.knownIdentities.every((x) => objects.has(x)) ||
    stack.size !== v.stack.length ||
    zones.size !== v.zones.length ||
    !f.orderedZones.every((x) => zones.has(x)) ||
    !sortedHandles(v.visibleObjects.map((x) => x.handle)) ||
    !uniquePlayerValues(v.zones, (x) =>
      JSON.stringify([x.zoneType, x.ownerSeatId]),
    )
  )
    return false;
  for (const seat of f.roster)
    for (const kind of ["library", "hand", "graveyard"])
      if (!v.zones.some((z) => z.zoneType === kind && z.ownerSeatId === seat))
        return false;
  for (const kind of ["battlefield", "stack", "exile", "command"])
    if (!v.zones.some((z) => z.zoneType === kind && z.ownerSeatId === null))
      return false;
  const assigned = new Set<string>();
  for (const zone of v.zones) {
    if (
      !zoneValid(zone, f.roster, ordered) ||
      (f.authorizedFullDisclosure && zone.visibility !== "visible")
    )
      return false;
    for (const handle of zoneObjects(zone)) {
      if (!objects.has(handle) || assigned.has(handle)) return false;
      assigned.add(handle);
    }
  }
  if (assigned.size !== objects.size) return false;
  for (const object of v.visibleObjects) {
    const zone = zones.get(object.zone);
    if (
      !zone ||
      !zoneObjects(zone).includes(object.handle) ||
      (zone.ownerSeatId !== null && zone.ownerSeatId !== object.ownerSeatId) ||
      !f.roster.includes(object.ownerSeatId) ||
      (object.controllerSeatId !== null &&
        !f.roster.includes(object.controllerSeatId)) ||
      (object.publicProperties !== null &&
        !object.publicProperties.creatureTypes.every((x) =>
          f.creatureTypeMembers.includes(x),
        )) ||
      (object.attachedTo !== null &&
        !playerTargetIn(object.attachedTo, objects, stack, f.roster))
    )
      return false;
    if (
      object.identity.kind === "known"
        ? !known.has(object.handle)
        : known.has(object.handle) || f.authorizedFullDisclosure
    )
      return false;
  }
  for (const player of v.players) {
    const counts: readonly [string, string][] = [
      ["hand", player.handCount],
      ["library", player.libraryCount],
    ];
    for (const [kind, count] of counts)
      if (
        !v.zones.some(
          (z) =>
            z.zoneType === kind &&
            z.ownerSeatId === player.seatId &&
            zoneCount(z) === BigInt(count),
        )
      )
        return false;
  }
  if (v.turn !== null && !validPlayerTurn(v.turn, f.roster)) return false;
  if (v.turn !== null) {
    if (
      v.turn.activeSeatIds.length === 0 &&
      (f.roster.length < 3 || !v.players.some((player) => player.eliminated))
    ) return false;
    if (v.turn.priorityDeadlineAt !== undefined && (
      f.roster.length < 3 || v.turn.prioritySeatIds.length !== 1 ||
      v.turn.prioritySeatIds[0] !== v.viewerSeatId || v.decisions.length !== 0
    )) return false;
    if (v.turn.prioritySeatIds.some((seat) =>
      v.players.some((player) => player.seatId === seat && player.eliminated)
    )) return false;
  }
  for (const entry of v.stack)
    if (
      !f.roster.includes(entry.controllerSeatId) ||
      (entry.sourceObject !== null && !objects.has(entry.sourceObject)) ||
      !entry.targets.every((t) => playerTargetIn(t, objects, stack, f.roster))
    )
      return false;
  if (!uniquePlayerValues(v.decisions, (x) => x.decision)) return false;
  return v.decisions.every(
    (d: PlayerDecision) =>
      validPlayerDecision(
        d,
        v.basis,
        f.viewer,
        f.roster,
        f.authorizedFullDisclosure,
      ) && playerDecisionReferences(d, objects, stack, f.roster),
  );
}

/** Observe or answer the reserved game's starting-player choice. A historical
 * draw can require both participants to confirm its previously unrecorded chooser. */
export const openingChoiceRequestSchema = z.strictObject({
  gameId: identifierSchema,
  playFirst: z.boolean().nullish(),
  priorChooserSeatId: identifierSchema.nullish(),
  firstSeatId: identifierSchema.nullish(),
}).refine((choice) => [choice.playFirst, choice.priorChooserSeatId, choice.firstSeatId]
  .filter(value => value != null).length <= 1);
export type OpeningChoiceRequest = z.infer<typeof openingChoiceRequestSchema>;

const openingRollSchema = z.number().int().min(1).max(20);
export const openingChoiceSchema = z.strictObject({
  gameId: identifierSchema,
  chooserSeatId: identifierSchema,
  firstSeatId: identifierSchema.nullable(),
  yourChoice: z.boolean(),
  rolls: z.array(openingRollSchema).min(2).max(6).nullable(),
  seatIds: z.array(identifierSchema).min(3).max(6).optional(),
  rollRounds: z.array(z.array(openingRollSchema).min(2).max(6)).optional(),
}).refine(({ rolls, seatIds, rollRounds, chooserSeatId, firstSeatId }) => {
  if (!seatIds) return !rollRounds && (rolls === null || (rolls.length === 2 && rolls[0] !== rolls[1]));
  if (!rolls || !rollRounds?.length || new Set(seatIds).size !== seatIds.length ||
      !seatIds.includes(chooserSeatId) || (firstSeatId !== null && !seatIds.includes(firstSeatId)) ||
      rolls.length !== seatIds.length || rolls.some((roll, i) => roll !== rollRounds[0]?.[i])) return false;
  let contenders = seatIds;
  for (const round of rollRounds) {
    if (contenders.length < 2 || round.length !== contenders.length) return false;
    const highest = Math.max(...round);
    contenders = contenders.filter((_, i) => round[i] === highest);
  }
  return contenders.length === 1 && contenders[0] === chooserSeatId;
});
export type OpeningChoice = z.infer<typeof openingChoiceSchema>;

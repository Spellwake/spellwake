/** Projected public records; expected facts still require a qualified native projector. */
import { z } from "zod";
import { amountSchema, identifierSchema } from "./primitives.ts";
import {
  playerBasisSchema,
  playerHandleSchema,
  playerDisplayTextSchema,
  playerCardTypeSchema,
  playerEnglishLabelSchema,
  rulesCountSchema,
  rulesIntegerSchema,
  type PlayerBasis,
} from "./player-primitives.ts";
import {
  PLAYER_ITEMS,
  playerManaColorSchema,
  playerManaSpendSchema,
  playerRestrictedManaSpendSchema,
  playerTargetSchema,
  playerObjectTargetSchema,
  playerSeatTargetSchema,
  uniquePlayerValues,
  type PlayerTarget,
} from "./player-actions.ts";
import {
  playerDecisionSchema,
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
export { playerCardTypeSchema } from "./player-primitives.ts";
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
  "cycling",
  "ninjutsu",
  "transmute",
  "crew",
  "saddle",
  "tap-chosen-permanent",
  "station",
  "activated-untargeted",
  "triggered",
  "continuous",
]);
export const playerAbilityViewSchema = z.strictObject({
  powerUpCost: z.strictObject({
    generic: rulesCountSchema,
    required: z.array(playerManaSpendSchema).max(6).refine((rows) =>
      uniquePlayerValues(rows, (row) => row.color) && rows.every((row) => BigInt(row.amount) > 0n)),
  }).optional(),
  exhausted: z.boolean().optional(),
  handle: playerHandleSchema,
  kind: playerAbilityKindSchema,
  label: playerDisplayTextSchema(1024).refine((v) => v.length > 0),
});
export const playerKeywordSchema = z.enum([
  "haste",
  "partner",
  "choose-a-background",
  "doctors-companion",
  "bargain",
  "banding",
  "bands-with-other-legendary-creatures",
  "bands-with-other-creatures-named-wolves-of-the-hunt",
  "vigilance",
  "flying",
  "reach",
  "daybound",
  "nightbound",
  "phasing",
  "defender",
  "menace",
  "shadow",
  "horsemanship",
  "fear",
  "intimidate",
  "skulk",
  "plainswalk",
  "islandwalk",
  "swampwalk",
  "mountainwalk",
  "forestwalk",
  "desertwalk",
  "nonbasic-landwalk",
  "legendary-landwalk",
  "snow-landwalk",
  "snow-forestwalk",
  "snow-swampwalk",
  "artifact-landwalk",
  "trample",
  "deathtouch",
  "convoke",
  "improvise",
  "protection-from-white",
  "protection-from-blue",
  "protection-from-black",
  "protection-from-red",
  "protection-from-green",
  "protection-from-multicolored",
  "retrace",
  "jump-start",
  "rebound",
  "delve",
  "lifelink",
  "indestructible",
  "first-strike",
  "double-strike",
  "shroud",
  "hexproof",
  "flash",
  "split-second",
  "changeling",
  "devoid",
  "start-your-engines",
  "exalted",
  "wither",
  "infect",
  "poisonous",
  "toxic",
  "echo",
  "cumulative-upkeep",
  "fading",
  "vanishing",
  "mentor",
  "evolve",
  "backup",
  "extort",
  "increment",
  "decayed",
  "mobilize",
  "power-up",
  "ingest",
  "annihilator",
  "modular",
  "soulshift",
  "graft",
  "persist",
  "undying",
  "afterlife",
  "fabricate",
  "afflict",
  "dethrone",
  "melee",
  "renown",
  "training",
  "prowess",
  "battle-cry",
  "bushido",
  "flanking",
  "frenzy",
  "rampage",
]);
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
  printedDefense: playerDisplayTextSchema(128).optional(),
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
    .max(8)
    .refine((xs) => uniquePlayerValues(xs, (x) => x)),
  supertypes: z
    .array(playerSupertypeSchema)
    .max(5)
    .refine((xs) => uniquePlayerValues(xs, (x) => x)),
  landTypes: z
    .array(playerLandTypeSchema)
    .max(17)
    .refine((xs) => uniquePlayerValues(xs, (x) => x)),
  spellTypes: z.array(z.enum(["Arcane","Adventure","Lesson","Trap","Omen"])).max(5).refine(xs=>new Set(xs).size===xs.length).optional(),
  enchantmentTypes: z.array(z.literal("Saga")).max(1).optional(),
  planeswalkerTypes: z.array(z.enum(["Ajani", "Calix", "Chandra", "Daretti", "Elspeth", "Freyalise", "Gideon", "Guff", "Jace", "Nahiri", "Narset", "Nixilis", "Teferi", "Teyo", "Ugin", "Vronos"]))
    .max(16).refine((xs) => uniquePlayerValues(xs, (x) => x)).optional(),
  creatureTypes: z
    .array(playerEnglishLabelSchema)
    .max(PLAYER_ITEMS)
    .refine((xs) => uniquePlayerValues(xs, (x) => x)),
  chosenCreatureType: playerEnglishLabelSchema.optional(),
  colors: z
    .array(playerManaColorSchema)
    .max(5)
    .refine(
      (xs) => uniquePlayerValues(xs, (x) => x) && !xs.includes("colourless"),
    ),
  renowned: z.boolean().optional(),
  keywords: z
    .array(playerKeywordSchema)
    .max(playerKeywordSchema.options.length)
    .refine((xs) => uniquePlayerValues(xs, (x) => x)),
  power: rulesIntegerSchema.nullable(),
  toughness: rulesIntegerSchema.nullable(),
}).refine((value) => (!value.spellTypes?.length || value.cardTypes.includes("instant") || value.cardTypes.includes("sorcery")) && (!value.planeswalkerTypes?.length || value.cardTypes.includes("planeswalker")) && (!value.enchantmentTypes?.length || value.cardTypes.includes("enchantment")));
export const playerVisibleSubjectSchema = z.union([
  playerObjectTargetSchema,
  playerSeatTargetSchema,
]);
export const playerObjectSchema = z.strictObject({
  commander: z.strictObject({
    commandZoneCasts: rulesCountSchema,
    combatDamage: z.array(z.strictObject({seatId:identifierSchema, amount:rulesCountSchema.refine((v)=>BigInt(v)>0n)})).max(PLAYER_ITEMS).refine((rows)=>uniquePlayerValues(rows,row=>row.seatId)),
  }).optional(),
  handle: playerHandleSchema,
  zone: playerHandleSchema,
  ownerSeatId: identifierSchema,
  controllerSeatId: identifierSchema.nullable(),
  protectorSeatId: identifierSchema.optional(),
  timesMutated: rulesCountSchema.refine((count) => BigInt(count) > 0n).optional(),
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
  saddled: z.boolean().optional(),
  bestowed: z.boolean().optional(),
  plottedOnTurn: rulesCountSchema.optional(),
  mayPlayFromExile: z.boolean().optional(),
  mayCastFreeFromExile:z.boolean().optional(),
  freeExileTypes:z.array(playerCardTypeSchema.exclude(["land"])).max(8).refine(types=>new Set(types).size===types.length).optional(),
  mayPlayFromLibrary: z.boolean().optional(),
  exileMana: z.enum(["any-color","any-type"]).optional(),
  warpedOnTurn: rulesCountSchema.optional(),
  adventureFace: playerCharacteristicsSchema.optional(),
  splitFaces:z.tuple([playerCharacteristicsSchema,playerCharacteristicsSchema]).optional(),
  transformFace: z.strictObject({backFaceUp:z.boolean(), opposite:playerCharacteristicsSchema}).optional(),
  faceDown: z.enum(["morph","disguise","manifest","forest"]).optional(),
  exiledFaceDown: z.boolean().optional(),
  faceDownOrder: rulesCountSchema.refine((n) => BigInt(n) > 0n).optional(),
  faceUpOptions: z.array(z.strictObject({
    ability:z.number().int().min(0).max(65535), generic:rulesCountSchema,
    required:z.array(playerManaSpendSchema).max(6).refine((cost) => uniquePlayerValues(cost, (item) => item.color) && cost.every((item) => BigInt(item.amount) > 0n)),
  })).max(PLAYER_ITEMS).refine((offers) => uniquePlayerValues(offers, (item) => String(item.ability))).optional(),
  foretoldOnTurn: rulesCountSchema.optional(),
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
  phasedOut: z.array(z.strictObject({names:z.array(playerDisplayTextSchema(256).refine(name=>name.length>0)).max(PLAYER_ITEMS),
    faceDown:z.boolean(),tapped:z.boolean(),phaseInSeatId:identifierSchema,loyalty:rulesCountSchema.nullable()}).refine(p=>!p.faceDown || p.names.length===0)).max(PLAYER_ITEMS).optional(),
  emblems: z.array(z.discriminatedUnion("kind", [z.strictObject({ kind: z.literal("prevent-all-but-one") }), z.strictObject({ kind: z.literal("loyalty-any-turn") }), z.strictObject({ kind: z.literal("return-fallen-artifacts") }), z.strictObject({ kind: z.literal("sacrifice-creature-gain-life-draw") }), z.strictObject({
    kind: z.literal("upkeep-damage"),
    amount: rulesIntegerSchema.refine((value) => BigInt(value) > 0n),
  }), z.strictObject({
    kind: z.literal("spellcast-damage"),
    amount: rulesIntegerSchema.refine((value) => BigInt(value) > 0n),
    noncreature_only: z.boolean(),
  }), z.strictObject({
    kind: z.literal("controlled-creature-bonus"),
    power: rulesIntegerSchema,
    toughness: rulesIntegerSchema,
    keywords: z.array(playerKeywordSchema).max(playerKeywordSchema.options.length).refine((xs) => uniquePlayerValues(xs, (x) => x)),
  })])).max(PLAYER_ITEMS).optional(),
  monarch: z.boolean().optional(),
  cityBlessing: z.boolean().optional(),
  speed:z.number().int().min(0).max(4).optional(),
  searchReveal: playerCharacteristicsSchema.optional(),
  searchReveals: z.array(playerCharacteristicsSchema).max(PLAYER_ITEMS).optional(),
  cascadeReveals:z.array(playerCharacteristicsSchema).optional(),
  faceDownReveals: z.array(playerCharacteristicsSchema).max(PLAYER_ITEMS).optional(),
  seatId: identifierSchema,
  life: rulesIntegerSchema,
  poison: rulesCountSchema,
  counters,
  manaPool: z
    .array(playerManaSpendSchema)
    .max(6)
    .refine((xs) => uniquePlayerValues(xs, (x) => x.color)),
  restrictedMana: z.array(playerRestrictedManaSpendSchema).max(PLAYER_ITEMS).optional(),
  handCount: rulesCountSchema,
  libraryCount: rulesCountSchema,
  eliminated: z.boolean(),
}).refine((seat) => uniquePlayerValues(seat.restrictedMana ?? [], (item) => `${item.color}:${item.restriction}:${item.creatureType ?? ""}`)
  && (seat.restrictedMana ?? []).every((item) => BigInt(item.amount) <= BigInt(seat.manaPool.find((pool) => pool.color === item.color)?.amount ?? "0"))
  && seat.manaPool.every((pool) => (seat.restrictedMana ?? []).filter((item) => item.color === pool.color).reduce((sum, item) => sum + BigInt(item.amount), 0n) <= BigInt(pool.amount)));
export const playerTurnSchema = z.strictObject({
  dayNight: z.enum(["day","night"]).optional(),
  number: rulesCountSchema.refine((v) => BigInt(v) > 0n),
  activeSeatIds: z
    .array(identifierSchema)
    .max(6)
    .refine((xs) => uniquePlayerValues(xs, (x) => x)),
  prioritySeatIds: z
    .array(identifierSchema)
    .max(6)
    .refine((xs) => uniquePlayerValues(xs, (x) => x)),
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
  chosenX: rulesCountSchema.optional(),
  modes: z.array(z.number().int().min(0).max(255)).max(255).optional(),
  visibleContributors: z.array(playerHandleSchema).max(PLAYER_ITEMS).optional(),
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
    v.players.filter((player) => player.monarch).length > 1 ||
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
      (object.commander !== undefined && !object.commander.combatDamage.every((hit)=>f.roster.includes(hit.seatId))) ||
      (object.protectorSeatId !== undefined && (!f.roster.includes(object.protectorSeatId) || zone.zoneType !== "battlefield" || !object.publicProperties?.cardTypes.includes("battle"))) ||
      (object.controllerSeatId !== null &&
        !f.roster.includes(object.controllerSeatId)) ||
      ((object.freeExileTypes?.length ?? 0) > 0 && object.mayCastFreeFromExile !== true) ||
      ((object.mayPlayFromExile === true || object.mayCastFreeFromExile === true || object.exileMana !== undefined) && zone.zoneType !== "exile") ||
      (object.mayPlayFromLibrary === true && (zone.zoneType !== "library" || zone.visibility === "hidden" || zone.objects[0] !== object.handle)) ||
      (object.warpedOnTurn !== undefined && zone.zoneType !== "exile") ||
      (object.exiledFaceDown === true && zone.zoneType !== "exile") ||
      (object.bestowed && zone.zoneType !== "battlefield" && zone.zoneType !== "stack") ||
      (object.splitFaces !== undefined && !known.has(object.handle)) ||
      (object.adventureFace !== undefined && !known.has(object.handle)) ||
      (object.transformFace !== undefined && (!known.has(object.handle) || (object.transformFace.backFaceUp && zone.zoneType !== "battlefield" && zone.zoneType !== "stack"))) ||
      ((object.faceDownOrder !== undefined) !== (object.faceDown !== undefined || object.foretoldOnTurn !== undefined || object.exiledFaceDown === true)) ||
      (Boolean(object.faceUpOptions?.length) && (object.faceDown === undefined || object.controllerSeatId !== v.viewerSeatId || !known.has(object.handle) || zone.zoneType !== "battlefield")) ||
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
  for (const entry of v.stack)
    if (
      !f.roster.includes(entry.controllerSeatId) ||
      (entry.visibleContributors?.some((v) => !objects.has(v)) ?? false) ||
      new Set(entry.visibleContributors).size !== (entry.visibleContributors?.length ?? 0) ||
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

import { describe, expect, test } from "bun:test";
import {
  playerCastOfferSchema,
  playerSnapshotSchema,
  validPlayerSnapshot,
  type PlayerProjectionFacts,
  type PlayerSnapshot,
} from "./player-views.ts";

const GAME = "11111111-1111-4111-8111-111111111111";
const CONTEXT = "22222222-2222-4222-8222-222222222222";
const SEAT_A = "33333333-3333-4333-8333-333333333333";
const SEAT_B = "44444444-4444-4444-8444-444444444444";

function baseSnapshot(
  castOffers?: PlayerSnapshot["castOffers"],
): { snapshot: PlayerSnapshot; facts: PlayerProjectionFacts } {
  const basis = {
    gameId: GAME,
    contextId: CONTEXT,
    epoch: "0",
    revision: "1",
  };
  const snapshot: PlayerSnapshot = {
    basis,
    viewerSeatId: SEAT_A,
    players: [
      {
        seatId: SEAT_A,
        life: "20",
        poison: "0",
        counters: [],
        manaPool: [],
        handCount: "1",
        libraryCount: "0",
        eliminated: false,
      },
      {
        seatId: SEAT_B,
        life: "20",
        poison: "0",
        counters: [],
        manaPool: [],
        handCount: "0",
        libraryCount: "0",
        eliminated: false,
      },
    ],
    turn: {
      number: "1",
      activeSeatIds: [SEAT_A],
      prioritySeatIds: [SEAT_A],
      phase: "precombat-main",
      step: "precombat-main",
    },
    visibleObjects: [
      {
        handle: "obj-bolt",
        zone: "zone-hand-a",
        ownerSeatId: SEAT_A,
        controllerSeatId: null,
        identity: {
          kind: "known",
          definitionId: null,
          printingId: null,
          characteristics: {
            name: "Lightning Bolt",
            typeLine: "Instant",
            rulesText: "Lightning Bolt deals 3 damage to any target.",
            manaCost: "{R}",
            printedPower: null,
            printedToughness: null,
            printedLoyalty: null,
          },
          currentNames: ["Lightning Bolt"],
        },
        publicProperties: {
          cardTypes: ["instant"],
          supertypes: [],
          landTypes: [],
          creatureTypes: [],
          colors: ["red"],
          keywords: [],
          power: null,
          toughness: null,
        },
        tapped: false,
        counters: [],
        abilities: [],
        damageMarked: "0",
        attachedTo: null,
      },
      {
        handle: "obj-mountain",
        zone: "zone-bf",
        ownerSeatId: SEAT_A,
        controllerSeatId: SEAT_A,
        identity: {
          kind: "known",
          definitionId: null,
          printingId: null,
          characteristics: {
            name: "Mountain",
            typeLine: "Basic Land — Mountain",
            rulesText: "{T}: Add {R}.",
            manaCost: null,
            printedPower: null,
            printedToughness: null,
            printedLoyalty: null,
          },
          currentNames: ["Mountain"],
        },
        publicProperties: {
          cardTypes: ["land"],
          supertypes: ["basic"],
          landTypes: ["Mountain"],
          creatureTypes: [],
          colors: [],
          keywords: [],
          power: null,
          toughness: null,
        },
        tapped: false,
        counters: [],
        abilities: [
          { handle: "ab-red", kind: "mana", label: "Add {R}" },
        ],
        damageMarked: "0",
        attachedTo: null,
      },
    ],
    zones: [
      {
        handle: "zone-lib-a",
        zoneType: "library",
        ownerSeatId: SEAT_A,
        visibility: "hidden",
        hiddenCount: "0",
      },
      {
        handle: "zone-hand-a",
        zoneType: "hand",
        ownerSeatId: SEAT_A,
        visibility: "visible",
        objects: ["obj-bolt"],
        orderKnown: false,
      },
      {
        handle: "zone-gy-a",
        zoneType: "graveyard",
        ownerSeatId: SEAT_A,
        visibility: "visible",
        objects: [],
        orderKnown: true,
      },
      {
        handle: "zone-lib-b",
        zoneType: "library",
        ownerSeatId: SEAT_B,
        visibility: "hidden",
        hiddenCount: "0",
      },
      {
        handle: "zone-hand-b",
        zoneType: "hand",
        ownerSeatId: SEAT_B,
        visibility: "hidden",
        hiddenCount: "0",
      },
      {
        handle: "zone-gy-b",
        zoneType: "graveyard",
        ownerSeatId: SEAT_B,
        visibility: "visible",
        objects: [],
        orderKnown: true,
      },
      {
        handle: "zone-bf",
        zoneType: "battlefield",
        ownerSeatId: null,
        visibility: "visible",
        objects: ["obj-mountain"],
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
        handle: "zone-cmd",
        zoneType: "command",
        ownerSeatId: null,
        visibility: "visible",
        objects: [],
        orderKnown: false,
      },
    ],
    stack: [],
    decisions: [],
    ...(castOffers !== undefined ? { castOffers } : {}),
    throughEventSequence: "1",
  };
  const facts: PlayerProjectionFacts = {
    basis,
    viewer: SEAT_A,
    roster: [SEAT_A, SEAT_B],
    throughSequence: "1",
    visibleObjects: ["obj-bolt", "obj-mountain"],
    knownIdentities: ["obj-bolt", "obj-mountain"],
    orderedZones: ["zone-gy-a", "zone-gy-b", "zone-stack"],
    authorizedFullDisclosure: false,
    creatureTypeMembers: [],
    abilityBindings: [
      { source: "obj-mountain", handle: "ab-red", kind: "mana" },
    ],
  };
  return { snapshot, facts };
}

describe("playerCastOfferSchema and PlayerSnapshot.castOffers", () => {
  test("parses and validates server-authored cast offers with targets, paySources, and payment", () => {
    const offer = playerCastOfferSchema.parse({
      source: "obj-bolt",
      targets: [{ kind: "seat", seatId: SEAT_B }],
      cost: "{R}",
      paySources: ["obj-mountain"],
      payment: { spend: [{ color: "red", amount: "1" }] },
    });
    const { snapshot, facts } = baseSnapshot([offer]);
    const parsed = playerSnapshotSchema.parse(snapshot);
    expect(parsed.castOffers).toHaveLength(1);
    expect(validPlayerSnapshot(parsed, facts)).toBe(true);
  });

  test("rejects cast offers referencing unknown objects, targets, or duplicate sources", () => {
    const unknownSource = baseSnapshot([
      { source: "obj-missing", cost: "{R}" },
    ]);
    expect(validPlayerSnapshot(unknownSource.snapshot, unknownSource.facts)).toBe(
      false,
    );

    const unknownPaySource = baseSnapshot([
      { source: "obj-bolt", cost: "{R}", paySources: ["obj-missing"] },
    ]);
    expect(
      validPlayerSnapshot(unknownPaySource.snapshot, unknownPaySource.facts),
    ).toBe(false);

    const unknownTarget = baseSnapshot([
      {
        source: "obj-bolt",
        cost: "{R}",
        targets: [{ kind: "object", handle: "obj-missing" }],
      },
    ]);
    expect(validPlayerSnapshot(unknownTarget.snapshot, unknownTarget.facts)).toBe(
      false,
    );

    const duplicate = baseSnapshot([
      { source: "obj-bolt", cost: "{R}" },
      { source: "obj-bolt", cost: "{R}" },
    ]);
    expect(validPlayerSnapshot(duplicate.snapshot, duplicate.facts)).toBe(false);
  });
});

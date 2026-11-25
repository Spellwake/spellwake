import { describe, expect, test } from "bun:test";
import {
  customCardCompileInputSchema,
  customCardFileV1Schema,
  deckFaceCardSchema,
  deckSettingsChangeSchema,
  deckSummarySchema,
  ENCHANTMENT_SUBTYPES,
  PLANESWALKER_SUBTYPES,
  playerObjectIdentitySchema,
  playerPublicPropertiesSchema,
  playerSeatViewSchema,
  roomDetailSchema,
  roomSummarySchema,
  saveDeckSchema,
  setAiDeckRequestSchema,
  updateRoomOptionsSchema,
  validRoomDetail,
  visionImportRequestSchema,
} from "./index.ts";

const U1 = "11111111-1111-4111-8111-111111111111";
const U2 = "22222222-2222-4222-8222-222222222222";
const U3 = "33333333-3333-4333-8333-333333333333";
const U4 = "44444444-4444-4444-8444-444444444444";

describe("custom card wire contracts", () => {
  test("validates single-face and two-face layout counts", () => {
    const validNormal = customCardCompileInputSchema.safeParse({
      layout: "normal",
      rarity: "rare",
      faces: [
        {
          name: "Aetherblade Vanguard",
          manaCost: "{2}{U}{R}",
          typeLine: "Creature — Human Artificer",
          oracleText: "Flying, haste\nWhen Aetherblade Vanguard enters, draw a card.",
          power: "3",
          toughness: "2",
          loyalty: null,
          defense: null,
        },
      ],
    });
    expect(validNormal.success).toBe(true);

    const invalidTransformSingleFace = customCardCompileInputSchema.safeParse({
      layout: "transform",
      rarity: "mythic",
      faces: [
        {
          name: "Solo Face",
          manaCost: "{1}{B}",
          typeLine: "Creature — Vampire",
          oracleText: "Flying",
          power: "2",
          toughness: "2",
          loyalty: null,
          defense: null,
        },
      ],
    });
    expect(invalidTransformSingleFace.success).toBe(false);
  });

  test("validates .swcard.json v1 schema and rejects SVG data URLs", () => {
    const ok = customCardFileV1Schema.safeParse({
      schema: "spellwake-custom-card-v1",
      layout: "normal",
      rarity: "rare",
      collectorNote: "Set 001",
      faces: [
        {
          name: "Aetherblade Vanguard",
          manaCost: "{2}{U}{R}",
          typeLine: "Creature — Human Artificer",
          oracleText: "Flying",
          power: "3",
          toughness: "2",
          loyalty: null,
          defense: null,
          flavorText: "Forged in stormlight.",
          frameAccent: "auto",
          art: {
            dataUrl: "data:image/webp;base64,UklGRh4AAABXRUJQVlA4TBEAAAAvAAAAAAfQ//73v/+BiOh/AAA=",
            cropX: 0,
            cropY: -0.1,
            zoom: 1.25,
            artist: "Aaron",
          },
        },
      ],
    });
    expect(ok.success).toBe(true);

    const svgRejected = customCardFileV1Schema.safeParse({
      schema: "spellwake-custom-card-v1",
      layout: "normal",
      rarity: "common",
      collectorNote: null,
      faces: [
        {
          name: "Bad Art",
          manaCost: "{R}",
          typeLine: "Instant",
          oracleText: "Bad Art deals 3 damage to any target.",
          power: null,
          toughness: null,
          loyalty: null,
          defense: null,
          flavorText: null,
          frameAccent: "R",
          art: {
            dataUrl: "data:image/svg+xml;base64,PHN2Zz48L3N2Zz4=",
            cropX: 0,
            cropY: 0,
            zoom: 1,
            artist: null,
          },
        },
      ],
    });
    expect(svgRejected.success).toBe(false);
  });

  test("supports discriminated and transitional legacy DeckFaceCard", () => {
    const legacy = deckFaceCardSchema.parse({
      definitionId: U1,
      artUrl: "https://cards.scryfall.io/art_crop/front/a/b/test.jpg",
    });
    expect(legacy).toEqual({
      kind: "official",
      definitionId: U1,
      artUrl: "https://cards.scryfall.io/art_crop/front/a/b/test.jpg",
    });

    const custom = deckFaceCardSchema.parse({
      kind: "custom",
      customCardRevisionId: U2,
      artPath: `/v1/custom-cards/revisions/${U2}/art/0`,
    });
    expect(custom.kind).toBe("custom");
  });

  test("enforces faceCardId vs faceCustomCardRevisionId mutual exclusivity and combined deck cap", () => {
    const bothFaces = deckSettingsChangeSchema.safeParse({
      name: "Open Deck",
      mode: "open",
      formatId: U1,
      appearance: { backId: U1, sleeveId: U2, diceId: U3, playmatId: null },
      faceCardId: U1,
      faceCustomCardRevisionId: U2,
    });
    expect(bothFaces.success).toBe(false);

    const onlyCustomFace = deckSettingsChangeSchema.safeParse({
      name: "Open Deck",
      mode: "open",
      formatId: U1,
      appearance: { backId: U1, sleeveId: U2, diceId: U3, playmatId: null },
      faceCustomCardRevisionId: U2,
    });
    expect(onlyCustomFace.success).toBe(true);

    const saveOk = saveDeckSchema.safeParse({
      requestId: U1,
      deckId: U2,
      expectedRevisionId: null,
      entries: [],
      customEntries: [
        { customCardRevisionId: U3, section: "main", quantity: "4" },
      ],
    });
    expect(saveOk.success).toBe(true);

    const summaryLegacy = deckSummarySchema.parse({
      id: U1,
      name: "Legacy Deck",
      mode: "open",
      currentRevisionId: null,
      format: { id: U2, key: "modern", name: "Modern" },
      appearance: { backId: U1, sleeveId: U2, diceId: U3, playmatId: null },
      faceCard: null,
    });
    expect(summaryLegacy.hasCustomCards).toBe(false);
  });

  test("enforces Open Play room constraints and validRoomDetail AI deck visibility for host", () => {
    const competitiveCustomRejected = roomSummarySchema.safeParse({
      id: U1,
      hostId: U2,
      formatId: U3,
      profileId: U4,
      mode: "collection",
      competitive: true,
      visibility: "private",
      state: "lobby",
      revision: "1",
      structure: "bo1",
      configurationId: U4,
      targetSeatCount: "2",
      countdownAt: null,
      allowCustomCards: true,
      shareHostDecks: false,
    });
    expect(competitiveCustomRejected.success).toBe(false);

    const openDetail = roomDetailSchema.parse({
      accountId: U2,
      room: {
        id: U1,
        hostId: U2,
        formatId: U3,
        profileId: U4,
        mode: "open",
        competitive: false,
        visibility: "private",
        state: "lobby",
        revision: "1",
        structure: "bo1",
        configurationId: U4,
        targetSeatCount: "2",
        countdownAt: null,
        allowCustomCards: true,
        shareHostDecks: true,
      },
      slots: {
        items: [
          {
            id: U3,
            accountId: U2,
            seatIndex: "0",
            controllerKind: "human",
            ready: false,
            deckRevisionId: U4,
            deckName: "My Custom Deck",
          },
          {
            id: U4,
            accountId: null,
            seatIndex: "1",
            controllerKind: "ai",
            ready: false,
            deckRevisionId: U4,
            deckName: "Host Shared Deck",
          },
        ],
        nextCursor: null,
      },
    });
    expect(validRoomDetail(openDetail, U2, U1, null, 20)).toBe(true);
    expect(
      setAiDeckRequestSchema.safeParse({
        requestId: U1,
        expectedRevision: "1",
        deckRevisionId: U4,
      }).success,
    ).toBe(true);
    expect(
      updateRoomOptionsSchema.safeParse({
        allowCustomCards: true,
        shareHostDecks: true,
      }).success,
    ).toBe(true);
  });

  test("syncs player views subtypes, emblems, manaLots, customCardRevisionId, and Vision PNG/image import", () => {
    expect(ENCHANTMENT_SUBTYPES.length).toBe(11);
    expect(PLANESWALKER_SUBTYPES.length).toBe(87);

    const known = playerObjectIdentitySchema.safeParse({
      kind: "known",
      definitionId: null,
      printingId: null,
      customCardRevisionId: U1,
      characteristics: {
        name: "Aetherblade Vanguard",
        typeLine: "Creature — Human Artificer",
        rulesText: "Flying",
        manaCost: "{2}{U}{R}",
        printedPower: "3",
        printedToughness: "2",
        printedLoyalty: null,
      },
      currentNames: ["Aetherblade Vanguard"],
    });
    expect(known.success).toBe(true);

    const props = playerPublicPropertiesSchema.safeParse({
      cardTypes: ["enchantment", "planeswalker"],
      supertypes: ["legendary"],
      landTypes: [],
      enchantmentTypes: ["Class", "Room", "Saga"],
      planeswalkerTypes: ["Kaito", "Oko"],
      creatureTypes: [],
      colors: ["blue"],
      keywords: [],
      power: null,
      toughness: null,
    });
    expect(props.success).toBe(true);

    const seat = playerSeatViewSchema.safeParse({
      seatId: U1,
      life: "20",
      poison: "0",
      counters: [],
      manaPool: [],
      manaLots: [
        {
          id: 1,
          color: "blue",
          effect: "retain-until-cleanup",
          amount: "1",
        },
      ],
      emblems: [
        { kind: "unlimited-hand-with-graveyard-return" },
        { kind: "authored", label: "Creatures you control get +1/+1." },
      ],
      handCount: "7",
      libraryCount: "53",
      eliminated: false,
    });
    expect(seat.success).toBe(true);

    expect(
      visionImportRequestSchema.safeParse({
        sourceKind: "external_png",
        imageDataUrl: "data:image/png;base64,iVBORw0KGgo=",
      }).success,
    ).toBe(true);
  });
});

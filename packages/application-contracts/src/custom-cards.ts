/** Shared Zod contracts for Spellwake Custom Card Studio, PNG/JSON import/export, and Vision OCR. */
import { z } from "zod";
import {
  amountSchema,
  identifierSchema,
  instantSchema,
  positiveAmountSchema,
} from "./primitives.ts";

export const CUSTOM_CARD_LAYOUTS = [
  "normal",
  "transform",
  "modal_dfc",
  "adventure",
  "split",
  "planeswalker",
  "saga",
] as const;
export const customCardLayoutSchema = z.enum(CUSTOM_CARD_LAYOUTS);
export type CustomCardLayout = z.infer<typeof customCardLayoutSchema>;

export const TWO_FACE_CUSTOM_CARD_LAYOUTS: ReadonlySet<CustomCardLayout> =
  new Set(["transform", "modal_dfc", "adventure", "split"]);

export const CUSTOM_CARD_RARITIES = [
  "common",
  "uncommon",
  "rare",
  "mythic",
] as const;
export const customCardRaritySchema = z.enum(CUSTOM_CARD_RARITIES);
export type CustomCardRarity = z.infer<typeof customCardRaritySchema>;

export const CUSTOM_CARD_FRAME_ACCENTS = [
  "auto",
  "W",
  "U",
  "B",
  "R",
  "G",
  "gold",
  "artifact",
  "colorless",
  "land",
] as const;
export const customCardFrameAccentSchema = z.enum(CUSTOM_CARD_FRAME_ACCENTS);
export type CustomCardFrameAccent = z.infer<typeof customCardFrameAccentSchema>;

export const CUSTOM_CARD_COLORS = ["W", "U", "B", "R", "G"] as const;
export const customCardColorSchema = z.enum(CUSTOM_CARD_COLORS);
export type CustomCardColor = z.infer<typeof customCardColorSchema>;

export const ENCHANTMENT_SUBTYPES = [
  "Background",
  "Cartouche",
  "Case",
  "Class",
  "Curse",
  "Role",
  "Room",
  "Rune",
  "Saga",
  "Shard",
  "Shrine",
] as const;

export const SPELL_SUBTYPES = [
  "Adventure",
  "Arcane",
  "Lesson",
  "Trap",
  "Omen",
] as const;

export const PLANESWALKER_SUBTYPES = [
  "Ajani",
  "Aminatou",
  "Angrath",
  "Arlinn",
  "Arzakon",
  "Ashiok",
  "Bahamut",
  "Basri",
  "Bolas",
  "Calix",
  "Chandra",
  "Comet",
  "Dack",
  "Dakkon",
  "Daretti",
  "Davriel",
  "Dellian",
  "Dihada",
  "Domri",
  "Dovin",
  "Dyfed",
  "Ellywick",
  "Elminster",
  "Elspeth",
  "Estrid",
  "Feroz",
  "Freyalise",
  "Garruk",
  "Gideon",
  "Greensleeves",
  "Grist",
  "Guff",
  "Huatli",
  "Jace",
  "Jared",
  "Jaya",
  "Jeska",
  "Kaito",
  "Karn",
  "Kasmina",
  "Kaya",
  "Kiora",
  "Koth",
  "Liliana",
  "Lolth",
  "Lukka",
  "Minsc",
  "Mordenkainen",
  "Nahiri",
  "Narset",
  "Niko",
  "Nissa",
  "Nixilis",
  "Oko",
  "Quintorius",
  "Ral",
  "Rowan",
  "Saheeli",
  "Samut",
  "Sarkhan",
  "Serra",
  "Sifa",
  "Sivitri",
  "Sorin",
  "Szat",
  "Tamiyo",
  "Tasha",
  "Teferi",
  "Teyo",
  "Tezzeret",
  "Thomil",
  "Tibalt",
  "Tyvar",
  "Ugin",
  "Urza",
  "Venser",
  "Vivien",
  "Vraska",
  "Vronos",
  "Will",
  "Windgrace",
  "Worzel",
  "Wrenn",
  "Xenagos",
  "Yanggu",
  "Yanling",
  "Zariel",
] as const;

const finiteRange = (min: number, max: number) =>
  z.number().refine((n) => Number.isFinite(n) && n >= min && n <= max);

const safeDisplayText = (maxLen: number) =>
  z
    .string()
    .max(maxLen)
    .refine((value) => !value.includes("\0") && !/[\uD800-\uDFFF]/u.test(value));

export const customCardFaceRulesSchema = z.strictObject({
  name: safeDisplayText(120).refine((v) => v.trim().length >= 1),
  manaCost: safeDisplayText(128).nullable(),
  typeLine: safeDisplayText(256).refine((v) => v.trim().length >= 1),
  oracleText: safeDisplayText(4096),
  power: safeDisplayText(16).nullable(),
  toughness: safeDisplayText(16).nullable(),
  loyalty: safeDisplayText(16).nullable(),
  defense: safeDisplayText(16).nullable(),
  colorIndicator: z
    .array(customCardColorSchema)
    .max(5)
    .refine((xs) => new Set(xs).size === xs.length)
    .nullable()
    .optional(),
});
export type CustomCardFaceRules = z.infer<typeof customCardFaceRulesSchema>;

function matchesLayoutFaceCount(
  layout: CustomCardLayout,
  faceCount: number,
): boolean {
  return TWO_FACE_CUSTOM_CARD_LAYOUTS.has(layout)
    ? faceCount === 2
    : faceCount === 1;
}

/** Slim rules-only payload sent to /v1/custom-cards/compile and /standardize (<= 16 KiB). */
export const customCardCompileInputSchema = z
  .strictObject({
    layout: customCardLayoutSchema,
    rarity: customCardRaritySchema,
    faces: z.array(customCardFaceRulesSchema).min(1).max(2),
  })
  .refine(
    (card) => matchesLayoutFaceCount(card.layout, card.faces.length),
    "Face count must match layout.",
  );
export type CustomCardCompileInput = z.infer<
  typeof customCardCompileInputSchema
>;

export const customCardArtDataUrlSchema = z
  .string()
  .max(180_000)
  .regex(/^data:image\/(webp|png);base64,[A-Za-z0-9+/=]+$/);

export const customCardFileFaceArtSchema = z.strictObject({
  dataUrl: customCardArtDataUrlSchema,
  cropX: finiteRange(-1, 1),
  cropY: finiteRange(-1, 1),
  zoom: finiteRange(0.5, 4),
  artist: safeDisplayText(80).nullable(),
});
export type CustomCardFileFaceArt = z.infer<typeof customCardFileFaceArtSchema>;

export const customCardFileFaceSchema = customCardFaceRulesSchema.extend({
  flavorText: safeDisplayText(512).nullable(),
  frameAccent: customCardFrameAccentSchema.default("auto"),
  art: customCardFileFaceArtSchema.nullable(),
});
export type CustomCardFileFace = z.infer<typeof customCardFileFaceSchema>;

/** Canonical `.swcard.json` and PNG `iTXt` (`spellwake-card-v1`) card export/import schema. */
export const customCardFileV1Schema = z
  .strictObject({
    schema: z.literal("spellwake-custom-card-v1"),
    layout: customCardLayoutSchema,
    rarity: customCardRaritySchema,
    collectorNote: safeDisplayText(120).nullable(),
    faces: z.array(customCardFileFaceSchema).min(1).max(2),
  })
  .refine(
    (card) => matchesLayoutFaceCount(card.layout, card.faces.length),
    "Face count must match layout.",
  );
export type CustomCardFileV1 = z.infer<typeof customCardFileV1Schema>;

export const phrasingChangeSchema = z.strictObject({
  faceIndex: z.number().int().min(0).max(1),
  before: z.string().max(4096),
  after: z.string().max(4096),
  reason: z.string().min(1).max(256),
});
export type PhrasingChange = z.infer<typeof phrasingChangeSchema>;

export const standardizeResultSchema = z.strictObject({
  faces: z.array(customCardFaceRulesSchema).min(1).max(2),
  changes: z.array(phrasingChangeSchema).max(64),
  changed: z.boolean(),
});
export type StandardizeResult = z.infer<typeof standardizeResultSchema>;

export const compileDiagnosticSchema = z.strictObject({
  severity: z.enum(["error", "warning", "info"]),
  stage: z.enum(["metadata", "stage1", "stage2", "validation"]),
  faceIndex: z.number().int().min(0).max(1).nullable(),
  code: z.string().min(1).max(128),
  message: z.string().min(1).max(512),
  clauseText: z.string().max(1024).nullable().optional(),
});
export type CompileDiagnostic = z.infer<typeof compileDiagnosticSchema>;

export const astReadingItemSchema = z.strictObject({
  faceIndex: z.number().int().min(0).max(1),
  kind: z.enum([
    "keyword",
    "triggered",
    "activated",
    "static",
    "replacement",
    "spell_effect",
    "loyalty",
    "saga_chapter",
    "class_level",
  ]),
  label: z.string().min(1).max(128),
  summary: z.string().min(1).max(512),
});
export type AstReadingItem = z.infer<typeof astReadingItemSchema>;

export const customCardCompileResultSchema = z.strictObject({
  rulesSha256: z.string().regex(/^[0-9a-f]{64}$/),
  compilerRevision: z.string().min(1).max(64),
  stage1Parsed: z.boolean(),
  stage2Playable: z.boolean(),
  diagnostics: z.array(compileDiagnosticSchema).max(128),
  astReading: z.array(astReadingItemSchema).max(128),
  colorIdentity: z.array(customCardColorSchema).max(5),
  manaValue: z.number().int().nonnegative(),
});
export type CustomCardCompileResult = z.infer<
  typeof customCardCompileResultSchema
>;

export const customCardSaveArtItemSchema = z
  .strictObject({
    faceIndex: z.number().int().min(0).max(1),
    mimeType: z.enum(["image/webp", "image/png"]).optional(),
    artBase64: z
      .string()
      .max(175_000)
      .regex(/^[A-Za-z0-9+/=]+$/)
      .optional(),
    copyFromRevisionId: identifierSchema.optional(),
    cropX: finiteRange(-1, 1).default(0),
    cropY: finiteRange(-1, 1).default(0),
    zoom: finiteRange(0.5, 4).default(1),
    artist: safeDisplayText(80).nullable().optional(),
  })
  .refine(
    (item) =>
      (item.artBase64 !== undefined && item.mimeType !== undefined) ||
      item.copyFromRevisionId !== undefined,
    "Art upload requires artBase64+mimeType or copyFromRevisionId.",
  );
export type CustomCardSaveArtItem = z.infer<typeof customCardSaveArtItemSchema>;

export const saveCustomCardRequestSchema = z
  .strictObject({
    requestId: identifierSchema,
    expectedRevisionId: identifierSchema.nullable().optional(),
    layout: customCardLayoutSchema,
    rarity: customCardRaritySchema,
    faces: z.array(customCardFaceRulesSchema).min(1).max(2),
    visualFrameAccents: z.array(customCardFrameAccentSchema).max(2).default([]),
    flavorTexts: z.array(safeDisplayText(512).nullable()).max(2).default([]),
    collectorNote: safeDisplayText(120).nullable().default(null),
    art: z.array(customCardSaveArtItemSchema).max(2).default([]),
  })
  .refine(
    (card) => matchesLayoutFaceCount(card.layout, card.faces.length),
    "Face count must match layout.",
  );
export type SaveCustomCardRequest = z.infer<typeof saveCustomCardRequestSchema>;

export const customCardArtMetaSchema = z.strictObject({
  faceIndex: z.number().int().min(0).max(1),
  mimeType: z.enum(["image/webp", "image/png"]),
  artSha256: z.string().regex(/^[0-9a-f]{64}$/),
  hasArt: z.boolean(),
  cropX: finiteRange(-1, 1),
  cropY: finiteRange(-1, 1),
  zoom: finiteRange(0.5, 4),
  artist: z.string().max(80).nullable(),
});
export type CustomCardArtMeta = z.infer<typeof customCardArtMetaSchema>;

export const customCardRevisionViewSchema = z.strictObject({
  revisionId: identifierSchema,
  customCardId: identifierSchema,
  revisionNumber: positiveAmountSchema,
  rulesSha256: z.string().regex(/^[0-9a-f]{64}$/),
  name: z.string().min(1).max(120),
  layout: customCardLayoutSchema,
  rarity: customCardRaritySchema,
  faces: z.array(customCardFaceRulesSchema).min(1).max(2),
  visualFrameAccents: z.array(customCardFrameAccentSchema).max(2),
  flavorTexts: z.array(z.string().max(512).nullable()).max(2),
  collectorNote: z.string().max(120).nullable(),
  stage1Parsed: z.boolean(),
  stage2Playable: z.boolean(),
  compilerRevision: z.string().min(1).max(64),
  art: z.array(customCardArtMetaSchema).max(2),
  createdAt: instantSchema,
});
export type CustomCardRevisionView = z.infer<
  typeof customCardRevisionViewSchema
>;

export const customCardSummarySchema = z.strictObject({
  id: identifierSchema,
  archivedAt: instantSchema.nullable(),
  createdAt: instantSchema,
  updatedAt: instantSchema,
  currentRevision: customCardRevisionViewSchema,
});
export type CustomCardSummary = z.infer<typeof customCardSummarySchema>;

export const customCardPageSchema = z.strictObject({
  accountId: identifierSchema,
  activeCount: z.number().int().nonnegative(),
  activeLimit: z.number().int().positive(),
  items: z.array(customCardSummarySchema).max(100),
  nextCursor: identifierSchema.nullable(),
});
export type CustomCardPage = z.infer<typeof customCardPageSchema>;

export const customCardMutationAckSchema = z.strictObject({
  resourceId: identifierSchema,
  revisionId: identifierSchema.optional(),
  revisionNumber: positiveAmountSchema.optional(),
  archived: z.boolean().optional(),
  disposition: z.enum(["deleted", "archived"]).optional(),
});
export type CustomCardMutationAck = z.infer<typeof customCardMutationAckSchema>;

export const savedCustomCardResponseSchema = z.strictObject({
  customCardId: identifierSchema,
  revisionId: identifierSchema,
  revisionNumber: positiveAmountSchema,
  compile: customCardCompileResultSchema,
});
export type SavedCustomCardResponse = z.infer<
  typeof savedCustomCardResponseSchema
>;

export const customCardArchiveRequestSchema = z.strictObject({
  requestId: identifierSchema,
  archived: z.boolean(),
});
export type CustomCardArchiveRequest = z.infer<
  typeof customCardArchiveRequestSchema
>;

export const customCardDeleteRequestSchema = z.strictObject({
  requestId: identifierSchema,
});
export type CustomCardDeleteRequest = z.infer<
  typeof customCardDeleteRequestSchema
>;

/** Vision OCR fallback request for external raster images (PNG/WebP/JPEG <= 512 KiB decoded). */
export const visionImportRequestSchema = z.strictObject({
  sourceKind: z.enum(["external_png", "external_image"]),
  imageDataUrl: z
    .string()
    .max(700_000)
    .regex(/^data:image\/(png|webp|jpeg);base64,[A-Za-z0-9+/=]+$/),
});
export type VisionImportRequest = z.infer<typeof visionImportRequestSchema>;

export const visionArtCropBoxSchema = z.strictObject({
  x: finiteRange(0, 1),
  y: finiteRange(0, 1),
  width: finiteRange(0.01, 1),
  height: finiteRange(0.01, 1),
});
export type VisionArtCropBox = z.infer<typeof visionArtCropBoxSchema>;

export const visionImportedCardSchema = z.strictObject({
  layout: customCardLayoutSchema,
  rarity: customCardRaritySchema,
  faces: z.array(customCardFaceRulesSchema).min(1).max(2),
  flavorTexts: z.array(z.string().max(512).nullable()).max(2),
  collectorNote: z.string().max(120).nullable(),
  artist: z.string().max(80).nullable(),
  artCropBox: visionArtCropBoxSchema.nullable(),
  standardized: standardizeResultSchema,
  compiled: customCardCompileResultSchema,
});
export type VisionImportedCard = z.infer<typeof visionImportedCardSchema>;

export const visionImportResultSchema = z.strictObject({
  cards: z.array(visionImportedCardSchema).min(1).max(1),
  warnings: z.array(z.string().max(512)).max(16),
});
export type VisionImportResult = z.infer<typeof visionImportResultSchema>;

export const customDeckEntrySchema = z.strictObject({
  customCardRevisionId: identifierSchema,
  section: z.string().regex(/^[a-z][a-z0-9_]{0,31}$/),
  quantity: positiveAmountSchema,
});
export type CustomDeckEntry = z.infer<typeof customDeckEntrySchema>;

export const customDeckEntriesSchema = z
  .array(customDeckEntrySchema)
  .max(1000)
  .refine((entries) => {
    const identities = new Set(
      entries.map(
        (entry) => `${entry.customCardRevisionId}/${entry.section}`,
      ),
    );
    return identities.size === entries.length;
  }, "Duplicate custom deck entries.");

export const quickTestCommandSchema = z.strictObject({
  requestId: identifierSchema,
  customCardRevisionId: identifierSchema,
  aiPlaysCustomCard: z.boolean().default(false),
});
export type QuickTestCommand = z.infer<typeof quickTestCommandSchema>;

export const quickTestAckSchema = z.strictObject({
  roomId: identifierSchema,
  deckId: identifierSchema,
  deckRevisionId: identifierSchema,
  aiSlotId: identifierSchema.nullable(),
});
export type QuickTestAck = z.infer<typeof quickTestAckSchema>;

export const customCardArtRelativePathSchema = z
  .string()
  .max(256)
  .regex(
    /^\/v1\/(custom-cards\/revisions\/[0-9a-f-]{36}\/art\/[01]|rooms\/[0-9a-f-]{36}\/lent-decks\/[0-9a-f-]{36}\/face-art\/[0-9a-f-]{36}|games\/[0-9a-f-]{36}\/custom-cards\/[0-9a-f-]{36}\/art\/[01]\?contextId=[0-9a-f-]{36})$/,
  );

/**
 * Discriminated deck face card supporting official Scryfall art and custom revision art,
 * with transitional acceptance of legacy untagged `{ definitionId, artUrl }` responses
 * during the Phase 2 -> Phase 3 zero-downtime rollout window.
 */
export const deckFaceCardSchema = z.union([
  z.strictObject({
    kind: z.literal("official"),
    definitionId: identifierSchema,
    artUrl: z.string().max(2048).startsWith("https://").nullable(),
  }),
  z.strictObject({
    kind: z.literal("custom"),
    customCardRevisionId: identifierSchema,
    artPath: customCardArtRelativePathSchema.nullable(),
    artSha256: z
      .string()
      .regex(/^[0-9a-f]{64}$/)
      .nullable()
      .optional(),
  }),
  z
    .strictObject({
      definitionId: identifierSchema,
      artUrl: z.string().max(2048).startsWith("https://").nullable(),
    })
    .transform((legacy) => ({
      kind: "official" as const,
      definitionId: legacy.definitionId,
      artUrl: legacy.artUrl,
    })),
]);
export type DeckFaceCard = z.output<typeof deckFaceCardSchema>;
export type DeckFaceCardInput = z.input<typeof deckFaceCardSchema>;
export const customCardAmountSchema = amountSchema;

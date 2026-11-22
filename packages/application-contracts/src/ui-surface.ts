/** Zod contracts for the completed Spellwake UI API surface. */
import { z } from "zod";
import { amountSchema, identifierSchema, instantSchema } from "./primitives.ts";

export const cardFaceMetadataSchema = z.object({
  faceIndex: z.number().int().nonnegative(),
  name: z.string(),
  characteristicsRef: z.string(),
});

export const cardMetadataSchema = z.object({
  printingId: identifierSchema,
  definitionId: identifierSchema,
  oracleRef: z.string(),
  name: z.string(),
  externalPrintingRef: z.string(),
  rarity: z.string(),
  finish: z.string(),
  imageUrl: z.string().nullable(),
  setId: identifierSchema,
  setName: z.string(),
  setCode: z.string(),
  rulesTextRef: z.string().nullable(),
  faces: z.array(cardFaceMetadataSchema),
  legalities: z.record(z.string(), z.string()),
});
export type CardMetadata = z.infer<typeof cardMetadataSchema>;

export const cardMetadataBatchSchema = z.object({
  printings: z.array(cardMetadataSchema),
  definitions: z.array(cardMetadataSchema),
});
export type CardMetadataBatch = z.infer<typeof cardMetadataBatchSchema>;

export const richCardSearchPageSchema = z.object({
  items: z.array(cardMetadataSchema),
  nextCursor: identifierSchema.nullable(),
});

export const duplicateDeckAckSchema = z.object({
  resourceId: identifierSchema,
  revisionId: identifierSchema.nullable(),
});

export const richCosmeticSchema = z.object({
  id: identifierSchema,
  kind: z.enum(["back", "sleeve", "dice", "playmat", "pet"]),
  name: z.string(),
  description: z.string().nullable(),
  assetRef: z.string(),
  previewUrl: z.string(),
  rarity: z.string().nullable(),
  state: z.string(),
  priceCents: z.number().int().nonnegative().nullable(),
  priceCoins: amountSchema.nullable(),
  owned: z.boolean(),
  equipped: z.boolean(),
});
export type RichCosmetic = z.infer<typeof richCosmeticSchema>;

export const richCosmeticPageSchema = z.object({
  accountId: identifierSchema,
  items: z.array(richCosmeticSchema),
  nextCursor: identifierSchema.nullable(),
});

export const profileBadgeResultSchema = z.object({
  accountId: identifierSchema,
  selectedBadge: z.string().nullable(),
});

export const patreonAccountStatusSchema = z.object({
  accountId: identifierSchema,
  linked: z.boolean(),
  active: z.boolean(),
  supporter: z.boolean(),
  tier: z
    .object({
      id: identifierSchema,
      name: z.string(),
      rank: z.number().int(),
    })
    .nullable(),
});
export type PatreonAccountStatus = z.infer<typeof patreonAccountStatusSchema>;

export const billingPortalSessionSchema = z.object({
  url: z.url(),
});

export const leaderboardEntrySchema = z.object({
  rankPosition: z.number().int().positive(),
  id: identifierSchema,
  accountId: identifierSchema,
  username: z.string(),
  formatId: identifierSchema,
  seasonId: identifierSchema,
  tier: z.string(),
  rating: z.string(),
  finalized: z.boolean(),
  selectedBadge: z.string().nullable(),
  cardArtProfile: z.unknown().nullable(),
  supporter: z.boolean(),
  wins: z.number().int().nonnegative(),
  losses: z.number().int().nonnegative(),
  draws: z.number().int().nonnegative(),
});
export type LeaderboardEntry = z.infer<typeof leaderboardEntrySchema>;

export const leaderboardPageSchema = z.object({
  seasonId: identifierSchema.nullable(),
  formatId: identifierSchema.nullable(),
  offset: z.number().int().nonnegative(),
  total: z.number().int().nonnegative(),
  items: z.array(leaderboardEntrySchema),
});

export const playerPublicSummarySchema = z.object({
  viewerAccountId: identifierSchema,
  person: z.object({
    id: identifierSchema,
    username: z.string(),
    isDeleted: z.boolean(),
  }),
  cardArtProfile: z.unknown().nullable(),
  selectedBadge: z.string().nullable(),
  supporter: z.boolean(),
  ranks: z.array(
    z.object({
      id: identifierSchema,
      formatId: identifierSchema,
      seasonId: identifierSchema,
      tier: z.string(),
      rating: z.string(),
      finalized: z.boolean(),
    }),
  ),
  record: z.object({
    wins: z.number().int().nonnegative(),
    losses: z.number().int().nonnegative(),
    draws: z.number().int().nonnegative(),
  }),
});
export type PlayerPublicSummary = z.infer<typeof playerPublicSummarySchema>;

export const marketSearchPageSchema = z.object({
  accountId: identifierSchema,
  offset: z.number().int().nonnegative(),
  total: z.number().int().nonnegative(),
  items: z.array(z.record(z.string(), z.unknown())),
});

export const tradeSessionItemSchema = z.object({
  assetId: identifierSchema,
  quantity: amountSchema,
  asset: z.record(z.string(), z.unknown()),
});

export const tradeSessionSchema = z.object({
  id: identifierSchema,
  state: z.enum(["active", "completed", "canceled"]),
  revision: amountSchema,
  initiator: z.object({
    id: identifierSchema,
    username: z.string(),
    isDeleted: z.boolean(),
  }),
  counterparty: z.object({
    id: identifierSchema,
    username: z.string(),
    isDeleted: z.boolean(),
  }),
  initiatorReady: z.boolean(),
  counterpartyReady: z.boolean(),
  initiatorConfirmed: z.boolean(),
  counterpartyConfirmed: z.boolean(),
  initiatorCoins: amountSchema,
  counterpartyCoins: amountSchema,
  initiatorItems: z.array(tradeSessionItemSchema),
  counterpartyItems: z.array(tradeSessionItemSchema),
  settlementOperationId: identifierSchema.nullable().optional(),
  createdAt: instantSchema,
  updatedAt: instantSchema,
});
export type TradeSession = z.infer<typeof tradeSessionSchema>;

export const tradeSessionEnvelopeSchema = z.object({
  accountId: identifierSchema.optional(),
  session: tradeSessionSchema,
});

export const tradeSessionListSchema = z.object({
  accountId: identifierSchema,
  items: z.array(tradeSessionSchema),
});

export const predictionSearchPageSchema = z.object({
  accountId: identifierSchema,
  offset: z.number().int().nonnegative(),
  total: z.number().int().nonnegative(),
  items: z.array(z.record(z.string(), z.unknown())),
});

export const roomGameHandoffSchema = z.discriminatedUnion("state", [
  z.object({
    state: z.literal("waiting"),
    accountId: identifierSchema,
    roomId: identifierSchema,
    roomState: z.string(),
  }),
  z.object({
    state: z.literal("admitted"),
    accountId: identifierSchema,
    roomId: identifierSchema,
    matchId: identifierSchema,
    seatId: identifierSchema,
    game: z
      .object({
        id: identifierSchema,
        state: z.string(),
        contextId: identifierSchema.nullable(),
      })
      .nullable(),
  }),
]);
export type RoomGameHandoff = z.infer<typeof roomGameHandoffSchema>;

export const roomEnrichedDetailSchema = z.object({
  accountId: identifierSchema,
  room: z.object({
    id: identifierSchema,
    hostId: identifierSchema,
    formatId: identifierSchema,
    formatKey: z.string(),
    formatName: z.string(),
    profileId: identifierSchema,
    mode: z.string(),
    competitive: z.boolean(),
    visibility: z.string(),
    state: z.string(),
    revision: amountSchema,
    countdownAt: instantSchema.nullable(),
    aiDifficulty: z.enum(["casual", "standard", "competitive"]),
    planechaseEnabled: z.boolean(),
    rangeOfInfluence: z.number().int().nullable(),
  }),
  slots: z.array(
    z.object({
      slotId: identifierSchema,
      seatIndex: z.number().int(),
      controllerKind: z.string(),
      accountId: identifierSchema.nullable(),
      username: z.string().nullable(),
      supporter: z.boolean(),
      selectedBadge: z.string().nullable(),
      cardArtProfile: z.unknown().nullable(),
      deckRevisionId: identifierSchema.nullable(),
      ready: z.boolean(),
      teamRef: z.string().nullable(),
    }),
  ),
});
export type RoomEnrichedDetail = z.infer<typeof roomEnrichedDetailSchema>;

export const roomInviteNotificationAckSchema = z.object({
  notificationId: identifierSchema,
  roomId: identifierSchema,
  invitedAccountId: identifierSchema,
});

export const limitedDraftStateSchema = z.object({
  draftId: identifierSchema,
  step: z.number().int().nonnegative(),
  complete: z.boolean(),
  picked: z.boolean(),
  pack: z.array(
    z.object({
      itemId: identifierSchema,
      printingId: identifierSchema,
    }),
  ),
  pool: z.array(identifierSchema),
});
export type LimitedDraftState = z.infer<typeof limitedDraftStateSchema>;

export const limitedDraftAckSchema = z.object({
  draftId: identifierSchema,
  step: z.string().optional(),
  itemId: identifierSchema.optional(),
});

export const dmAttachmentMessageSchema = z.object({
  id: identifierSchema,
  conversationId: identifierSchema,
  senderId: identifierSchema,
  body: z.string(),
  attachment: z.record(z.string(), z.unknown()),
  sentAt: instantSchema,
});

export const openingChoiceResponseSchema = z.object({
  gameId: identifierSchema,
  chooserSeatId: identifierSchema,
  selectedFirstSeatId: identifierSchema.nullable(),
  playFirst: z.boolean().nullable(),
});

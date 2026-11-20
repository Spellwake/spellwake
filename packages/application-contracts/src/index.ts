import { appearanceSchema } from "./appearance.ts";
export * from "./qualified-intents.ts";
export { appearanceSchema, type AppearanceSelection } from "./appearance.ts";
import { makeCommandDispositionSchema } from "./command-protocol.ts";
/** Validated public application protocol, paired with native Rust DTOs. */
import { z } from "zod";

import {
  identifierSchema,
  amountSchema,
  positiveAmountSchema,
  instantSchema,
  usernameSchema,
} from "./primitives.ts";
export {
  identifierSchema,
  amountSchema,
  instantSchema,
  usernameSchema,
} from "./primitives.ts";
export * from "./catalogue.ts";
export * from "./social.ts";
export * from "./economy.ts";
export * from "./exchange-reads.ts";
import { assetIdentitySchema as assetSchema } from "./assets.ts";
import { economyCommandErrorsSchema } from "./economy.ts";
import { predictionCommandErrorsSchema } from "./predictions.ts";
import {
  DIRECT_MESSAGE_PAGE_LIMIT,
  socialMessageBodySchema,
} from "./social.ts";
export const requestDeletionSchema = z.strictObject({
  requestId: identifierSchema,
  statusKeyHash: z.string().regex(/^[0-9a-f]{64}$/),
});
export type RequestDeletion = z.infer<typeof requestDeletionSchema>;
export const observeDeletionSchema = z.strictObject({
  accountId: identifierSchema,
  requestId: identifierSchema,
  // Exactly 32 bytes, no padding or nonzero unused bits in the final symbol.
  statusKey: z.string().regex(/^[A-Za-z0-9_-]{42}[AEIMQUYcgkosw048]$/),
});
export type ObserveDeletion = z.infer<typeof observeDeletionSchema>;
const deletionObservationIdentity = {
  accountId: identifierSchema,
  requestId: identifierSchema,
};
export const deletionObservationSchema = z.discriminatedUnion("phase", [
  z.strictObject({
    ...deletionObservationIdentity,
    phase: z.literal("pending"),
  }),
  z.strictObject({
    ...deletionObservationIdentity,
    phase: z.literal("finalizing"),
    retirementOperationId: identifierSchema,
  }),
  z.strictObject({
    ...deletionObservationIdentity,
    phase: z.literal("deleted"),
    retirementOperationId: identifierSchema,
    authCleanupReceiptId: identifierSchema,
  }),
]);
export type DeletionObservation = z.infer<typeof deletionObservationSchema>;
export const signInMethodSchema = z.enum([
  "google",
  "apple",
  "discord",
  "email_code",
]);
const sessionIdentity = {
  subjectId: identifierSchema,
  sessionId: identifierSchema,
  method: signInMethodSchema,
};
export const accountSessionSchema = z.discriminatedUnion("state", [
  z.strictObject({
    ...sessionIdentity,
    state: z.literal("needs_registration"),
    accountId: z.null(),
  }),
  z.strictObject({
    ...sessionIdentity,
    state: z.literal("active"),
    accountId: identifierSchema,
  }),
  z.strictObject({
    ...sessionIdentity,
    state: z.literal("deletion_pending"),
    accountId: identifierSchema,
  }),
]);
export type AccountSession = z.infer<typeof accountSessionSchema>;

/** Every successful payload is validated separately from business and server failures. */
export function apiResponseSchema<T extends z.ZodType>(data: T) {
  return z.discriminatedUnion("status", [
    z.strictObject({ status: z.literal("success"), data }),
    z.strictObject({
      status: z.literal("fail"),
      data: z.record(z.string(), z.string()),
    }),
    z.strictObject({
      status: z.literal("error"),
      message: z.string(),
      code: z.string().optional(),
      data: z.unknown().optional(),
    }),
  ]);
}
export const registerAccountSchema = z.strictObject({
  username: usernameSchema,
});
export const registeredAccountSchema = z.strictObject({
  accountId: identifierSchema,
});
export const commandAckSchema = z.strictObject({
  resourceId: identifierSchema,
});
export type CommandAck = z.infer<typeof commandAckSchema>;
export const checkoutInputSchema = z.strictObject({
  requestId: identifierSchema,
  cosmeticId: identifierSchema,
});
/** An immutable failed reservation receipt, never a generic preflight failure. */
export const checkoutRejectionSchema = z.strictObject({
  request: z.literal("terminal_rejection"),
  requestId: identifierSchema,
  cosmeticId: identifierSchema,
  reason: z.enum(["cosmetic_unavailable", "account_unavailable"]),
});

export const inventoryItemSchema = z
  .strictObject({
    id: identifierSchema,
    assetId: identifierSchema,
    revision: amountSchema,
    owned: amountSchema,
    held: amountSchema,
    available: amountSchema,
    asset: assetSchema,
  })
  .refine(
    (item) =>
      BigInt(item.owned) > 0n &&
      BigInt(item.owned) - BigInt(item.held) === BigInt(item.available),
    "Inconsistent available inventory.",
  );
export const inventoryPageSchema = z
  .strictObject({
    items: z.array(inventoryItemSchema).max(100),
    nextCursor: identifierSchema.nullable(),
  })
  .refine(
    (page) =>
      page.items.every(
        (item, index) =>
          index === 0 || (page.items[index - 1]?.id ?? "") < item.id,
      ) &&
      (page.nextCursor === null || page.items.at(-1)?.id === page.nextCursor),
    "Invalid inventory cursor.",
  );
export type InventoryPage = z.infer<typeof inventoryPageSchema>;

/** Hosted checkout is confined to the provider's exact HTTPS origin. */
export const hostedCheckoutUrlSchema = z
  .url()
  .max(4096)
  .refine((value) => {
    const url = new URL(value);
    return (
      url.origin === "https://checkout.stripe.com" &&
      !url.username &&
      !url.password
    );
  });
const checkoutViewSchema = z.discriminatedUnion("status", [
  z.strictObject({ status: z.literal("waiting") }),
  z.strictObject({ status: z.literal("unavailable") }),
  z.strictObject({
    status: z.literal("ready"),
    expiresAt: instantSchema,
    url: hostedCheckoutUrlSchema,
  }),
]);
export const purchaseSchema = z.strictObject({
  purchaseId: identifierSchema,
  cosmeticId: identifierSchema,
  createdAt: instantSchema,
  state: z.enum([
    "creating",
    "pending",
    "paid",
    "canceled",
    "refunded",
    "reversed",
  ]),
  revision: amountSchema,
  price: z.strictObject({
    currency: z.string().regex(/^[a-z]{3}$/),
    minorUnits: z.string().regex(/^[1-9][0-9]{0,7}$/),
  }),
  checkout: checkoutViewSchema,
});
export type Purchase = z.infer<typeof purchaseSchema>;

export const cosmeticSummarySchema = z
  .strictObject({
    id: identifierSchema,
    name: z
      .string()
      .max(240)
      .refine(
        (value) =>
          Array.from(value).length >= 1 &&
          Array.from(value).length <= 120 &&
          !value.startsWith(" ") &&
          !value.endsWith(" "),
      ),
    kind: z.enum(["back", "sleeve", "dice", "playmat"]),
    isFree: z.boolean(),
    isOwned: z.boolean(),
    includedWithPatreon: z.boolean(),
  })
  .refine((item) => !item.isFree || item.isOwned);
export type CosmeticSummary = z.infer<typeof cosmeticSummarySchema>;
export const cosmeticCatalogueSchema = z
  .strictObject({
    items: z.array(cosmeticSummarySchema).max(100),
    nextCursor: identifierSchema.nullable(),
  })
  .refine(
    (page) =>
      page.items.every(
        (item, index) =>
          index === 0 || (page.items[index - 1]?.id ?? "") < item.id,
      ) &&
      (page.nextCursor === null || page.items.at(-1)?.id === page.nextCursor),
  );
export const purchasePageSchema = z
  .strictObject({
    items: z.array(purchaseSchema).max(100),
    nextCursor: identifierSchema.nullable(),
  })
  .refine(
    (page) =>
      page.items.every(
        (item, index) =>
          index === 0 ||
          (page.items[index - 1]?.purchaseId ?? "") < item.purchaseId,
      ) &&
      (page.nextCursor === null ||
        page.items.at(-1)?.purchaseId === page.nextCursor),
  );

export const changeProfileDefaultsSchema = z.strictObject({
  requestId: identifierSchema,
  expected: appearanceSchema,
  appearance: appearanceSchema,
});
export type ChangeProfileDefaults = z.infer<typeof changeProfileDefaultsSchema>;
export const profileDefaultsErrorsSchema = z.union([
  z.strictObject({ account: z.literal("account unavailable") }),
  z.strictObject({
    defaults: z.literal("account defaults changed; reload before saving"),
  }),
  z.strictObject({ cosmetics: z.literal("cosmetic selection unavailable") }),
]);
export const profileDefaultsDispositionSchema = makeCommandDispositionSchema(
  profileDefaultsErrorsSchema,
  commandAckSchema,
);
export type ProfileDefaultsDisposition = z.infer<
  typeof profileDefaultsDispositionSchema
>;
/** Social commands never carry their actor; the original session selects it. */
export const socialCommandSchema = z.discriminatedUnion("kind", [
  z.strictObject({
    kind: z.literal("friendship"),
    command: z.strictObject({
      requestId: identifierSchema,
      otherAccountId: identifierSchema,
      action: z.enum(["request", "accept", "remove"]),
    }),
  }),
  z.strictObject({
    kind: z.literal("block"),
    command: z.strictObject({
      requestId: identifierSchema,
      otherAccountId: identifierSchema,
      blocked: z.boolean(),
    }),
  }),
  z.strictObject({
    kind: z.literal("open_dm"),
    command: z.strictObject({
      requestId: identifierSchema,
      otherAccountId: identifierSchema,
    }),
  }),
  z.strictObject({
    kind: z.literal("send_dm"),
    command: z.strictObject({
      requestId: identifierSchema,
      conversationId: identifierSchema,
      body: socialMessageBodySchema,
    }),
  }),
]);
export type SocialCommand = z.infer<typeof socialCommandSchema>;
export const socialCommandErrorsSchema = z.union([
  z.strictObject({ contact: z.literal("contact unavailable") }),
  z.strictObject({
    friendship: z.enum([
      "no incoming request to accept",
      "friendship unavailable",
    ]),
  }),
  z.strictObject({ conversation: z.literal("conversation unavailable") }),
  z.strictObject({
    body: z.literal("message must contain 1 to 4000 characters"),
  }),
  z.strictObject({ action: z.literal("invalid friend action") }),
]);
export const socialCommandDispositionSchema = makeCommandDispositionSchema(
  socialCommandErrorsSchema,
  commandAckSchema,
);
export const economyCommandDispositionSchema = makeCommandDispositionSchema(
  economyCommandErrorsSchema,
  commandAckSchema,
);
export type EconomyCommandDisposition = z.infer<
  typeof economyCommandDispositionSchema
>;
export type SocialCommandDisposition = z.infer<
  typeof socialCommandDispositionSchema
>;
export const markDirectReadSchema = z.strictObject({
  requestId: identifierSchema,
  conversationId: identifierSchema,
  messageIds: z
    .array(identifierSchema)
    .min(1)
    .max(DIRECT_MESSAGE_PAGE_LIMIT)
    .refine((ids) => new Set(ids).size === ids.length),
});
export type MarkDirectRead = z.infer<typeof markDirectReadSchema>;
export const directReadErrorsSchema = z.strictObject({
  conversation: z.literal("conversation unavailable"),
});
export const directReadDispositionSchema = makeCommandDispositionSchema(
  directReadErrorsSchema,
  commandAckSchema,
);
const walletSchema = z
  .strictObject({
    owned: amountSchema,
    held: amountSchema,
    available: amountSchema,
  })
  .refine(
    (wallet) =>
      BigInt(wallet.owned) - BigInt(wallet.held) === BigInt(wallet.available),
    "Inconsistent available coins.",
  );
export const accountSummarySchema = z.strictObject({
  accountId: identifierSchema,
  username: usernameSchema,
  state: z.enum(["active", "deletion_pending"]),
  appearanceRevision: amountSchema,
  defaults: appearanceSchema,
  wallet: walletSchema,
});
export type AccountSummary = z.infer<typeof accountSummarySchema>;
export const deckFormatSchema = z.strictObject({
  id: identifierSchema,
  key: z.string().min(1),
  name: z.string().min(1),
});
export const deckEntrySchema = z.strictObject({
  definitionId: identifierSchema,
  printingId: identifierSchema.nullable(),
  section: z.string().regex(/^[a-z][a-z0-9_]{0,31}$/),
  quantity: positiveAmountSchema,
});
const deckEntriesSchema = z
  .array(deckEntrySchema)
  .max(1000)
  .refine((entries) => {
    const identities = new Set(
      entries.map(
        (entry) =>
          `${entry.definitionId}/${entry.printingId ?? ""}/${entry.section}`,
      ),
    );
    return identities.size === entries.length;
  }, "Duplicate deck entries.");
export const createDeckSchema = z.strictObject({
  requestId: identifierSchema,
  name: z
    .string()
    .refine(
      (name) =>
        Array.from(name.trim()).length >= 1 &&
        Array.from(name.trim()).length <= 120 &&
        !name.includes("\0"),
    ),
  mode: z.enum(["open", "collection"]),
  formatId: identifierSchema,
});
/** Resolved content import is one atomic library transaction, not match admission. */
export const importDeckSchema = z
  .strictObject({ ...createDeckSchema.shape, entries: deckEntriesSchema })
  .refine(
    (input) => new TextEncoder().encode(JSON.stringify(input)).length <= 262144,
    "Deck import is too large.",
  );
export const importDeckAckSchema = z
  .strictObject({ resourceId: identifierSchema, revisionId: identifierSchema })
  .refine((value) => value.resourceId !== value.revisionId);
export type ImportDeck = z.infer<typeof importDeckSchema>;
export type ImportDeckAck = z.infer<typeof importDeckAckSchema>;
export const saveDeckSchema = z
  .strictObject({
    requestId: identifierSchema,
    deckId: identifierSchema,
    expectedRevisionId: identifierSchema.nullable(),
    entries: deckEntriesSchema,
  })
  .refine(
    (input) => new TextEncoder().encode(JSON.stringify(input)).length <= 262144,
    "Deck request is too large.",
  );
/** Settings updates compare the complete observed baseline, independently of contents. */
export const deckSettingsSchema = z.strictObject({
  name: createDeckSchema.shape.name,
  mode: createDeckSchema.shape.mode,
  formatId: identifierSchema,
  appearance: appearanceSchema,
});
export const changeDeckSettingsSchema = z
  .strictObject({
    requestId: identifierSchema,
    deckId: identifierSchema,
    expected: deckSettingsSchema,
    settings: deckSettingsSchema,
  })
  .refine(
    (input) => new TextEncoder().encode(JSON.stringify(input)).length <= 4096,
    "Deck settings request is too large.",
  );
export type DeckSettings = z.infer<typeof deckSettingsSchema>;
export type ChangeDeckSettings = z.infer<typeof changeDeckSettingsSchema>;
export type CreateDeck = z.infer<typeof createDeckSchema>;
export type SaveDeck = z.infer<typeof saveDeckSchema>;
export const deckCommandSchema = z.discriminatedUnion("kind", [
  z.strictObject({ kind: z.literal("create"), command: createDeckSchema }),
  z.strictObject({ kind: z.literal("save"), command: saveDeckSchema }),
  z.strictObject({
    kind: z.literal("settings"),
    command: changeDeckSettingsSchema,
  }),
]);
export type DeckCommand = z.infer<typeof deckCommandSchema>;
export const deckCommandErrorsSchema = z.union([
  z.strictObject({ account: z.literal("account unavailable") }),
  z.strictObject({
    deck: z.enum(["invalid deck metadata", "deck unavailable"]),
  }),
  z.strictObject({ revision: z.literal("deck changed; reload before saving") }),
  z.strictObject({
    entries: z.enum([
      "expected deck entry array",
      "too many deck entries",
      "invalid deck entries",
    ]),
  }),
  z.strictObject({
    cosmetics: z.enum([
      "default cosmetic unavailable",
      "cosmetic selection unavailable",
    ]),
  }),
  z.strictObject({
    settings: z.literal("deck settings changed; reload before saving"),
  }),
]);
export const deckCommandDispositionSchema = makeCommandDispositionSchema(
  deckCommandErrorsSchema,
  commandAckSchema,
);
export type DeckCommandDisposition = z.infer<
  typeof deckCommandDispositionSchema
>;
export const deckSummarySchema = z.strictObject({
  id: identifierSchema,
  name: z
    .string()
    .max(240)
    .refine(
      (value) =>
        Array.from(value).length >= 1 &&
        Array.from(value).length <= 120 &&
        !value.startsWith(" ") &&
        !value.endsWith(" "),
    ),
  mode: z.enum(["open", "collection"]),
  currentRevisionId: identifierSchema.nullable(),
  format: deckFormatSchema,
  appearance: appearanceSchema,
});
export type DeckSummary = z.infer<typeof deckSummarySchema>;
export const deckDetailSchema = z
  .strictObject({
    deck: deckSummarySchema,
    revision: z
      .strictObject({
        id: identifierSchema,
        number: positiveAmountSchema,
        entries: deckEntriesSchema,
      })
      .nullable(),
  })
  .refine(
    (detail) => detail.deck.currentRevisionId === (detail.revision?.id ?? null),
    "Inconsistent saved revision.",
  );
export type DeckDetail = z.infer<typeof deckDetailSchema>;
export const deckFormatPageSchema = z
  .strictObject({
    items: z.array(deckFormatSchema).max(100),
    nextCursor: identifierSchema.nullable(),
  })
  .refine(
    (page) =>
      page.items.every(
        (item, index) =>
          index === 0 || (page.items[index - 1]?.id ?? "") < item.id,
      ) &&
      (page.nextCursor === null || page.nextCursor === page.items.at(-1)?.id),
    "Invalid format cursor.",
  );
export const deckLibraryPageSchema = z
  .strictObject({
    items: z.array(deckSummarySchema).max(100),
    nextCursor: identifierSchema.nullable(),
  })
  .refine(
    (page) =>
      page.items.every(
        (item, index) =>
          index === 0 || (page.items[index - 1]?.id ?? "") < item.id,
      ) &&
      (page.nextCursor === null || page.items.at(-1)?.id === page.nextCursor),
    "Invalid library cursor.",
  );

export * from "./pack-shop.ts";

export * from "./predictions.ts";
export const predictionCommandDispositionSchema = makeCommandDispositionSchema(
  predictionCommandErrorsSchema,
  commandAckSchema,
);
export type PredictionCommandDisposition = z.infer<
  typeof predictionCommandDispositionSchema
>;

export * from "./prediction-reads.ts";

export * from "./prediction-values.ts";
export * from "./prediction-publication.ts";

export * from "./patreon-association.ts";

export const importDeckErrorsSchema = z.union([
  z.strictObject({ account: z.enum(["unavailable", "account unavailable"]) }),
  z.strictObject({
    deck: z.enum(["invalid deck metadata", "deck unavailable"]),
  }),
  z.strictObject({
    entries: z.enum([
      "expected deck entry array",
      "too many deck entries",
      "invalid deck entries",
    ]),
  }),
  z.strictObject({ cosmetics: z.literal("default cosmetic unavailable") }),
]);
export const importDeckDispositionSchema = makeCommandDispositionSchema(
  importDeckErrorsSchema,
  importDeckAckSchema,
);
export type ImportDeckDisposition = z.infer<typeof importDeckDispositionSchema>;
export * from "./progression.ts";

export * from "./rooms.ts";
import {
  roomCommandErrorsSchema,
  roomConfigurationErrorsSchema,
  roomConfigurationAckSchema,
  roomGameSchema,
} from "./rooms.ts";
export const roomGameResponseSchema = apiResponseSchema(roomGameSchema);
export const roomCommandDispositionSchema = makeCommandDispositionSchema(
  roomCommandErrorsSchema,
  commandAckSchema,
);

export const roomConfigurationDispositionSchema = makeCommandDispositionSchema(
  roomConfigurationErrorsSchema,
  roomConfigurationAckSchema,
);

export * from "./invitations.ts";
import {
  issuedInvitationSchema,
  joinRoomAckSchema,
  issueInvitationErrorsSchema,
  joinRoomErrorsSchema,
  revokeInvitationErrorsSchema,
} from "./invitations.ts";
export const issueInvitationDispositionSchema = makeCommandDispositionSchema(
  issueInvitationErrorsSchema,
  issuedInvitationSchema,
);
export const joinRoomDispositionSchema = makeCommandDispositionSchema(
  joinRoomErrorsSchema,
  joinRoomAckSchema,
);
export const revokeInvitationDispositionSchema = makeCommandDispositionSchema(
  revokeInvitationErrorsSchema,
  commandAckSchema,
);

export * from "./matchmaking.ts";
import {
  enqueueMatchmakingAckSchema,
  enqueueMatchmakingErrorsSchema,
  cancelMatchmakingErrorsSchema,
} from "./matchmaking.ts";
export const enqueueMatchmakingDispositionSchema = makeCommandDispositionSchema(
  enqueueMatchmakingErrorsSchema,
  enqueueMatchmakingAckSchema,
);
export const cancelMatchmakingDispositionSchema = makeCommandDispositionSchema(
  cancelMatchmakingErrorsSchema,
  commandAckSchema,
);

export * from "./game-chat.ts";
export * from "./notifications.ts";

/** Only canonical persisted failures belong in terminal game-chat receipts. */
export const gameMessageErrorsSchema = z.union([
  z.strictObject({ conversation: z.literal("conversation unavailable") }),
  z.strictObject({
    body: z.literal("message must contain 1 to 4000 characters"),
  }),
]);
export const notificationReadErrorsSchema = z.strictObject({
  notifications: z.literal("unavailable"),
});
export const gameMessageDispositionSchema = makeCommandDispositionSchema(
  gameMessageErrorsSchema,
  commandAckSchema,
);
export const notificationReadDispositionSchema = makeCommandDispositionSchema(
  notificationReadErrorsSchema,
  commandAckSchema,
);
export type GameMessageDisposition = z.infer<
  typeof gameMessageDispositionSchema
>;
export type NotificationReadDisposition = z.infer<
  typeof notificationReadDispositionSchema
>;

export * from "./profile-art.ts";
import { profileArtErrorsSchema, validProfileArtAck } from "./profile-art.ts";
export const profileArtDispositionSchema = makeCommandDispositionSchema(
  profileArtErrorsSchema,
  commandAckSchema,
);
export type ProfileArtDisposition = z.infer<typeof profileArtDispositionSchema>;

/** Apply the original caller context after decoding a profile command disposition. */
export function validProfileArtDisposition(
  value: ProfileArtDisposition,
  owner: string,
): boolean {
  return (
    identifierSchema.safeParse(owner).success &&
    (value.state !== "accepted" ||
      validProfileArtAck(value.response.data, owner))
  );
}

export * from "./match-history.ts";
export * from "./competitive-history.ts";
export * from "./room-controls.ts";
export * from "./ante.ts";
export * from "./supply-ballot.ts";

export * from "./player.ts";
export { parseStrictJson, type RawJsonResult } from "./raw-json.ts";

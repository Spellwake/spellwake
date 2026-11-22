/** Original-bearer application transport. Neither worker keys nor vendor bodies belong here. */
import { z } from "zod";
import { publicOrigin } from "../lib/public-config";
export { publicOrigin } from "../lib/public-config";
import {
  addAiDispositionSchema,
  bootstrapAckSchema,
  bootstrapDispositionSchema,
  bootstrapRequestSchema,
  configureRoomSchema,
  forceStartDispositionSchema,
  hostRoomConfigurationSchema,
  publicProfileArtSchema,
  readyRoomDispositionSchema,
  removeAiDispositionSchema,
  revisionRequestSchema,
  roomCommandDispositionSchema,
  roomCommandSchema,
  roomConfigurationAckSchema,
  roomConfigurationDispositionSchema,
  roomDetailSchema,
  tableProfilePageSchema,
  unreadyDispositionSchema,
  validHostRoomConfiguration,
  validRoomConfigurationAck,
  validRoomDetail,
  type ConfigureRoom,
  type RoomCommand,
} from "@spellwake/application-contracts";
import {
  accountSessionSchema,
  patreonLinkRequestSchema,
  patreonCompleteRequestSchema,
  patreonLinkStartSchema,
  patreonLinkStatusSchema,
  type PatreonLinkRequest,
  type PatreonCompleteRequest,
  apiResponseSchema,
  checkoutInputSchema,
  commandAckSchema,
  identifierSchema,
  inventoryPageSchema,
  purchaseSchema,
  registerAccountSchema,
  registeredAccountSchema,
  accountSummarySchema,
  deckLibraryPageSchema,
  cosmeticCatalogueSchema,
  purchasePageSchema,
  createDeckSchema,
  saveDeckSchema,
  deckDetailSchema,
  deckFormatPageSchema,
  type CreateDeck,
  type SaveDeck,
  type ChangeDeckSettings,
  changeDeckSettingsSchema,
  type ChangeProfileDefaults,
  changeProfileDefaultsSchema,
  profileDefaultsDispositionSchema,
  deckCommandSchema,
  deckCommandDispositionSchema,
  type DeckCommand,
  cardPageSchema,
  setPageSchema,
  printingPageSchema,
  catalogueQuerySchema,
  catalogueLookupSchema,
  catalogueLabelsSchema,
  type CatalogueLookup,
  namedPrintingQuerySchema,
  requestDeletionSchema,
  observeDeletionSchema,
  deletionObservationSchema,
  type RequestDeletion,
  type ObserveDeletion,
  socialCommandSchema,
  socialCommandDispositionSchema,
  type SocialCommand,
  socialPeoplePageSchema,
  socialStatusSchema,
  socialDirectoryQuerySchema,
  directMessagePageSchema,
  messageCursorSchema,
  messagePositionBefore,
  type MessageCursor,
  DIRECT_MESSAGE_PAGE_LIMIT,
  markDirectReadSchema,
  directReadDispositionSchema,
  type MarkDirectRead,
  packProductPageSchema,
  ownedPackQuoteSchema,
  packQuantitySchema,
  PACK_PRODUCT_PAGE_LIMIT,
  ownedPredictionPublicationSourceSchema,
  predictionSourcePreparationSchema,
  ownedPredictionMarketSchema,
  predictionMarketPageSchema,
  predictionOrderPageSchema,
  predictionPositionPageSchema,
  predictionCommandSchema,
  predictionCommandDispositionSchema,
  predictionAckMatches,
  type PredictionCommand,
  economyCommandSchema,
  economyCommandDispositionSchema,
  economyAckMatches,
  type EconomyCommand,
  ownedPackOutcomeSchema,
  ownedTradeSchema,
  ownedListingSchema,
  tradePageSchema,
  listingPageSchema,
  tradePageQuerySchema,
  marketPageQuerySchema,
  EXCHANGE_PAGE_LIMIT,
  type TradePageQuery,
  type MarketPageQuery,
} from "@spellwake/application-contracts";

import {
  importDeckAckSchema,
  importDeckDispositionSchema,
  importDeckSchema,
  ownProfileArtSchema,
  validOwnProfileArt,
  type ImportDeck,
  cardMetadataBatchSchema,
  richCardSearchPageSchema,
  duplicateDeckAckSchema,
  richCosmeticPageSchema,
  profileBadgeResultSchema,
  patreonAccountStatusSchema,
  billingPortalSessionSchema,
  leaderboardPageSchema,
  playerPublicSummarySchema,
  marketSearchPageSchema,
  tradeSessionEnvelopeSchema,
  tradeSessionListSchema,
  predictionSearchPageSchema,
  roomGameHandoffSchema,
  roomEnrichedDetailSchema,
  roomInviteNotificationAckSchema,
  limitedDraftStateSchema,
  limitedDraftAckSchema,
  dmAttachmentMessageSchema,
  openingChoiceResponseSchema,
  playerSnapshotSchema,
  playerCommandAckSchema,
  playerStreamFrameSchema,
  playerReconnectSchema,
  playerHistoryPageSchema,
  playerReplayFrameSchema,
  playerReceiptSchema,
  type PlayerCommand,
  type PlayerCursor,
  type PlayerStreamFrame,
} from "@spellwake/application-contracts";

export type ApiResponse<T> =
  | { status: "success"; data: T }
  | { status: "fail"; data: Record<string, string> }
  | {
      status: "error";
      message: string;
      code?: string | undefined;
      data?: unknown;
    };
const unavailable = {
  status: "error",
  message: "Spellwake is temporarily unavailable.",
  code: "unavailable",
} satisfies ApiResponse<never>;

/** Bound decoded response bytes, including when Content-Length is absent or misleading. */
async function boundedJson(response: Response): Promise<unknown> {
  const reader = response.body?.getReader();
  if (!reader) return null;
  const parts: Uint8Array[] = [];
  let size = 0;
  try {
    for (;;) {
      const next = await reader.read();
      if (next.done) break;
      size += next.value.byteLength;
      if (size > 1024 * 1024) {
        await reader.cancel();
        return null;
      }
      parts.push(next.value);
    }
    const bytes = new Uint8Array(size);
    let offset = 0;
    for (const part of parts) {
      bytes.set(part, offset);
      offset += part.byteLength;
    }
    return JSON.parse(new TextDecoder("utf-8", { fatal: true }).decode(bytes));
  } finally {
    reader.releaseLock();
  }
}

/** The room actions that name a room and the revision they were decided against. */
export type RoomControl = "ready" | "unready" | "force-start" | "ai";
/** What became of a room action: accepted with the new revision, refused, or not known. */
export type RoomControlDisposition =
  | z.infer<typeof readyRoomDispositionSchema>
  | z.infer<typeof unreadyDispositionSchema>
  | z.infer<typeof forceStartDispositionSchema>
  | z.infer<typeof addAiDispositionSchema>;
const ROOM_CONTROL_DISPOSITIONS: Record<
  RoomControl,
  z.ZodType<RoomControlDisposition>
> = {
  ready: readyRoomDispositionSchema as z.ZodType<RoomControlDisposition>,
  unready: unreadyDispositionSchema as z.ZodType<RoomControlDisposition>,
  "force-start":
    forceStartDispositionSchema as z.ZodType<RoomControlDisposition>,
  ai: addAiDispositionSchema as z.ZodType<RoomControlDisposition>,
};

export class ApplicationApi {
  private constructor(
    readonly origin: string,
    private readonly transport: (
      input: RequestInfo | URL,
      init?: RequestInit,
    ) => Promise<Response>,
  ) {}

  /** Invalid origins never construct a credential-bearing transport. */
  static create(
    origin: string,
    transport: (
      input: RequestInfo | URL,
      init?: RequestInit,
    ) => Promise<Response> = (input, init) => fetch(input, init),
  ): ApplicationApi | null {
    const validated = publicOrigin(origin);
    return validated ? new ApplicationApi(validated, transport) : null;
  }

  private async request<T>(
    path: string,
    bearer: string | null,
    schema: z.ZodType<T>,
    input?: unknown,
    signal?: AbortSignal,
    methodOverride?: "GET" | "POST" | "PATCH" | "DELETE",
  ): Promise<ApiResponse<T>> {
    if (
      bearer !== null &&
      (bearer.length < 1 || bearer.length > 16384 || /\s/.test(bearer))
    )
      return { status: "fail", data: { session: "unauthorized" } };
    const method = methodOverride ?? (input === undefined ? "GET" : "POST");
    try {
      const response = await this.transport(this.origin + path, {
        method,
        headers: {
          ...(bearer === null ? {} : { Authorization: `Bearer ${bearer}` }),
          ...(input === undefined
            ? {}
            : { "Content-Type": "application/json" }),
        },
        ...(input === undefined ? {} : { body: JSON.stringify(input) }),
        signal: signal
          ? AbortSignal.any([signal, AbortSignal.timeout(15000)])
          : AbortSignal.timeout(15000),
        credentials: "omit",
        redirect: "error",
        cache: "no-store",
        referrerPolicy: "no-referrer",
      });
      const result = apiResponseSchema(schema).safeParse(
        await boundedJson(response),
      );
      if (!result.success || (result.data.status === "success" && !response.ok))
        return unavailable;
      return result.data;
    } catch {
      // Do not log a fetch/parse exception: it may contain the bearer or body.
      if (!signal?.aborted)
        console.warn("application_api code=request_unavailable");
      return unavailable;
    }
  }

  session(bearer: string, signal?: AbortSignal) {
    return this.request(
      "/v1/session",
      bearer,
      accountSessionSchema,
      undefined,
      signal,
    );
  }
  requestDeletion(
    bearer: string,
    input: RequestDeletion,
    signal?: AbortSignal,
  ) {
    const parsed = requestDeletionSchema.safeParse(input);
    return parsed.success
      ? this.request(
          "/v1/account/deletion",
          bearer,
          commandAckSchema,
          parsed.data,
          signal,
        )
      : Promise.resolve<ApiResponse<z.infer<typeof commandAckSchema>>>({
          status: "fail",
          data: { request: "invalid" },
        });
  }
  /** The capability is sent only in the bounded POST body, with no account bearer. */
  async observeDeletion(
    input: ObserveDeletion,
    signal?: AbortSignal,
  ): Promise<ApiResponse<z.infer<typeof deletionObservationSchema>>> {
    const parsed = observeDeletionSchema.safeParse(input);
    if (!parsed.success)
      return { status: "fail", data: { observer: "unavailable" } };
    const result = await this.request(
      "/v1/account/deletion/observe",
      null,
      deletionObservationSchema,
      parsed.data,
      signal,
    );
    if (
      result.status === "success" &&
      (result.data.accountId !== parsed.data.accountId ||
        result.data.requestId !== parsed.data.requestId)
    )
      return unavailable;
    return result;
  }
  account(bearer: string, signal?: AbortSignal) {
    return this.request(
      "/v1/account",
      bearer,
      accountSummarySchema,
      undefined,
      signal,
    );
  }
  async decks(
    bearer: string,
    after: string | null = null,
    signal?: AbortSignal,
  ): Promise<ApiResponse<z.infer<typeof deckLibraryPageSchema>>> {
    if (after !== null && !identifierSchema.safeParse(after).success)
      return { status: "fail", data: { query: "invalid" } };
    const result = await this.request(
      `/v1/decks?limit=100${after ? `&after=${after}` : ""}`,
      bearer,
      deckLibraryPageSchema,
      undefined,
      signal,
    );
    if (result.status === "success") {
      const expected =
        result.data.items.length === 100 ? result.data.items.at(-1)?.id : null;
      if (
        result.data.nextCursor !== expected ||
        (after && result.data.items.some((item) => item.id <= after))
      )
        return unavailable;
    }
    return result;
  }
  register(bearer: string, username: string) {
    const parsed = registerAccountSchema.safeParse({ username });
    return parsed.success
      ? this.request(
          "/v1/account",
          bearer,
          registeredAccountSchema,
          parsed.data,
        )
      : Promise.resolve<ApiResponse<z.infer<typeof registeredAccountSchema>>>({
          status: "fail",
          data: { username: "invalid" },
        });
  }
  cosmetics(bearer: string, after: string | null = null, signal?: AbortSignal) {
    return this.storePage(
      "/v1/cosmetics",
      bearer,
      cosmeticCatalogueSchema,
      (item) => item.id,
      after,
      signal,
    );
  }
  purchases(bearer: string, after: string | null = null, signal?: AbortSignal) {
    return this.storePage(
      "/v1/purchases",
      bearer,
      purchasePageSchema,
      (item) => item.purchaseId,
      after,
      signal,
    );
  }
  private async storePage<
    Page extends { items: unknown[]; nextCursor: string | null },
  >(
    path: string,
    bearer: string,
    schema: z.ZodType<Page>,
    id: (item: Page["items"][number]) => string,
    after: string | null,
    signal?: AbortSignal,
    query?: URLSearchParams,
    limit = 100,
  ) {
    if (
      !Number.isInteger(limit) ||
      limit < 1 ||
      limit > 100 ||
      (after !== null && !identifierSchema.safeParse(after).success)
    )
      return {
        status: "fail",
        data: { query: "invalid" },
      } satisfies ApiResponse<never>;
    const parameters = new URLSearchParams(query);
    parameters.set("limit", String(limit));
    if (after) parameters.set("after", after);
    const result = await this.request(
      `${path}?${parameters}`,
      bearer,
      schema,
      undefined,
      signal,
    );
    if (result.status === "success") {
      const last = result.data.items.at(-1);
      const expected =
        result.data.items.length === limit && last ? id(last) : null;
      if (
        result.data.items.length > limit ||
        result.data.nextCursor !== expected ||
        (after && result.data.items.some((item) => id(item) <= after))
      )
        return unavailable;
    }
    return result;
  }
  cards(
    bearer: string,
    query: string,
    exact = false,
    after: string | null = null,
    signal?: AbortSignal,
  ) {
    const parsed = catalogueQuerySchema.safeParse({ query, exact });
    if (!parsed.success)
      return Promise.resolve({
        status: "fail",
        data: { query: "invalid" },
      } satisfies ApiResponse<never>);
    return this.storePage(
      "/v1/cards",
      bearer,
      cardPageSchema,
      (card) => card.id,
      after,
      signal,
      new URLSearchParams({
        query: parsed.data.query,
        exact: String(parsed.data.exact),
      }),
    );
  }
  async printings(
    bearer: string,
    definitionId: string,
    after: string | null = null,
    signal?: AbortSignal,
  ) {
    if (!identifierSchema.safeParse(definitionId).success)
      return {
        status: "fail",
        data: { definition: "invalid" },
      } satisfies ApiResponse<never>;
    const result = await this.storePage(
      `/v1/cards/${definitionId}/printings`,
      bearer,
      printingPageSchema,
      (printing) => printing.id,
      after,
      signal,
    );
    if (
      result.status === "success" &&
      result.data.items.some(
        (printing) => printing.definitionId !== definitionId,
      )
    )
      return unavailable;
    return result;
  }
  async catalogue(
    bearer: string,
    input: CatalogueLookup,
    signal?: AbortSignal,
  ) {
    const parsed = catalogueLookupSchema.safeParse(input);
    if (!parsed.success)
      return {
        status: "fail",
        data: { query: "invalid" },
      } satisfies ApiResponse<never>;
    const result = await this.request(
      "/v1/catalogue/resolve",
      bearer,
      catalogueLabelsSchema,
      parsed.data,
      signal,
    );
    if (
      result.status === "success" &&
      (result.data.cards.some(
        (card) => !parsed.data.definitionIds.includes(card.id),
      ) ||
        result.data.printings.some(
          (printing) => !parsed.data.printingIds.includes(printing.id),
        ))
    )
      return unavailable;
    return result;
  }
  namedPrintings(
    bearer: string,
    name: string,
    hints: { setCode?: string; collectorNumber?: string } = {},
    after: string | null = null,
    signal?: AbortSignal,
  ) {
    const parsed = namedPrintingQuerySchema.safeParse({
      name,
      setCode: hints.setCode ?? null,
      collectorNumber: hints.collectorNumber ?? null,
    });
    if (!parsed.success)
      return Promise.resolve({
        status: "fail",
        data: { query: "invalid" },
      } satisfies ApiResponse<never>);
    const parameters = new URLSearchParams({ name: parsed.data.name });
    if (parsed.data.setCode !== null)
      parameters.set("setCode", parsed.data.setCode);
    if (parsed.data.collectorNumber !== null)
      parameters.set("collectorNumber", parsed.data.collectorNumber);
    return this.storePage(
      "/v1/cards/printings",
      bearer,
      printingPageSchema,
      (printing) => printing.id,
      after,
      signal,
      parameters,
    );
  }
  /** Discover published set labels, including sets with no qualified purchasable product. */
  sets(
    bearer: string,
    query = "",
    after: string | null = null,
    signal?: AbortSignal,
  ) {
    const parsed = catalogueQuerySchema.shape.query.safeParse(query);
    if (!parsed.success)
      return Promise.resolve<ApiResponse<z.infer<typeof setPageSchema>>>({
        status: "fail",
        data: { query: "invalid" },
      });
    return this.storePage(
      "/v1/sets",
      bearer,
      setPageSchema,
      (item) => item.id,
      after,
      signal,
      new URLSearchParams({ query: parsed.data }),
      40,
    );
  }
  async inventory(
    bearer: string,
    cursor: string | null = null,
    signal?: AbortSignal,
  ): Promise<ApiResponse<z.infer<typeof inventoryPageSchema>>> {
    if (cursor !== null && !identifierSchema.safeParse(cursor).success)
      return { status: "fail", data: { cursor: "invalid" } };
    const result = await this.request(
      `/v1/inventory?limit=100${cursor ? `&after=${cursor}` : ""}`,
      bearer,
      inventoryPageSchema,
      undefined,
      signal,
    );
    if (result.status === "success") {
      const expected =
        result.data.items.length === 100 ? result.data.items.at(-1)?.id : null;
      if (
        result.data.nextCursor !== expected ||
        (cursor && result.data.items.some((item) => item.id <= cursor))
      )
        return unavailable;
    }
    return result;
  }
  checkout(bearer: string, requestId: string, cosmeticId: string) {
    const parsed = checkoutInputSchema.safeParse({ requestId, cosmeticId });
    return parsed.success
      ? this.request("/v1/purchases", bearer, commandAckSchema, parsed.data)
      : Promise.resolve<ApiResponse<z.infer<typeof commandAckSchema>>>({
          status: "fail",
          data: { request: "invalid" },
        });
  }
  /** Creates one deck with its resolved cards, once for each request ID. */
  importDeck(bearer: string, input: ImportDeck) {
    const parsed = importDeckSchema.safeParse(input);
    return parsed.success
      ? this.request(
          "/v1/decks/import",
          bearer,
          importDeckAckSchema,
          parsed.data,
        )
      : Promise.resolve<ApiResponse<z.infer<typeof importDeckAckSchema>>>({
          status: "fail",
          data: { import: "invalid" },
        });
  }
  /** What became of an import, by its original body. */
  importDeckDisposition(bearer: string, input: ImportDeck) {
    const parsed = importDeckSchema.safeParse(input);
    return parsed.success
      ? this.request(
          "/v1/decks/import/disposition",
          bearer,
          importDeckDispositionSchema,
          parsed.data,
        )
      : Promise.resolve<
          ApiResponse<z.infer<typeof importDeckDispositionSchema>>
        >({ status: "fail", data: { import: "invalid" } });
  }
  /** A room this person is in, with its seats. Read whole: a room holds at most six. */
  roomDetail(
    bearer: string,
    expectedAccountId: string,
    roomId: string,
    signal?: AbortSignal,
  ) {
    if (
      !identifierSchema.safeParse(expectedAccountId).success ||
      !identifierSchema.safeParse(roomId).success
    )
      return Promise.resolve<ApiResponse<z.infer<typeof roomDetailSchema>>>({
        status: "fail",
        data: { room: "unavailable" },
      });
    return this.request(
      `/v1/rooms/${encodeURIComponent(roomId)}?slotLimit=100`,
      bearer,
      roomDetailSchema.refine((detail) =>
        validRoomDetail(detail, expectedAccountId, roomId, null, 100),
      ),
      undefined,
      signal,
    );
  }
  /** Creates a room, chooses a deck in one, or leaves one, once per request ID. */
  roomCommand(bearer: string, input: RoomCommand) {
    const parsed = roomCommandSchema.safeParse(input);
    return parsed.success
      ? this.request(
          "/v1/rooms/commands",
          bearer,
          commandAckSchema,
          parsed.data,
        )
      : Promise.resolve<ApiResponse<z.infer<typeof commandAckSchema>>>({
          status: "fail",
          data: { request: "invalid" },
        });
  }
  roomCommandDisposition(bearer: string, input: RoomCommand) {
    const parsed = roomCommandSchema.safeParse(input);
    return parsed.success
      ? this.request(
          "/v1/rooms/commands/disposition",
          bearer,
          roomCommandDispositionSchema,
          parsed.data,
        )
      : Promise.resolve<
          ApiResponse<z.infer<typeof roomCommandDispositionSchema>>
        >({ status: "fail", data: { request: "invalid" } });
  }
  /**
   * Ready, Not ready, force start or add an AI seat: each names the room and
   * the revision it was decided against, and answers with the new revision.
   */
  roomControl(
    bearer: string,
    roomId: string,
    action: RoomControl,
    input: { requestId: string; expectedRevision: string },
  ) {
    const parsed = revisionRequestSchema.safeParse(input);
    return parsed.success && identifierSchema.safeParse(roomId).success
      ? this.request(
          `/v1/rooms/${encodeURIComponent(roomId)}/${action}`,
          bearer,
          roomConfigurationAckSchema.refine((ack) => ack.resourceId === roomId),
          parsed.data,
        )
      : Promise.resolve<
          ApiResponse<z.infer<typeof roomConfigurationAckSchema>>
        >({ status: "fail", data: { request: "invalid" } });
  }
  roomControlDisposition(
    bearer: string,
    roomId: string,
    action: RoomControl,
    input: { requestId: string; expectedRevision: string },
  ) {
    const parsed = revisionRequestSchema.safeParse(input);
    const schema = ROOM_CONTROL_DISPOSITIONS[action];
    return parsed.success && identifierSchema.safeParse(roomId).success
      ? this.request(
          `/v1/rooms/${encodeURIComponent(roomId)}/${action}/disposition`,
          bearer,
          schema,
          parsed.data,
        )
      : Promise.resolve<ApiResponse<RoomControlDisposition>>({
          status: "fail",
          data: { request: "invalid" },
        });
  }
  /** Prepares a new AI seat with a deck, or Random Deck when null. */
  bootstrapAi(
    bearer: string,
    slotId: string,
    input: z.infer<typeof bootstrapRequestSchema>,
  ) {
    const parsed = bootstrapRequestSchema.safeParse(input);
    return parsed.success && identifierSchema.safeParse(slotId).success
      ? this.request(
          `/v1/ai-slots/${encodeURIComponent(slotId)}/bootstrap`,
          bearer,
          bootstrapAckSchema,
          parsed.data,
        )
      : Promise.resolve<ApiResponse<z.infer<typeof bootstrapAckSchema>>>({
          status: "fail",
          data: { request: "invalid" },
        });
  }
  bootstrapAiDisposition(
    bearer: string,
    slotId: string,
    input: z.infer<typeof bootstrapRequestSchema>,
  ) {
    const parsed = bootstrapRequestSchema.safeParse(input);
    return parsed.success && identifierSchema.safeParse(slotId).success
      ? this.request(
          `/v1/ai-slots/${encodeURIComponent(slotId)}/bootstrap/disposition`,
          bearer,
          bootstrapDispositionSchema,
          parsed.data,
        )
      : Promise.resolve<
          ApiResponse<z.infer<typeof bootstrapDispositionSchema>>
        >({ status: "fail", data: { request: "invalid" } });
  }
  /** Removes an AI seat, leaving its place open for a person. */
  removeAi(
    bearer: string,
    slotId: string,
    input: { requestId: string; expectedRevision: string },
  ) {
    const parsed = revisionRequestSchema.safeParse(input);
    return parsed.success && identifierSchema.safeParse(slotId).success
      ? this.request(
          `/v1/ai-slots/${encodeURIComponent(slotId)}/remove`,
          bearer,
          z.strictObject({ resourceId: identifierSchema }),
          parsed.data,
        )
      : Promise.resolve<ApiResponse<{ resourceId: string }>>({
          status: "fail",
          data: { request: "invalid" },
        });
  }
  removeAiDisposition(
    bearer: string,
    slotId: string,
    input: { requestId: string; expectedRevision: string },
  ) {
    const parsed = revisionRequestSchema.safeParse(input);
    return parsed.success && identifierSchema.safeParse(slotId).success
      ? this.request(
          `/v1/ai-slots/${encodeURIComponent(slotId)}/remove/disposition`,
          bearer,
          removeAiDispositionSchema,
          parsed.data,
        )
      : Promise.resolve<ApiResponse<z.infer<typeof removeAiDispositionSchema>>>(
          { status: "fail", data: { request: "invalid" } },
        );
  }
  /** The table rules a room may use: Free-for-All and the like, with their seat ranges. */
  tableProfiles(
    bearer: string,
    expectedAccountId: string,
    signal?: AbortSignal,
  ) {
    return this.request(
      "/v1/table-profiles?limit=100",
      bearer,
      tableProfilePageSchema.refine(
        (page) => page.accountId === expectedAccountId,
      ),
      undefined,
      signal,
    );
  }
  /** The host's view of a room's table rules, match length and teams. */
  roomConfiguration(
    bearer: string,
    expectedAccountId: string,
    roomId: string,
    signal?: AbortSignal,
  ) {
    return this.request(
      `/v1/rooms/${encodeURIComponent(roomId)}/configuration`,
      bearer,
      hostRoomConfigurationSchema.refine((value) =>
        validHostRoomConfiguration(value, expectedAccountId, roomId),
      ),
      undefined,
      signal,
    );
  }
  configureRoom(bearer: string, roomId: string, input: ConfigureRoom) {
    const parsed = configureRoomSchema.safeParse(input);
    return parsed.success && identifierSchema.safeParse(roomId).success
      ? this.request(
          `/v1/rooms/${encodeURIComponent(roomId)}/configuration`,
          bearer,
          roomConfigurationAckSchema.refine((ack) =>
            validRoomConfigurationAck(parsed.data, roomId, ack),
          ),
          parsed.data,
        )
      : Promise.resolve<
          ApiResponse<z.infer<typeof roomConfigurationAckSchema>>
        >({ status: "fail", data: { request: "invalid" } });
  }
  configureRoomDisposition(
    bearer: string,
    roomId: string,
    input: ConfigureRoom,
  ) {
    const parsed = configureRoomSchema.safeParse(input);
    return parsed.success && identifierSchema.safeParse(roomId).success
      ? this.request(
          `/v1/rooms/${encodeURIComponent(roomId)}/configuration/disposition`,
          bearer,
          roomConfigurationDispositionSchema,
          parsed.data,
        )
      : Promise.resolve<
          ApiResponse<z.infer<typeof roomConfigurationDispositionSchema>>
        >({ status: "fail", data: { request: "invalid" } });
  }
  /** Another player's public name, for a lobby seat. */
  playerProfile(
    bearer: string,
    expectedAccountId: string,
    playerId: string,
    signal?: AbortSignal,
  ) {
    if (!identifierSchema.safeParse(playerId).success)
      return Promise.resolve<
        ApiResponse<z.infer<typeof publicProfileArtSchema>>
      >({ status: "fail", data: { player: "unavailable" } });
    return this.request(
      `/v1/players/${encodeURIComponent(playerId)}/profile`,
      bearer,
      publicProfileArtSchema.refine(
        (profile) =>
          profile.viewerAccountId === expectedAccountId &&
          profile.person.id === playerId,
      ),
      undefined,
      signal,
    );
  }
  createDeck(bearer: string, input: CreateDeck) {
    const parsed = createDeckSchema.safeParse(input);
    return parsed.success
      ? this.request("/v1/decks", bearer, commandAckSchema, parsed.data)
      : Promise.resolve<ApiResponse<z.infer<typeof commandAckSchema>>>({
          status: "fail",
          data: { deck: "invalid" },
        });
  }
  async deckCommandDisposition(bearer: string, input: DeckCommand) {
    const parsed = deckCommandSchema.safeParse(input);
    if (!parsed.success)
      return {
        status: "fail",
        data: { request: "invalid" },
      } satisfies ApiResponse<never>;
    const result = await this.request(
      "/v1/decks/commands/disposition",
      bearer,
      deckCommandDispositionSchema,
      parsed.data,
    );
    if (
      parsed.data.kind === "settings" &&
      result.status === "success" &&
      result.data.state === "accepted" &&
      result.data.response.data.resourceId !== parsed.data.command.deckId
    )
      return unavailable;
    return result;
  }
  saveDeck(bearer: string, input: SaveDeck) {
    const parsed = saveDeckSchema.safeParse(input);
    return parsed.success
      ? this.request(
          `/v1/decks/${parsed.data.deckId}/revisions`,
          bearer,
          commandAckSchema,
          parsed.data,
        )
      : Promise.resolve<ApiResponse<z.infer<typeof commandAckSchema>>>({
          status: "fail",
          data: { deck: "invalid" },
        });
  }
  async changeDeckSettings(bearer: string, input: ChangeDeckSettings) {
    const parsed = changeDeckSettingsSchema.safeParse(input);
    if (!parsed.success)
      return {
        status: "fail",
        data: { deck: "invalid" },
      } satisfies ApiResponse<never>;
    const result = await this.request(
      `/v1/decks/${parsed.data.deckId}/settings`,
      bearer,
      commandAckSchema,
      parsed.data,
    );
    return result.status === "success" &&
      result.data.resourceId !== parsed.data.deckId
      ? unavailable
      : result;
  }
  changeProfileDefaults(bearer: string, input: ChangeProfileDefaults) {
    const parsed = changeProfileDefaultsSchema.safeParse(input);
    return parsed.success
      ? this.request(
          "/v1/account/defaults",
          bearer,
          commandAckSchema,
          parsed.data,
        )
      : Promise.resolve<ApiResponse<z.infer<typeof commandAckSchema>>>({
          status: "fail",
          data: { defaults: "invalid" },
        });
  }
  profileDefaultsDisposition(bearer: string, input: ChangeProfileDefaults) {
    const parsed = changeProfileDefaultsSchema.safeParse(input);
    return parsed.success
      ? this.request(
          "/v1/account/defaults/disposition",
          bearer,
          profileDefaultsDispositionSchema,
          parsed.data,
        )
      : Promise.resolve<
          ApiResponse<z.infer<typeof profileDefaultsDispositionSchema>>
        >({ status: "fail", data: { request: "invalid" } });
  }
  /** Read an exact participant offer; the expected owner stays local. */
  trade(
    bearer: string,
    expectedAccountId: string,
    tradeId: string,
    signal?: AbortSignal,
  ) {
    if (
      !identifierSchema.safeParse(expectedAccountId).success ||
      !identifierSchema.safeParse(tradeId).success
    )
      return Promise.resolve<ApiResponse<z.infer<typeof ownedTradeSchema>>>({
        status: "fail",
        data: { trade: "unavailable" },
      });
    return this.request(
      `/v1/trades/${encodeURIComponent(tradeId)}`,
      bearer,
      ownedTradeSchema.refine(
        (value) =>
          value.accountId === expectedAccountId && value.trade.id === tradeId,
      ),
      undefined,
      signal,
    );
  }
  /** Read a public active listing or caller-participated history. */
  marketListing(
    bearer: string,
    expectedAccountId: string,
    listingId: string,
    signal?: AbortSignal,
  ) {
    if (
      !identifierSchema.safeParse(expectedAccountId).success ||
      !identifierSchema.safeParse(listingId).success
    )
      return Promise.resolve<ApiResponse<z.infer<typeof ownedListingSchema>>>({
        status: "fail",
        data: { listing: "unavailable" },
      });
    return this.request(
      `/v1/market/listings/${encodeURIComponent(listingId)}`,
      bearer,
      ownedListingSchema.refine(
        (value) =>
          value.accountId === expectedAccountId &&
          value.listing.id === listingId,
      ),
      undefined,
      signal,
    );
  }
  /** Read filtered caller history with the established UUID cursor. */
  trades(
    bearer: string,
    expectedAccountId: string,
    query: TradePageQuery = {},
    signal?: AbortSignal,
  ) {
    const parsed = tradePageQuerySchema.safeParse(query);
    if (
      !identifierSchema.safeParse(expectedAccountId).success ||
      !parsed.success
    )
      return Promise.resolve<ApiResponse<z.infer<typeof tradePageSchema>>>({
        status: "fail",
        data: { query: "invalid" },
      });
    const parameters = new URLSearchParams();
    if (parsed.data.state) parameters.set("state", parsed.data.state);
    return this.storePage(
      "/v1/trades",
      bearer,
      tradePageSchema.refine(
        (page) =>
          page.accountId === expectedAccountId &&
          page.items.every(
            (item) => !parsed.data.state || item.state === parsed.data.state,
          ),
      ),
      (item) => item.id,
      parsed.data.after ?? null,
      signal,
      parameters,
      parsed.data.limit ?? EXCHANGE_PAGE_LIMIT,
    );
  }
  /** Read listed lots or caller history with exact bounded paging. */
  marketListings(
    bearer: string,
    expectedAccountId: string,
    query: MarketPageQuery = {},
    signal?: AbortSignal,
  ) {
    const parsed = marketPageQuerySchema.safeParse(query);
    if (
      !identifierSchema.safeParse(expectedAccountId).success ||
      !parsed.success
    )
      return Promise.resolve<ApiResponse<z.infer<typeof listingPageSchema>>>({
        status: "fail",
        data: { query: "invalid" },
      });
    const owned = parsed.data.owned ?? false;
    const parameters = new URLSearchParams({ owned: String(owned) });
    return this.storePage(
      "/v1/market/listings",
      bearer,
      listingPageSchema.refine(
        (page) =>
          page.accountId === expectedAccountId &&
          page.items.every((item) =>
            owned ? item.participant !== null : item.state === "listed",
          ),
      ),
      (item) => item.id,
      parsed.data.after ?? null,
      signal,
      parameters,
      parsed.data.limit ?? EXCHANGE_PAGE_LIMIT,
    );
  }
  /** Browse only qualified, executable products with published pricing. */
  packProducts(
    bearer: string,
    expectedAccountId: string,
    after: string | null = null,
    signal?: AbortSignal,
  ) {
    if (!identifierSchema.safeParse(expectedAccountId).success)
      return Promise.resolve<
        ApiResponse<z.infer<typeof packProductPageSchema>>
      >({ status: "fail", data: { account: "unavailable" } });
    return this.storePage(
      "/v1/packs/products",
      bearer,
      packProductPageSchema.refine(
        (page) => page.accountId === expectedAccountId,
      ),
      (item) => item.id,
      after,
      signal,
      new URLSearchParams(),
      PACK_PRODUCT_PAGE_LIMIT,
    );
  }
  /** Observe exact current terms; the purchase command still checks its policy/revision/total. */
  packQuote(
    bearer: string,
    expectedAccountId: string,
    recipeId: string,
    quantity = "1",
    signal?: AbortSignal,
  ) {
    if (
      ![expectedAccountId, recipeId].every(
        (id) => identifierSchema.safeParse(id).success,
      ) ||
      !packQuantitySchema.safeParse(quantity).success
    )
      return Promise.resolve<ApiResponse<z.infer<typeof ownedPackQuoteSchema>>>(
        { status: "fail", data: { quote: "invalid" } },
      );
    return this.request(
      `/v1/packs/products/${encodeURIComponent(recipeId)}/quote?quantity=${quantity}`,
      bearer,
      ownedPackQuoteSchema.refine(
        (value) =>
          value.accountId === expectedAccountId &&
          value.quote.recipeId === recipeId &&
          value.quote.quantity === quantity,
      ),
      undefined,
      signal,
    );
  }
  packOutcome(
    bearer: string,
    expectedAccountId: string,
    opened: string,
    signal?: AbortSignal,
  ) {
    if (
      !identifierSchema.safeParse(expectedAccountId).success ||
      !identifierSchema.safeParse(opened).success
    )
      return Promise.resolve<
        ApiResponse<z.infer<typeof ownedPackOutcomeSchema>>
      >({ status: "fail", data: { pack: "unavailable" } });
    return this.request(
      `/v1/packs/openings/${encodeURIComponent(opened)}`,
      bearer,
      ownedPackOutcomeSchema.refine(
        (value) =>
          value.accountId === expectedAccountId && value.outcome.id === opened,
      ),
      undefined,
      signal,
    );
  }
  /**
   * The caller's own profile art and supporter entitlement. A response for any
   * other account is treated as unavailable, never as this account's.
   */
  async ownProfile(
    bearer: string,
    expectedAccountId: string,
    signal?: AbortSignal,
  ): Promise<ApiResponse<z.infer<typeof ownProfileArtSchema>>> {
    const result = await this.request(
      "/v1/account/profile",
      bearer,
      ownProfileArtSchema,
      undefined,
      signal,
    );
    if (
      result.status === "success" &&
      !validOwnProfileArt(result.data, expectedAccountId)
    )
      return unavailable;
    return result;
  }
  /** Issue a private, same-session Patreon benefit association; caller owns opening the returned URL. */
  patreonStart(
    bearer: string,
    expectedAccountId: string,
    input: PatreonLinkRequest,
    signal?: AbortSignal,
  ) {
    const parsed = patreonLinkRequestSchema.safeParse(input);
    if (
      !identifierSchema.safeParse(expectedAccountId).success ||
      !parsed.success
    )
      return Promise.resolve<
        ApiResponse<z.infer<typeof patreonLinkStartSchema>>
      >({ status: "fail", data: { request: "invalid" } });
    return this.request(
      "/v1/account/patreon/start",
      bearer,
      patreonLinkStartSchema.refine(
        (value) =>
          value.accountId === expectedAccountId &&
          value.requestId === parsed.data.requestId &&
          patreonAuthorizationState(
            value.authorizationUrl,
            parsed.data.stateKey,
          ),
      ),
      parsed.data,
      signal,
    );
  }
  /** Complete exactly this callback; repeated/spent claims never authorize another code exchange. */
  patreonComplete(
    bearer: string,
    expectedAccountId: string,
    input: PatreonCompleteRequest,
    signal?: AbortSignal,
  ) {
    const parsed = patreonCompleteRequestSchema.safeParse(input);
    if (
      !identifierSchema.safeParse(expectedAccountId).success ||
      !parsed.success
    )
      return Promise.resolve<
        ApiResponse<z.infer<typeof patreonLinkStatusSchema>>
      >({ status: "fail", data: { request: "invalid" } });
    return this.request(
      "/v1/account/patreon/complete",
      bearer,
      patreonLinkStatusSchema.refine(
        (value) =>
          value.accountId === expectedAccountId &&
          value.requestId === parsed.data.requestId,
      ),
      parsed.data,
      signal,
    );
  }
  /** Explicit flow observation, available even when provider credentials have been removed. */
  patreonStatus(
    bearer: string,
    expectedAccountId: string,
    input: PatreonLinkRequest,
    signal?: AbortSignal,
  ) {
    const parsed = patreonLinkRequestSchema.safeParse(input);
    if (
      !identifierSchema.safeParse(expectedAccountId).success ||
      !parsed.success
    )
      return Promise.resolve<
        ApiResponse<z.infer<typeof patreonLinkStatusSchema>>
      >({ status: "fail", data: { request: "invalid" } });
    return this.request(
      "/v1/account/patreon/status",
      bearer,
      patreonLinkStatusSchema.refine(
        (value) =>
          value.accountId === expectedAccountId &&
          value.requestId === parsed.data.requestId,
      ),
      parsed.data,
      signal,
    );
  }
  /** Explicitly prepare retained-source evidence; GET preflight remains necessary afterward. */
  preparePredictionSource(
    bearer: string,
    expectedAccountId: string,
    printingId: string,
    signal?: AbortSignal,
  ) {
    if (
      ![expectedAccountId, printingId].every(
        (id) => identifierSchema.safeParse(id).success,
      )
    )
      return Promise.resolve<
        ApiResponse<z.infer<typeof predictionSourcePreparationSchema>>
      >({
        status: "fail",
        data: { printing: "unavailable" },
      });
    return this.request(
      `/v1/predictions/printings/${printingId}/source`,
      bearer,
      predictionSourcePreparationSchema.refine(
        (value) =>
          value.accountId === expectedAccountId &&
          value.printingId === printingId,
      ),
      {},
      signal,
    );
  }
  /** Read currently usable exact-printing evidence; never creates or fetches a source probe. */
  predictionPublicationSource(
    bearer: string,
    expectedAccountId: string,
    printingId: string,
    signal?: AbortSignal,
  ) {
    if (
      ![expectedAccountId, printingId].every(
        (id) => identifierSchema.safeParse(id).success,
      )
    )
      return Promise.resolve<
        ApiResponse<z.infer<typeof ownedPredictionPublicationSourceSchema>>
      >({ status: "fail", data: { printing: "unavailable" } });
    return this.request(
      `/v1/predictions/printings/${printingId}/source`,
      bearer,
      ownedPredictionPublicationSourceSchema.refine(
        (value) =>
          value.accountId === expectedAccountId &&
          value.source.printingId === printingId,
      ),
      undefined,
      signal,
    );
  }
  /** Anonymous public terms/depth scoped to the original caller, never an actor-selected read. */
  predictionMarket(
    bearer: string,
    expectedAccountId: string,
    marketId: string,
    signal?: AbortSignal,
  ) {
    if (
      ![expectedAccountId, marketId].every(
        (id) => identifierSchema.safeParse(id).success,
      )
    )
      return Promise.resolve<
        ApiResponse<z.infer<typeof ownedPredictionMarketSchema>>
      >({ status: "fail", data: { market: "unavailable" } });
    return this.request(
      `/v1/predictions/markets/${marketId}`,
      bearer,
      ownedPredictionMarketSchema.refine(
        (value) =>
          value.accountId === expectedAccountId &&
          value.market.terms.id === marketId,
      ),
      undefined,
      signal,
    );
  }
  /** Browse exact market terms without embedding private orders or positions. */
  predictionMarkets(
    bearer: string,
    expectedAccountId: string,
    after: string | null = null,
    signal?: AbortSignal,
  ) {
    if (!identifierSchema.safeParse(expectedAccountId).success)
      return Promise.resolve<
        ApiResponse<z.infer<typeof predictionMarketPageSchema>>
      >({ status: "fail", data: { account: "unavailable" } });
    return this.storePage(
      "/v1/predictions/markets",
      bearer,
      predictionMarketPageSchema.refine(
        (value) => value.accountId === expectedAccountId,
      ),
      (item) => item.id,
      after,
      signal,
      new URLSearchParams(),
      50,
    );
  }
  /** Read caller-owned orders only; an optional market filter cannot select another owner. */
  predictionOrders(
    bearer: string,
    expectedAccountId: string,
    marketId: string | null = null,
    after: string | null = null,
    signal?: AbortSignal,
  ) {
    if (
      !identifierSchema.safeParse(expectedAccountId).success ||
      (marketId !== null && !identifierSchema.safeParse(marketId).success)
    )
      return Promise.resolve<
        ApiResponse<z.infer<typeof predictionOrderPageSchema>>
      >({ status: "fail", data: { query: "invalid" } });
    return this.storePage(
      "/v1/predictions/orders",
      bearer,
      predictionOrderPageSchema.refine(
        (value) =>
          value.accountId === expectedAccountId &&
          value.items.every(
            (item) => marketId === null || item.marketId === marketId,
          ),
      ),
      (item) => item.id,
      after,
      signal,
      new URLSearchParams(marketId ? { marketId } : {}),
      50,
    );
  }
  /** Read immutable filled positions; this method cannot sell or cancel a filled unit. */
  predictionPositions(
    bearer: string,
    expectedAccountId: string,
    marketId: string | null = null,
    after: string | null = null,
    signal?: AbortSignal,
  ) {
    if (
      !identifierSchema.safeParse(expectedAccountId).success ||
      (marketId !== null && !identifierSchema.safeParse(marketId).success)
    )
      return Promise.resolve<
        ApiResponse<z.infer<typeof predictionPositionPageSchema>>
      >({ status: "fail", data: { query: "invalid" } });
    return this.storePage(
      "/v1/predictions/positions",
      bearer,
      predictionPositionPageSchema.refine(
        (value) =>
          value.accountId === expectedAccountId &&
          value.items.every(
            (item) => marketId === null || item.marketId === marketId,
          ),
      ),
      (item) => item.id,
      after,
      signal,
      new URLSearchParams(marketId ? { marketId } : {}),
      50,
    );
  }
  /** Submit immutable prediction terms; no actor, hold, payout or outcome is accepted. */
  predictionCommand(bearer: string, input: PredictionCommand) {
    const parsed = predictionCommandSchema.safeParse(input);
    return parsed.success
      ? this.request(
          "/v1/predictions/commands",
          bearer,
          commandAckSchema.refine((ack) =>
            predictionAckMatches(parsed.data, ack.resourceId),
          ),
          parsed.data,
        )
      : Promise.resolve<ApiResponse<z.infer<typeof commandAckSchema>>>({
          status: "fail",
          data: { request: "invalid" },
        });
  }
  /** Recover the exact original commitment without creating a replacement order. */
  predictionCommandDisposition(bearer: string, input: PredictionCommand) {
    const parsed = predictionCommandSchema.safeParse(input);
    return parsed.success
      ? this.request(
          "/v1/predictions/commands/disposition",
          bearer,
          predictionCommandDispositionSchema.refine(
            (value) =>
              value.state !== "accepted" ||
              predictionAckMatches(parsed.data, value.response.data.resourceId),
          ),
          parsed.data,
        )
      : Promise.resolve<
          ApiResponse<z.infer<typeof predictionCommandDispositionSchema>>
        >({ status: "fail", data: { request: "invalid" } });
  }
  economyCommand(bearer: string, input: EconomyCommand) {
    const parsed = economyCommandSchema.safeParse(input);
    return parsed.success
      ? this.request(
          "/v1/economy/commands",
          bearer,
          commandAckSchema.refine((ack) =>
            economyAckMatches(parsed.data, ack.resourceId),
          ),
          parsed.data,
        )
      : Promise.resolve<ApiResponse<z.infer<typeof commandAckSchema>>>({
          status: "fail",
          data: { request: "invalid" },
        });
  }
  economyCommandDisposition(bearer: string, input: EconomyCommand) {
    const parsed = economyCommandSchema.safeParse(input);
    return parsed.success
      ? this.request(
          "/v1/economy/commands/disposition",
          bearer,
          economyCommandDispositionSchema.refine(
            (value) =>
              value.state !== "accepted" ||
              economyAckMatches(parsed.data, value.response.data.resourceId),
          ),
          parsed.data,
        )
      : Promise.resolve<
          ApiResponse<z.infer<typeof economyCommandDispositionSchema>>
        >({ status: "fail", data: { request: "invalid" } });
  }
  socialCommand(bearer: string, input: SocialCommand) {
    const parsed = socialCommandSchema.safeParse(input);
    return parsed.success
      ? this.request(
          "/v1/social/commands",
          bearer,
          commandAckSchema.refine(
            (ack) =>
              parsed.data.kind !== "block" ||
              ack.resourceId === parsed.data.command.otherAccountId,
          ),
          parsed.data,
        )
      : Promise.resolve<ApiResponse<z.infer<typeof commandAckSchema>>>({
          status: "fail",
          data: { request: "invalid" },
        });
  }
  markDirectRead(bearer: string, input: MarkDirectRead) {
    const parsed = markDirectReadSchema.safeParse(input);
    return parsed.success
      ? this.request(
          "/v1/social/read",
          bearer,
          commandAckSchema.refine(
            (ack) => ack.resourceId === parsed.data.conversationId,
          ),
          parsed.data,
        )
      : Promise.resolve<ApiResponse<z.infer<typeof commandAckSchema>>>({
          status: "fail",
          data: { request: "invalid" },
        });
  }
  directReadDisposition(bearer: string, input: MarkDirectRead) {
    const parsed = markDirectReadSchema.safeParse(input);
    return parsed.success
      ? this.request(
          "/v1/social/read/disposition",
          bearer,
          directReadDispositionSchema.refine(
            (value) =>
              value.state !== "accepted" ||
              value.response.data.resourceId === parsed.data.conversationId,
          ),
          parsed.data,
        )
      : Promise.resolve<
          ApiResponse<z.infer<typeof directReadDispositionSchema>>
        >({ status: "fail", data: { request: "invalid" } });
  }
  socialCommandDisposition(bearer: string, input: SocialCommand) {
    const parsed = socialCommandSchema.safeParse(input);
    return parsed.success
      ? this.request(
          "/v1/social/commands/disposition",
          bearer,
          socialCommandDispositionSchema.refine(
            (disposition) =>
              parsed.data.kind !== "block" ||
              disposition.state !== "accepted" ||
              disposition.response.data.resourceId ===
                parsed.data.command.otherAccountId,
          ),
          parsed.data,
        )
      : Promise.resolve<
          ApiResponse<z.infer<typeof socialCommandDispositionSchema>>
        >({ status: "fail", data: { request: "invalid" } });
  }
  socialStatus(
    bearer: string,
    expectedAccountId: string,
    personId: string | null = null,
    signal?: AbortSignal,
  ) {
    if (
      !identifierSchema.safeParse(expectedAccountId).success ||
      (personId !== null && !identifierSchema.safeParse(personId).success)
    )
      return Promise.resolve({
        status: "fail",
        data: { query: "invalid" },
      } satisfies ApiResponse<never>);
    const query =
      personId === null ? "" : `?${new URLSearchParams({ personId })}`;
    return this.request(
      `/v1/social/status${query}`,
      bearer,
      socialStatusSchema.refine(
        (status) =>
          status.accountId === expectedAccountId &&
          status.personId === personId,
      ),
      undefined,
      signal,
    );
  }
  socialDirectory(
    bearer: string,
    expectedAccountId: string,
    query: string,
    after: string | null = null,
    signal?: AbortSignal,
  ) {
    return this.socialPeople(bearer, expectedAccountId, query, after, signal);
  }
  socialThreads(
    bearer: string,
    expectedAccountId: string,
    after: string | null = null,
    signal?: AbortSignal,
  ) {
    return this.socialPeople(bearer, expectedAccountId, null, after, signal);
  }
  private async socialPeople(
    bearer: string,
    expectedAccountId: string,
    query: string | null,
    after: string | null,
    signal?: AbortSignal,
  ) {
    if (
      !identifierSchema.safeParse(expectedAccountId).success ||
      (query !== null && !socialDirectoryQuerySchema.safeParse(query).success)
    )
      return {
        status: "fail",
        data: { query: "invalid" },
      } satisfies ApiResponse<never>;
    const literal = query
      ?.replace(/^ +| +$/g, "")
      .replace(/[A-Z]/g, (letter) => letter.toLowerCase());
    return this.storePage(
      query === null ? "/v1/social/threads" : "/v1/social/people",
      bearer,
      socialPeoplePageSchema.refine(
        (page) =>
          page.accountId === expectedAccountId &&
          (literal === undefined ||
            page.items.every(
              (person) =>
                !person.isDeleted &&
                person.username.toLowerCase().includes(literal),
            )),
      ),
      (person) => person.id,
      after,
      signal,
      query === null ? undefined : new URLSearchParams({ query }),
    );
  }
  directMessages(
    bearer: string,
    expectedAccountId: string,
    conversationId: string,
    before: MessageCursor | null = null,
    signal?: AbortSignal,
  ) {
    if (
      !identifierSchema.safeParse(expectedAccountId).success ||
      !identifierSchema.safeParse(conversationId).success ||
      (before !== null && !messageCursorSchema.safeParse(before).success)
    )
      return Promise.resolve({
        status: "fail",
        data: { query: "invalid" },
      } satisfies ApiResponse<never>);
    const query = new URLSearchParams({
      limit: String(DIRECT_MESSAGE_PAGE_LIMIT),
    });
    if (before) {
      query.set("beforeAt", before.sentAt);
      query.set("beforeId", before.id);
    }
    return this.request(
      `/v1/social/conversations/${conversationId}/messages?${query}`,
      bearer,
      directMessagePageSchema.refine(
        (page) =>
          page.accountId === expectedAccountId &&
          page.conversationId === conversationId &&
          (page.items.length === DIRECT_MESSAGE_PAGE_LIMIT) ===
            (page.nextCursor !== null) &&
          (before === null ||
            page.items.every((message) =>
              messagePositionBefore(message, before),
            )),
      ),
      undefined,
      signal,
    );
  }
  async deck(bearer: string, deckId: string, signal?: AbortSignal) {
    if (!identifierSchema.safeParse(deckId).success)
      return {
        status: "fail",
        data: { deck: "invalid" },
      } satisfies ApiResponse<never>;
    const result = await this.request(
      `/v1/decks/${deckId}`,
      bearer,
      deckDetailSchema,
      undefined,
      signal,
    );
    if (result.status === "success" && result.data.deck.id !== deckId)
      return unavailable;
    return result;
  }
  formats(bearer: string, after: string | null = null, signal?: AbortSignal) {
    return this.storePage(
      "/v1/formats",
      bearer,
      deckFormatPageSchema,
      (format) => format.id,
      after,
      signal,
    );
  }
  async purchase(bearer: string, id: string, signal?: AbortSignal) {
    if (!identifierSchema.safeParse(id).success)
      return {
        status: "fail",
        data: { purchase: "invalid" },
      } satisfies ApiResponse<never>;
    const result = await this.request(
      `/v1/purchases/${id}`,
      bearer,
      purchaseSchema,
      undefined,
      signal,
    );
    if (result.status === "success" && result.data.purchaseId !== id)
      return unavailable;
    return result;
  }

  // -------------------------------------------------------------------------
  // Live gameplay, match history, replays & opening/between-game choices
  // -------------------------------------------------------------------------
  gameView(
    bearer: string,
    gameId: string,
    contextId: string,
    signal?: AbortSignal,
  ) {
    return this.request(
      `/v1/games/${encodeURIComponent(gameId)}/view`,
      bearer,
      playerSnapshotSchema,
      { gameId, contextId },
      signal,
    );
  }

  gameCommand(
    bearer: string,
    gameId: string,
    command: PlayerCommand,
    signal?: AbortSignal,
  ) {
    return this.request(
      `/v1/games/${encodeURIComponent(gameId)}/commands`,
      bearer,
      playerCommandAckSchema,
      command,
      signal,
    );
  }

  async gameStream(
    bearer: string,
    gameId: string,
    contextId: string,
    cursor: PlayerCursor | null,
    onFrame: (frame: PlayerStreamFrame) => void,
    signal?: AbortSignal,
  ): Promise<ApiResponse<{ closed: true }>> {
    if (bearer.length < 1 || bearer.length > 16384 || /\s/.test(bearer)) {
      return { status: "fail", data: { session: "unauthorized" } };
    }
    try {
      const response = await this.transport(
        `${this.origin}/v1/games/${encodeURIComponent(gameId)}/stream`,
        {
          method: "POST",
          headers: {
            Authorization: `Bearer ${bearer}`,
            "Content-Type": "application/json",
            Accept: "text/event-stream",
          },
          body: JSON.stringify({ gameId, contextId, cursor }),
          ...(signal ? { signal } : {}),
          credentials: "omit",
          redirect: "error",
          cache: "no-store",
          referrerPolicy: "no-referrer",
        },
      );
      if (!response.ok || !response.body) {
        const parsed = apiResponseSchema(z.unknown()).safeParse(
          await boundedJson(response),
        );
        return parsed.success && parsed.data.status !== "success"
          ? parsed.data
          : unavailable;
      }
      const reader = response.body.getReader();
      const decoder = new TextDecoder();
      let buffer = "";
      while (true) {
        const { done, value } = await reader.read();
        if (done) break;
        buffer += decoder.decode(value, { stream: true });
        let boundary = buffer.indexOf("\n\n");
        while (boundary !== -1) {
          const chunk = buffer.slice(0, boundary).trim();
          buffer = buffer.slice(boundary + 2);
          if (chunk.startsWith("data:")) {
            const raw = chunk.slice(5).trim();
            const envelope = apiResponseSchema(
              playerStreamFrameSchema,
            ).safeParse(JSON.parse(raw));
            if (envelope.success) {
              if (envelope.data.status === "success") {
                onFrame(envelope.data.data);
              } else {
                await reader.cancel();
                return envelope.data;
              }
            }
          }
          boundary = buffer.indexOf("\n\n");
        }
      }
      return { status: "success", data: { closed: true } };
    } catch {
      return unavailable;
    }
  }

  gameReconnect(
    bearer: string,
    gameId: string,
    contextId: string,
    cursor: PlayerCursor | null = null,
    signal?: AbortSignal,
  ) {
    return this.request(
      `/v1/games/${encodeURIComponent(gameId)}/reconnect`,
      bearer,
      playerReconnectSchema,
      { gameId, contextId, cursor },
      signal,
    );
  }

  gameHistory(
    bearer: string,
    gameId: string,
    contextId: string,
    afterSequence: string | null = null,
    limit = 50,
    signal?: AbortSignal,
  ) {
    return this.request(
      `/v1/games/${encodeURIComponent(gameId)}/history`,
      bearer,
      playerHistoryPageSchema,
      { gameId, contextId, afterSequence, limit },
      signal,
    );
  }

  gameReplay(
    bearer: string,
    gameId: string,
    input: {
      gameId: string;
      contextId: string;
      mode: "live_perspective" | "completed_full_information";
      revision: string;
    },
    signal?: AbortSignal,
  ) {
    return this.request(
      `/v1/games/${encodeURIComponent(gameId)}/replay`,
      bearer,
      playerReplayFrameSchema,
      input,
      signal,
    );
  }

  gameReceipts(
    bearer: string,
    gameId: string,
    input: Record<string, unknown>,
    signal?: AbortSignal,
  ) {
    return this.request(
      `/v1/games/${encodeURIComponent(gameId)}/receipts`,
      bearer,
      playerReceiptSchema,
      input,
      signal,
    );
  }

  gameOpeningChoice(
    bearer: string,
    gameId: string,
    input: {
      gameId: string;
      playFirst?: boolean | null;
      priorChooserSeatId?: string | null;
      firstSeatId?: string | null;
    },
    signal?: AbortSignal,
  ) {
    return this.request(
      `/v1/games/${encodeURIComponent(gameId)}/opening-choice`,
      bearer,
      openingChoiceResponseSchema,
      input,
      signal,
    );
  }

  betweenGameReply(
    bearer: string,
    input: Record<string, unknown>,
    signal?: AbortSignal,
  ) {
    return this.request(
      "/v1/between-game/replies",
      bearer,
      z.record(z.string(), z.unknown()),
      input,
      signal,
    );
  }

  betweenGameReplyDisposition(
    bearer: string,
    input: Record<string, unknown>,
    signal?: AbortSignal,
  ) {
    return this.request(
      "/v1/between-game/replies/disposition",
      bearer,
      z.record(z.string(), z.unknown()),
      input,
      signal,
    );
  }

  matches(bearer: string, after?: string | null, limit = 50, signal?: AbortSignal) {
    const q = new URLSearchParams({ limit: String(limit) });
    if (after) q.set("after", after);
    return this.request(
      `/v1/matches?${q}`,
      bearer,
      z.record(z.string(), z.unknown()),
      undefined,
      signal,
    );
  }

  matchDetail(bearer: string, matchId: string, signal?: AbortSignal) {
    return this.request(
      `/v1/matches/${encodeURIComponent(matchId)}`,
      bearer,
      z.record(z.string(), z.unknown()),
      undefined,
      signal,
    );
  }

  matchGames(bearer: string, matchId: string, after?: string | null, limit = 50, signal?: AbortSignal) {
    const q = new URLSearchParams({ limit: String(limit) });
    if (after) q.set("after", after);
    return this.request(
      `/v1/matches/${encodeURIComponent(matchId)}/games?${q}`,
      bearer,
      z.record(z.string(), z.unknown()),
      undefined,
      signal,
    );
  }

  matchSeats(bearer: string, matchId: string, after?: string | null, limit = 50, signal?: AbortSignal) {
    const q = new URLSearchParams({ limit: String(limit) });
    if (after) q.set("after", after);
    return this.request(
      `/v1/matches/${encodeURIComponent(matchId)}/seats?${q}`,
      bearer,
      z.record(z.string(), z.unknown()),
      undefined,
      signal,
    );
  }

  matchDecisions(bearer: string, matchId: string, after?: string | null, limit = 50, signal?: AbortSignal) {
    const q = new URLSearchParams({ limit: String(limit) });
    if (after) q.set("after", after);
    return this.request(
      `/v1/matches/${encodeURIComponent(matchId)}/decisions?${q}`,
      bearer,
      z.record(z.string(), z.unknown()),
      undefined,
      signal,
    );
  }

  matchChat(bearer: string, matchId: string, before?: MessageCursor | null, signal?: AbortSignal) {
    const q = new URLSearchParams({ limit: "50" });
    if (before) {
      q.set("beforeAt", before.sentAt);
      q.set("beforeId", before.id);
    }
    return this.request(
      `/v1/matches/${encodeURIComponent(matchId)}/chat?${q}`,
      bearer,
      z.record(z.string(), z.unknown()),
      undefined,
      signal,
    );
  }

  sendMatchChat(
    bearer: string,
    matchId: string,
    input: { requestId: string; body: string },
    signal?: AbortSignal,
  ) {
    return this.request(
      `/v1/matches/${encodeURIComponent(matchId)}/chat/messages`,
      bearer,
      commandAckSchema,
      input,
      signal,
    );
  }

  // -------------------------------------------------------------------------
  // League, progression, matchmaking, quests & supply ballot
  // -------------------------------------------------------------------------
  ranks(
    bearer: string,
    opts?: { formatId?: string; seasonId?: string; after?: string; limit?: number },
    signal?: AbortSignal,
  ) {
    const q = new URLSearchParams();
    if (opts?.formatId) q.set("formatId", opts.formatId);
    if (opts?.seasonId) q.set("seasonId", opts.seasonId);
    if (opts?.after) q.set("after", opts.after);
    if (opts?.limit) q.set("limit", String(opts.limit));
    const qs = q.toString();
    return this.request(
      `/v1/ranks${qs ? `?${qs}` : ""}`,
      bearer,
      z.record(z.string(), z.unknown()),
      undefined,
      signal,
    );
  }

  seasons(bearer: string, after?: string | null, limit = 50, signal?: AbortSignal) {
    const q = new URLSearchParams({ limit: String(limit) });
    if (after) q.set("after", after);
    return this.request(
      `/v1/seasons?${q}`,
      bearer,
      z.record(z.string(), z.unknown()),
      undefined,
      signal,
    );
  }

  quests(bearer: string, after?: string | null, limit = 50, signal?: AbortSignal) {
    const q = new URLSearchParams({ limit: String(limit) });
    if (after) q.set("after", after);
    return this.request(
      `/v1/quests?${q}`,
      bearer,
      z.record(z.string(), z.unknown()),
      undefined,
      signal,
    );
  }

  rerollQuest(
    bearer: string,
    input: { requestId: string; assignmentId: string },
    signal?: AbortSignal,
  ) {
    return this.request(
      "/v1/quests/reroll",
      bearer,
      z.record(z.string(), z.unknown()),
      input,
      signal,
    );
  }

  dailyWins(bearer: string, after?: string | null, limit = 50, signal?: AbortSignal) {
    const q = new URLSearchParams({ limit: String(limit) });
    if (after) q.set("after", after);
    return this.request(
      `/v1/daily-wins?${q}`,
      bearer,
      z.record(z.string(), z.unknown()),
      undefined,
      signal,
    );
  }

  competitiveResults(
    bearer: string,
    opts?: { formatId?: string; seasonId?: string; after?: string; limit?: number },
    signal?: AbortSignal,
  ) {
    const q = new URLSearchParams();
    if (opts?.formatId) q.set("formatId", opts.formatId);
    if (opts?.seasonId) q.set("seasonId", opts.seasonId);
    if (opts?.after) q.set("after", opts.after);
    if (opts?.limit) q.set("limit", String(opts.limit));
    const qs = q.toString();
    return this.request(
      `/v1/competitive-results${qs ? `?${qs}` : ""}`,
      bearer,
      z.record(z.string(), z.unknown()),
      undefined,
      signal,
    );
  }

  queueState(bearer: string, after?: string | null, limit = 50, signal?: AbortSignal) {
    const q = new URLSearchParams({ limit: String(limit) });
    if (after) q.set("after", after);
    return this.request(
      `/v1/queue?${q}`,
      bearer,
      z.record(z.string(), z.unknown()),
      undefined,
      signal,
    );
  }

  enqueueMatchmaking(
    bearer: string,
    input: Record<string, unknown>,
    signal?: AbortSignal,
  ) {
    return this.request(
      "/v1/queue",
      bearer,
      z.record(z.string(), z.unknown()),
      input,
      signal,
    );
  }

  cancelMatchmaking(
    bearer: string,
    entryId: string,
    input: { requestId: string },
    signal?: AbortSignal,
  ) {
    return this.request(
      `/v1/queue/${encodeURIComponent(entryId)}/cancel`,
      bearer,
      commandAckSchema,
      input,
      signal,
    );
  }

  seasonBallot(bearer: string, seasonId: string, signal?: AbortSignal) {
    return this.request(
      `/v1/seasons/${encodeURIComponent(seasonId)}/ballot`,
      bearer,
      z.record(z.string(), z.unknown()),
      undefined,
      signal,
    );
  }

  replaceSeasonBallot(
    bearer: string,
    seasonId: string,
    input: Record<string, unknown>,
    signal?: AbortSignal,
  ) {
    return this.request(
      `/v1/seasons/${encodeURIComponent(seasonId)}/ballot`,
      bearer,
      z.record(z.string(), z.unknown()),
      input,
      signal,
    );
  }

  leagueLeaderboard(
    bearer: string,
    opts?: { formatId?: string; seasonId?: string; offset?: number; limit?: number },
    signal?: AbortSignal,
  ) {
    const q = new URLSearchParams();
    if (opts?.formatId) q.set("formatId", opts.formatId);
    if (opts?.seasonId) q.set("seasonId", opts.seasonId);
    if (opts?.offset !== undefined) q.set("offset", String(opts.offset));
    if (opts?.limit !== undefined) q.set("limit", String(opts.limit));
    const qs = q.toString();
    return this.request(
      `/v1/league/leaderboard${qs ? `?${qs}` : ""}`,
      bearer,
      leaderboardPageSchema,
      undefined,
      signal,
    );
  }

  playerPublicSummary(bearer: string, playerId: string, signal?: AbortSignal) {
    return this.request(
      `/v1/players/${encodeURIComponent(playerId)}/summary`,
      bearer,
      playerPublicSummarySchema,
      undefined,
      signal,
    );
  }

  // -------------------------------------------------------------------------
  // Profile, badge, Patreon & billing
  // -------------------------------------------------------------------------
  updateProfileArt(
    bearer: string,
    input: {
      requestId: string;
      expected: unknown;
      cardArtProfile: unknown;
    },
    signal?: AbortSignal,
  ) {
    return this.request(
      "/v1/account/profile",
      bearer,
      commandAckSchema,
      input,
      signal,
    );
  }

  setProfileBadge(bearer: string, badge: string | null, signal?: AbortSignal) {
    return this.request(
      "/v1/account/profile/badge",
      bearer,
      profileBadgeResultSchema,
      { badge },
      signal,
    );
  }

  patreonAccountStatus(bearer: string, signal?: AbortSignal) {
    return this.request(
      "/v1/account/patreon",
      bearer,
      patreonAccountStatusSchema,
      undefined,
      signal,
    );
  }

  createBillingPortalSession(
    bearer: string,
    returnUrl: string,
    signal?: AbortSignal,
  ) {
    return this.request(
      "/v1/billing/portal",
      bearer,
      billingPortalSessionSchema,
      { returnUrl },
      signal,
    );
  }

  // -------------------------------------------------------------------------
  // Rich catalogue, decks & cosmetics
  // -------------------------------------------------------------------------
  cardMetadata(
    bearer: string,
    input: { printingIds?: string[]; definitionIds?: string[] },
    signal?: AbortSignal,
  ) {
    return this.request(
      "/v1/catalogue/metadata",
      bearer,
      cardMetadataBatchSchema,
      input,
      signal,
    );
  }

  searchCardsRich(
    bearer: string,
    opts?: {
      query?: string;
      formatId?: string;
      rarity?: string;
      setId?: string;
      after?: string;
      limit?: number;
    },
    signal?: AbortSignal,
  ) {
    const q = new URLSearchParams();
    if (opts?.query) q.set("query", opts.query);
    if (opts?.formatId) q.set("formatId", opts.formatId);
    if (opts?.rarity) q.set("rarity", opts.rarity);
    if (opts?.setId) q.set("setId", opts.setId);
    if (opts?.after) q.set("after", opts.after);
    if (opts?.limit !== undefined) q.set("limit", String(opts.limit));
    const qs = q.toString();
    return this.request(
      `/v1/catalogue/cards${qs ? `?${qs}` : ""}`,
      bearer,
      richCardSearchPageSchema,
      undefined,
      signal,
    );
  }

  deleteDeck(
    bearer: string,
    deckId: string,
    requestId?: string,
    signal?: AbortSignal,
  ) {
    const qs = requestId ? `?requestId=${encodeURIComponent(requestId)}` : "";
    return this.request(
      `/v1/decks/${encodeURIComponent(deckId)}${qs}`,
      bearer,
      commandAckSchema,
      undefined,
      signal,
      "DELETE",
    );
  }

  duplicateDeck(
    bearer: string,
    deckId: string,
    input: { requestId?: string; name?: string } = {},
    signal?: AbortSignal,
  ) {
    return this.request(
      `/v1/decks/${encodeURIComponent(deckId)}/duplicate`,
      bearer,
      duplicateDeckAckSchema,
      input,
      signal,
    );
  }

  cosmeticsRich(
    bearer: string,
    opts?: {
      kind?: "back" | "sleeve" | "dice" | "playmat" | "pet";
      after?: string;
      limit?: number;
    },
    signal?: AbortSignal,
  ) {
    const q = new URLSearchParams();
    if (opts?.kind) q.set("kind", opts.kind);
    if (opts?.after) q.set("after", opts.after);
    if (opts?.limit !== undefined) q.set("limit", String(opts.limit));
    const qs = q.toString();
    return this.request(
      `/v1/cosmetics/catalogue${qs ? `?${qs}` : ""}`,
      bearer,
      richCosmeticPageSchema,
      undefined,
      signal,
    );
  }

  // -------------------------------------------------------------------------
  // Market search, bilateral live trade rooms & predictions search/one-bet
  // -------------------------------------------------------------------------
  marketSearch(
    bearer: string,
    opts?: {
      search?: string;
      kind?: "card" | "pack" | "coin";
      rarity?: string;
      minPrice?: number;
      maxPrice?: number;
      owned?: boolean;
      sort?: "newest" | "price_asc" | "price_desc";
      offset?: number;
      limit?: number;
    },
    signal?: AbortSignal,
  ) {
    const q = new URLSearchParams();
    if (opts?.search) q.set("search", opts.search);
    if (opts?.kind) q.set("kind", opts.kind);
    if (opts?.rarity) q.set("rarity", opts.rarity);
    if (opts?.minPrice !== undefined) q.set("minPrice", String(opts.minPrice));
    if (opts?.maxPrice !== undefined) q.set("maxPrice", String(opts.maxPrice));
    if (opts?.owned !== undefined) q.set("owned", String(opts.owned));
    if (opts?.sort) q.set("sort", opts.sort);
    if (opts?.offset !== undefined) q.set("offset", String(opts.offset));
    if (opts?.limit !== undefined) q.set("limit", String(opts.limit));
    const qs = q.toString();
    return this.request(
      `/v1/market/search${qs ? `?${qs}` : ""}`,
      bearer,
      marketSearchPageSchema,
      undefined,
      signal,
    );
  }

  tradeSessions(
    bearer: string,
    opts?: { state?: "active" | "completed" | "canceled"; limit?: number },
    signal?: AbortSignal,
  ) {
    const q = new URLSearchParams();
    if (opts?.state) q.set("state", opts.state);
    if (opts?.limit !== undefined) q.set("limit", String(opts.limit));
    const qs = q.toString();
    return this.request(
      `/v1/trades/sessions${qs ? `?${qs}` : ""}`,
      bearer,
      tradeSessionListSchema,
      undefined,
      signal,
    );
  }

  createTradeSession(
    bearer: string,
    counterpartyId: string,
    requestId?: string,
    signal?: AbortSignal,
  ) {
    return this.request(
      "/v1/trades/sessions",
      bearer,
      tradeSessionEnvelopeSchema,
      { counterpartyId, ...(requestId ? { requestId } : {}) },
      signal,
    );
  }

  tradeSessionDetail(bearer: string, sessionId: string, signal?: AbortSignal) {
    return this.request(
      `/v1/trades/sessions/${encodeURIComponent(sessionId)}`,
      bearer,
      tradeSessionEnvelopeSchema,
      undefined,
      signal,
    );
  }

  updateTradeSessionOffer(
    bearer: string,
    sessionId: string,
    input: {
      expectedRevision: number;
      coins: number;
      items: Array<{ assetId: string; quantity: number }>;
    },
    signal?: AbortSignal,
  ) {
    return this.request(
      `/v1/trades/sessions/${encodeURIComponent(sessionId)}/offer`,
      bearer,
      tradeSessionEnvelopeSchema,
      input,
      signal,
      "PATCH",
    );
  }

  setTradeSessionReady(
    bearer: string,
    sessionId: string,
    expectedRevision: number,
    ready: boolean,
    signal?: AbortSignal,
  ) {
    return this.request(
      `/v1/trades/sessions/${encodeURIComponent(sessionId)}/ready`,
      bearer,
      tradeSessionEnvelopeSchema,
      { expectedRevision, ready },
      signal,
    );
  }

  confirmTradeSession(
    bearer: string,
    sessionId: string,
    expectedRevision: number,
    signal?: AbortSignal,
  ) {
    return this.request(
      `/v1/trades/sessions/${encodeURIComponent(sessionId)}/confirm`,
      bearer,
      tradeSessionEnvelopeSchema,
      { expectedRevision },
      signal,
    );
  }

  cancelTradeSession(bearer: string, sessionId: string, signal?: AbortSignal) {
    return this.request(
      `/v1/trades/sessions/${encodeURIComponent(sessionId)}/cancel`,
      bearer,
      tradeSessionEnvelopeSchema,
      {},
      signal,
    );
  }

  predictionsSearch(
    bearer: string,
    opts?: {
      search?: string;
      state?: string;
      sort?: "closing_soon" | "newest" | "resolved_recent";
      offset?: number;
      limit?: number;
    },
    signal?: AbortSignal,
  ) {
    const q = new URLSearchParams();
    if (opts?.search) q.set("search", opts.search);
    if (opts?.state) q.set("state", opts.state);
    if (opts?.sort) q.set("sort", opts.sort);
    if (opts?.offset !== undefined) q.set("offset", String(opts.offset));
    if (opts?.limit !== undefined) q.set("limit", String(opts.limit));
    const qs = q.toString();
    return this.request(
      `/v1/predictions/markets/search${qs ? `?${qs}` : ""}`,
      bearer,
      predictionSearchPageSchema,
      undefined,
      signal,
    );
  }

  predictionBet(
    bearer: string,
    marketId: string,
    input: {
      requestId: string;
      side: "yes" | "no";
      limitCostCoins: number;
      quantity: number;
    },
    signal?: AbortSignal,
  ) {
    return this.request(
      `/v1/predictions/markets/${encodeURIComponent(marketId)}/bet`,
      bearer,
      z.record(z.string(), z.unknown()),
      input,
      signal,
    );
  }

  // -------------------------------------------------------------------------
  // Lobby, invitations, live limited draft, notifications & DM attachments
  // -------------------------------------------------------------------------
  rooms(bearer: string, after?: string | null, limit = 50, signal?: AbortSignal) {
    const q = new URLSearchParams({ limit: String(limit) });
    if (after) q.set("after", after);
    return this.request(
      `/v1/rooms?${q}`,
      bearer,
      z.record(z.string(), z.unknown()),
      undefined,
      signal,
    );
  }

  roomGame(bearer: string, roomId: string, signal?: AbortSignal) {
    return this.request(
      `/v1/rooms/${encodeURIComponent(roomId)}/game`,
      bearer,
      roomGameHandoffSchema,
      undefined,
      signal,
    );
  }

  roomEnrichedDetail(bearer: string, roomId: string, signal?: AbortSignal) {
    return this.request(
      `/v1/rooms/${encodeURIComponent(roomId)}/detail`,
      bearer,
      roomEnrichedDetailSchema,
      undefined,
      signal,
    );
  }

  updateRoomOptions(
    bearer: string,
    roomId: string,
    options: {
      formatId?: string | null;
      aiDifficulty?: "casual" | "standard" | "competitive";
      planechaseEnabled?: boolean;
      rangeOfInfluence?: number;
    },
    signal?: AbortSignal,
  ) {
    return this.request(
      `/v1/rooms/${encodeURIComponent(roomId)}/options`,
      bearer,
      roomEnrichedDetailSchema,
      options,
      signal,
      "PATCH",
    );
  }

  issueRoomInvitation(
    bearer: string,
    roomId: string,
    input: { requestId: string },
    signal?: AbortSignal,
  ) {
    return this.request(
      `/v1/rooms/${encodeURIComponent(roomId)}/invitations`,
      bearer,
      z.record(z.string(), z.unknown()),
      input,
      signal,
    );
  }

  inviteRoomByUsername(
    bearer: string,
    roomId: string,
    username: string,
    requestId?: string,
    signal?: AbortSignal,
  ) {
    return this.request(
      `/v1/rooms/${encodeURIComponent(roomId)}/invitations/by-username`,
      bearer,
      roomInviteNotificationAckSchema,
      { username, ...(requestId ? { requestId } : {}) },
      signal,
    );
  }

  joinRoom(
    bearer: string,
    roomId: string,
    input: { requestId: string; token?: string },
    signal?: AbortSignal,
  ) {
    return this.request(
      `/v1/rooms/${encodeURIComponent(roomId)}/join`,
      bearer,
      z.record(z.string(), z.unknown()),
      input,
      signal,
    );
  }

  invitations(bearer: string, after?: string | null, limit = 50, signal?: AbortSignal) {
    const q = new URLSearchParams({ limit: String(limit) });
    if (after) q.set("after", after);
    return this.request(
      `/v1/invitations?${q}`,
      bearer,
      z.record(z.string(), z.unknown()),
      undefined,
      signal,
    );
  }

  revokeInvitation(
    bearer: string,
    invitationId: string,
    input: { requestId: string },
    signal?: AbortSignal,
  ) {
    return this.request(
      `/v1/invitations/${encodeURIComponent(invitationId)}/revoke`,
      bearer,
      commandAckSchema,
      input,
      signal,
    );
  }

  limitedDraft(bearer: string, roomId: string, signal?: AbortSignal) {
    return this.request(
      `/v1/rooms/${encodeURIComponent(roomId)}/draft`,
      bearer,
      limitedDraftStateSchema,
      undefined,
      signal,
    );
  }

  startLimitedDraft(
    bearer: string,
    roomId: string,
    sourceId: string,
    requestId: string,
    signal?: AbortSignal,
  ) {
    return this.request(
      `/v1/rooms/${encodeURIComponent(roomId)}/draft/start`,
      bearer,
      limitedDraftAckSchema,
      { sourceId, requestId },
      signal,
    );
  }

  pickLimitedDraftCard(
    bearer: string,
    roomId: string,
    input: { draftId: string; step: number; itemId: string; requestId: string },
    signal?: AbortSignal,
  ) {
    return this.request(
      `/v1/rooms/${encodeURIComponent(roomId)}/draft/pick`,
      bearer,
      limitedDraftAckSchema,
      input,
      signal,
    );
  }

  sealLimitedDraftPool(bearer: string, roomId: string, signal?: AbortSignal) {
    return this.request(
      `/v1/rooms/${encodeURIComponent(roomId)}/draft/seal`,
      bearer,
      limitedDraftAckSchema,
      {},
      signal,
    );
  }

  notifications(bearer: string, after?: string | null, limit = 50, signal?: AbortSignal) {
    const q = new URLSearchParams({ limit: String(limit) });
    if (after) q.set("after", after);
    return this.request(
      `/v1/notifications?${q}`,
      bearer,
      z.record(z.string(), z.unknown()),
      undefined,
      signal,
    );
  }

  markNotificationsRead(
    bearer: string,
    input: { requestId: string; notificationIds: string[] },
    signal?: AbortSignal,
  ) {
    return this.request(
      "/v1/notifications/read",
      bearer,
      commandAckSchema,
      input,
      signal,
    );
  }

  sendDmAttachment(
    bearer: string,
    conversationId: string,
    input: {
      requestId: string;
      body?: string;
      attachment: { kind: "trade_invite" | "game_invite"; [key: string]: unknown };
    },
    signal?: AbortSignal,
  ) {
    return this.request(
      `/v1/social/conversations/${encodeURIComponent(conversationId)}/attachments`,
      bearer,
      dmAttachmentMessageSchema,
      input,
      signal,
    );
  }
}

/** Refinement must stay total even when an earlier nested URL refinement reported an issue. */
function patreonAuthorizationState(value: string, expected: string): boolean {
  try {
    return new URL(value).searchParams.get("state") === expected;
  } catch {
    return false;
  }
}

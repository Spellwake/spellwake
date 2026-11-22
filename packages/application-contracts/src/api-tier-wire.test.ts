import { validHumanFixture } from "./human-wire-cases.ts";
import { assetIdentitySchema } from "./assets.ts";
import { describe, expect, test } from "bun:test";
import {
  gameChatPageSchema,
  validGameChatPage,
  sendGameMessageSchema,
  notificationPageSchema,
  validNotificationPage,
  readNotificationsSchema,
  changeProfileArtSchema,
  ownProfileArtSchema,
  publicProfileArtSchema,
  validOwnProfileArt,
  validPublicProfileArt,
  deckLibraryPageSchema,
  roomCommandSchema,
  enqueueMatchmakingSchema,
  enqueueMatchmakingAckSchema,
  queuePageSchema,
  queueIntentSchema,
  tableProfilePageSchema,
  profileConfigurationPageSchema,
  validQueuePage,
  validQueueIntent,
  validTableProfilePage,
  validProfileConfigurationPage,
  issueInvitationSchema,
  joinRoomSchema,
  revokeInvitationSchema,
  issuedInvitationSchema,
  joinRoomAckSchema,
  validJoinRoomAck,
  configureRoomSchema,
  roomConfigurationAckSchema,
  validRoomConfigurationAck,
  roomDetailSchema,
  validRoomDetail,
  importDeckSchema,
  importDeckAckSchema,
  importDeckDispositionSchema,
  questPageSchema,
  rankPageSchema,
  seasonPageSchema,
  validProgressionPage,
  validRankPage,
} from "./index.ts";
import artifact from "../fixtures/api-tier-wire.json";
import { parseStrictJson } from "./raw-json.ts";

describe("shared Rust/TypeScript API wire fixtures", () => {
  for (const fixture of artifact.cases) {
    test(fixture.name, () => {
      const raw = parseStrictJson(fixture.wire);
      if (!raw.success) {
        expect(fixture.valid).toBe(false);
        return;
      }
      const value: unknown = raw.value;
      let valid: boolean;
      switch (fixture.kind) {
        case "asset_identity":
          valid = assetIdentitySchema.safeParse(value).success;
          break;
        case "deck_library":
          valid = deckLibraryPageSchema.safeParse(value).success;
          break;
        case "profile_art_command":
          valid = changeProfileArtSchema.safeParse(value).success;
          break;
        case "own_profile_art": {
          const p = ownProfileArtSchema.safeParse(value);
          valid = p.success && validOwnProfileArt(p.data, artifact.owner);
          break;
        }
        case "public_profile_art": {
          const p = publicProfileArtSchema.safeParse(value);
          valid =
            p.success &&
            validPublicProfileArt(
              p.data,
              artifact.owner,
              "playerId" in fixture ? String(fixture.playerId) : "",
            );
          break;
        }
        case "game_chat": {
          const p = gameChatPageSchema.safeParse(value);
          valid =
            p.success &&
            validGameChatPage(
              p.data,
              artifact.owner,
              "matchId" in fixture ? String(fixture.matchId) : "",
              null,
              16,
            );
          break;
        }
        case "send_game_message":
          valid = sendGameMessageSchema.safeParse(value).success;
          break;
        case "notifications": {
          const p = notificationPageSchema.safeParse(value);
          valid =
            p.success &&
            validNotificationPage(p.data, artifact.owner, null, 100);
          break;
        }
        case "read_notifications":
          valid = readNotificationsSchema.safeParse(value).success;
          break;
        case "enqueue":
          valid = enqueueMatchmakingSchema.safeParse(value).success;
          break;
        case "enqueue_ack":
          valid = enqueueMatchmakingAckSchema.safeParse(value).success;
          break;
        case "queue_page": {
          const p = queuePageSchema.safeParse(value);
          valid =
            p.success &&
            validQueuePage(
              p.data,
              artifact.owner,
              "intentId" in fixture ? fixture.intentId : null,
              null,
              100,
            );
          break;
        }
        case "queue_intent": {
          const p = queueIntentSchema.safeParse(value);
          valid =
            p.success &&
            "intentId" in fixture &&
            validQueueIntent(p.data, artifact.owner, fixture.intentId);
          break;
        }
        case "profile_page": {
          const p = tableProfilePageSchema.safeParse(value);
          valid =
            p.success &&
            validTableProfilePage(p.data, artifact.owner, null, 100);
          break;
        }
        case "profile_configurations": {
          const p = profileConfigurationPageSchema.safeParse(value);
          valid =
            p.success &&
            "profileId" in fixture &&
            validProfileConfigurationPage(
              p.data,
              artifact.owner,
              fixture.profileId,
              "afterIndex" in fixture ? fixture.afterIndex : null,
              "limit" in fixture ? fixture.limit : 100,
            );
          break;
        }
        case "issue_invitation":
          valid = issueInvitationSchema.safeParse(value).success;
          break;
        case "join_room":
          valid = joinRoomSchema.safeParse(value).success;
          break;
        case "revoke_invitation":
          valid = revokeInvitationSchema.safeParse(value).success;
          break;
        case "issued_invitation":
          valid = issuedInvitationSchema.safeParse(value).success;
          break;
        case "join_ack": {
          const parsed = joinRoomAckSchema.safeParse(value);
          valid =
            parsed.success &&
            "roomId" in fixture &&
            validJoinRoomAck(parsed.data, fixture.roomId);
          break;
        }
        case "configuration_ack": {
          const parsed = roomConfigurationAckSchema.safeParse(value);
          valid =
            parsed.success &&
            "roomId" in fixture &&
            "expectedRevision" in fixture &&
            validRoomConfigurationAck(
              {
                requestId: artifact.owner,
                profileId: artifact.owner,
                expectedRevision: fixture.expectedRevision,
                structure: "bo1",
                teams: [],
              },
              fixture.roomId,
              parsed.data,
            );
          break;
        }
        case "configuration":
          valid = configureRoomSchema.safeParse(value).success;
          break;
        case "room_detail": {
          const parsed = roomDetailSchema.safeParse(value);
          valid =
            parsed.success &&
            "roomId" in fixture &&
            validRoomDetail(
              parsed.data,
              artifact.owner,
              fixture.roomId,
              null,
              100,
            );
          break;
        }
        case "room":
          valid = roomCommandSchema.safeParse(value).success;
          break;
        case "import":
          valid = importDeckSchema.safeParse(value).success;
          break;
        case "ack":
          valid = importDeckAckSchema.safeParse(value).success;
          break;
        case "disposition":
          valid = importDeckDispositionSchema.safeParse(value).success;
          break;
        case "quests": {
          const parsed = questPageSchema.safeParse(value);
          valid =
            parsed.success &&
            validProgressionPage(parsed.data, artifact.owner, null, 100);
          break;
        }
        case "ranks": {
          const parsed = rankPageSchema.safeParse(value);
          valid =
            parsed.success &&
            validRankPage(
              parsed.data,
              artifact.owner,
              null,
              100,
              "formatId" in fixture ? fixture.formatId : null,
              "seasonId" in fixture ? fixture.seasonId : null,
            );
          break;
        }
        case "seasons": {
          const parsed = seasonPageSchema.safeParse(value);
          valid =
            parsed.success &&
            validProgressionPage(parsed.data, artifact.owner, null, 100);
          break;
        }
        default: {
          const result = validHumanFixture(
            value,
            fixture.kind,
            artifact.owner,
            fixture,
          );
          if (result === null) throw new Error("Unknown wire fixture kind");
          valid = result;
        }
      }
      expect(valid).toBe(fixture.valid);
    });
  }
});

describe("game and notification receipt failures", () => {
  test("only persisted business failures appear in rejected receipts", async () => {
    const { gameMessageDispositionSchema, notificationReadDispositionSchema } =
      await import("./index.ts");
    const rejected = (data: Record<string, string>) => ({
      state: "rejected",
      response: { status: "fail", data },
    });
    expect(
      gameMessageDispositionSchema.safeParse(
        rejected({ conversation: "conversation unavailable" }),
      ).success,
    ).toBe(true);
    expect(
      notificationReadDispositionSchema.safeParse(
        rejected({ notifications: "unavailable" }),
      ).success,
    ).toBe(true);
    for (const schema of [
      gameMessageDispositionSchema,
      notificationReadDispositionSchema,
    ]) {
      expect(schema.safeParse(rejected({ request: "conflict" })).success).toBe(
        false,
      );
      expect(schema.safeParse(rejected({ provider: "private" })).success).toBe(
        false,
      );
    }
  });
});

test("profile acknowledgments bind the original caller", async () => {
  const { profileArtDispositionSchema, validProfileArtDisposition } =
    await import("./index.ts");
  const value = {
    state: "accepted",
    response: { status: "success", data: { resourceId: artifact.owner } },
  };
  const parsed = profileArtDispositionSchema.parse(value);
  expect(validProfileArtDisposition(parsed, artifact.owner)).toBe(true);
  expect(
    validProfileArtDisposition(parsed, "22222222-2222-4222-8222-222222222222"),
  ).toBe(false);
});

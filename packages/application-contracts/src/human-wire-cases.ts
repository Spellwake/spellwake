/** Shared artifact test adapter; fixtures are decoded through the same public schemas/context validators. */
import { z } from "zod";
import * as c from "./index.ts";
export function validHumanFixture(
  value: unknown,
  kind: string,
  owner: string,
  context: Record<string, unknown>,
): boolean | null {
  const text = (key: string): string =>
    typeof context[key] === "string" ? context[key] : "";
  const limit = typeof context.limit === "number" ? context.limit : 100;
  const check = <T>(
    schema: z.ZodType<T>,
    validate: (v: T) => boolean = () => true,
  ): boolean => {
    const p = schema.safeParse(value);
    return p.success && validate(p.data);
  };
  const controlKind = text("operation");
  const validControl = (ack: z.infer<typeof c.controlAckSchema>): boolean =>
    (controlKind === "add" ||
      controlKind === "bootstrap" ||
      controlKind === "remove" ||
      controlKind === "unready") &&
    c.validControlAck(
      controlKind,
      text("targetId"),
      text("expectedRevision"),
      ack,
    );
  switch (kind) {
    case "deck_command":
      return check(c.deckCommandSchema);
    case "social_command":
      return check(c.socialCommandSchema);
    case "economy_command":
      return check(c.economyCommandSchema);
    case "prediction_command":
      return check(c.predictionCommandSchema);
    case "source_preparation":
      return check(
        c.predictionSourcePreparationSchema,
        (v) => v.accountId === owner,
      );
    case "match_detail":
      return check(c.matchDetailSchema, (v) =>
        c.validMatchDetail(v, owner, text("matchId")),
      );
    case "match_list":
      return check(c.matchListSchema, (v) =>
        c.validProgressionPage(v, owner, null, limit),
      );
    case "game_page":
      return check(c.gamePageSchema, (v) =>
        c.validMatchPage(v, owner, text("matchId"), null, limit),
      );
    case "seat_page":
      return check(c.seatPageSchema, (v) =>
        c.validMatchPage(v, owner, text("matchId"), null, limit),
      );
    case "decision_page":
      return check(c.decisionPageSchema, (v) =>
        c.validMatchPage(v, owner, text("matchId"), null, limit),
      );
    case "credit_page":
      return check(c.creditPageSchema, (v) =>
        c.validCreditPage(
          v,
          owner,
          null,
          limit,
          text("formatId") || null,
          text("seasonId") || null,
        ),
      );
    case "daily_win_page":
      return check(c.dailyWinPageSchema, (v) =>
        c.validProgressionPage(v, owner, null, limit),
      );
    case "ai_status_page":
      return check(c.aiStatusPageSchema, (v) =>
        c.validAiStatusPage(v, owner, text("roomId"), null, limit),
      );
    case "ballot_page":
      return check(c.ballotPageSchema, (v) =>
        c.validBallotPage(v, owner, text("seasonId"), null, limit),
      );
    case "ante_detail":
      return check(c.anteDetailSchema, (v) =>
        c.validAnteDetail(v, owner, text("matchId")),
      );
    case "revision_request":
      return check(c.revisionRequestSchema);
    case "bootstrap_request":
      return check(c.bootstrapRequestSchema);
    case "replace_ballot":
      return check(c.replaceBallotSchema);
    case "ante_offer":
      return check(c.offerAnteSchema);
    case "ante_accept":
      return check(c.acceptAnteSchema);
    case "ante_cancel":
      return check(c.cancelAnteSchema);
    case "control_ack":
      return check(c.controlAckSchema, validControl);
    case "control_disposition":
      switch (controlKind) {
        case "add":
          return check(
            c.addAiDispositionSchema,
            (v) => v.state !== "accepted" || validControl(v.response.data),
          );
        case "bootstrap":
          return check(
            c.bootstrapDispositionSchema,
            (v) => v.state !== "accepted" || validControl(v.response.data),
          );
        case "remove":
          return check(
            c.removeAiDispositionSchema,
            (v) => v.state !== "accepted" || validControl(v.response.data),
          );
        case "unready":
          return check(
            c.unreadyDispositionSchema,
            (v) => v.state !== "accepted" || validControl(v.response.data),
          );
        default:
          return false;
      }
    case "ante_ack":
      switch (text("operation")) {
        case "offer":
          return check(c.anteOfferAckSchema, (v) =>
            c.validAnteOfferAck(v, {
              requestId: owner,
              expectedTermsRevision: text("expectedRevision"),
              assets: [],
            }),
          );
        case "accept":
          return check(c.anteAcceptAckSchema, (v) =>
            c.validAnteAcceptAck(v, text("targetId"), {
              requestId: owner,
              expectedTermsRevision: text("expectedRevision"),
              termsHash: "a".repeat(64),
            }),
          );
        case "cancel":
          return check(c.anteCancelAckSchema, (v) =>
            c.validAnteCancelAck(v, text("targetId")),
          );
        default:
          return false;
      }
    case "ante_disposition":
      switch (text("operation")) {
        case "offer":
          return check(
            c.anteOfferDispositionSchema,
            (v) =>
              v.state !== "accepted" ||
              c.validAnteOfferAck(v.response.data, {
                requestId: owner,
                expectedTermsRevision: text("expectedRevision"),
                assets: [],
              }),
          );
        case "accept":
          return check(
            c.anteAcceptDispositionSchema,
            (v) =>
              v.state !== "accepted" ||
              c.validAnteAcceptAck(v.response.data, text("targetId"), {
                requestId: owner,
                expectedTermsRevision: text("expectedRevision"),
                termsHash: "a".repeat(64),
              }),
          );
        case "cancel":
          return check(
            c.anteCancelDispositionSchema,
            (v) =>
              v.state !== "accepted" ||
              c.validAnteCancelAck(v.response.data, text("targetId")),
          );
        default:
          return false;
      }
    case "ballot_ack":
      return check(c.ballotAckSchema, (v) =>
        c.validBallotAck(v, text("targetId"), {
          requestId: owner,
          setIds: [owner, text("targetId")],
        }),
      );
    case "ballot_disposition":
      return check(
        c.ballotDispositionSchema,
        (v) =>
          v.state !== "accepted" ||
          c.validBallotAck(v.response.data, text("targetId"), {
            requestId: owner,
            setIds: [owner, text("targetId")],
          }),
      );
    default:
      return null;
  }
}

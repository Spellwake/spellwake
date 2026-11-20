import { expect, test } from "bun:test";
import { z } from "zod";
import fixture from "../fixtures/qualified-intents.json";
import { identifierSchema } from "./primitives.ts";
import { roomConfigurationAckSchema } from "./rooms.ts";
import { parseStrictJson } from "./raw-json.ts";
import {
  roomAdmissionIntentSchema,
  betweenGameIntentSchema,
  questRerollIntentSchema,
  betweenGameAckSchema,
  questRerollAckSchema,
  validRoomAdmissionAck,
  validRoomAdmissionDisposition,
  validBetweenGameAck,
  validBetweenGameDisposition,
  validQuestRerollAck,
  validQuestRerollDisposition,
} from "./qualified-intents.ts";
const cases = z
  .strictObject({
    cases: z.array(
      z.strictObject({
        name: z.string(),
        kind: z.string(),
        wire: z.string(),
        valid: z.boolean(),
        room: identifierSchema,
        original_room: roomAdmissionIntentSchema,
        original_between: betweenGameIntentSchema,
        original_reroll: questRerollIntentSchema,
      }),
    ),
  })
  .parse(fixture).cases;
for (const c of cases)
  test(`original human wire: ${c.name}`, () => {
    const raw = parseStrictJson(c.wire, 32768);
    let valid = false;
    if (raw.success) {
      const value = raw.value;
      switch (c.kind) {
        case "room_intent":
          valid = roomAdmissionIntentSchema.safeParse(value).success;
          break;
        case "between_intent":
          valid = betweenGameIntentSchema.safeParse(value).success;
          break;
        case "reroll_intent":
          valid = questRerollIntentSchema.safeParse(value).success;
          break;
        case "room_ack": {
          const a = roomConfigurationAckSchema.safeParse(value);
          valid =
            a.success && validRoomAdmissionAck(c.original_room, c.room, a.data);
          break;
        }
        case "between_ack": {
          const a = betweenGameAckSchema.safeParse(value);
          valid = a.success && validBetweenGameAck(c.original_between, a.data);
          break;
        }
        case "reroll_ack": {
          const a = questRerollAckSchema.safeParse(value);
          valid = a.success && validQuestRerollAck(c.original_reroll, a.data);
          break;
        }
        case "ready_disposition":
        case "force_disposition":
          valid = validRoomAdmissionDisposition(
            c.original_room,
            c.room,
            c.kind === "force_disposition",
            value,
          );
          break;
        case "between_disposition":
          valid = validBetweenGameDisposition(c.original_between, value);
          break;
        case "reroll_disposition":
          valid = validQuestRerollDisposition(c.original_reroll, value);
          break;
        default:
          throw new Error("Unknown fixture kind");
      }
    }
    expect(valid).toBe(c.valid);
  });

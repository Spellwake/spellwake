import { describe, expect, test } from "bun:test";
import {
  aiQuotaStatusSchema,
  aiSlotStatusSchema,
  setAiTierDispositionSchema,
  setAiTierRequestSchema,
} from "@spellwake/application-contracts";

const UUID_1 = "11111111-1111-4111-8111-111111111111";
const UUID_2 = "22222222-2222-4222-8222-222222222222";

describe("AI tier and daily quota contracts", () => {
  test("validates finite and unlimited AI quota responses", () => {
    const free = aiQuotaStatusSchema.parse({
      accountId: UUID_1,
      tierName: "Free",
      displayUnlimited: false,
      dailyLimit: 3,
      usedToday: 1,
      remainingToday: 2,
      normalUnlimited: true,
      resetsAt: "2026-11-23T00:00:00.000000Z",
    });
    expect(free.remainingToday).toBe(2);

    const benefactor = aiQuotaStatusSchema.parse({
      accountId: UUID_1,
      tierName: "Benefactor",
      displayUnlimited: true,
      dailyLimit: null,
      usedToday: 15,
      remainingToday: null,
      normalUnlimited: true,
      resetsAt: "2026-11-23T00:00:00.000000Z",
    });
    expect(benefactor.displayUnlimited).toBe(true);
    expect(benefactor.dailyLimit).toBeNull();
  });

  test("validates setAiTier request, disposition, and aiSlotStatus default tier", () => {
    const req = setAiTierRequestSchema.parse({
      requestId: UUID_1,
      expectedRevision: "4",
      tier: "advanced",
    });
    expect(req.tier).toBe("advanced");

    const disp = setAiTierDispositionSchema.parse({
      state: "accepted",
      response: {
        status: "success",
        data: { resourceId: UUID_2, revision: "5" },
      },
    });
    expect(disp.state).toBe("accepted");

    const legacySlot = aiSlotStatusSchema.parse({
      slotId: UUID_2,
      seatIndex: "1",
      isSponsor: true,
      isRetired: false,
      stopPending: false,
      deckRevisionId: null,
      isReady: false,
      binding: null,
    });
    expect(legacySlot.aiTier).toBe("normal");
  });
});

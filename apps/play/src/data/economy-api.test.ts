import { expect, test } from "bun:test";
import {
  economyCommandSchema,
  type EconomyCommand,
} from "@spellwake/application-contracts";
import { ApplicationApi } from "./application-api";
import { ownedPackOutcomeSchema } from "@spellwake/application-contracts";

const target = "11111111-1111-4111-8111-111111111111";
const request = "22222222-2222-4222-8222-222222222222";
const big = "9007199254740993";

test("owned pack results bind receipt and owner, preserve quantities and omit private randomness", async () => {
  const valid = {
    accountId: target,
    outcome: {
      id: request,
      inventoryAssetId: target,
      recipeId: target,
      operationId: target,
      contents: [{ printingId: target, quantity: "2" }],
    },
  };
  expect(ownedPackOutcomeSchema.safeParse(valid).success).toBe(true);
  for (const contents of [
    [],
    [{ printingId: target, quantity: "invalid" }],
    [{ printingId: target, quantity: "64001" }],
    [...valid.outcome.contents, ...valid.outcome.contents],
  ]) {
    expect(
      ownedPackOutcomeSchema.safeParse({
        ...valid,
        outcome: { ...valid.outcome, contents },
      }).success,
    ).toBe(false);
  }
  expect(
    ownedPackOutcomeSchema.safeParse({
      ...valid,
      outcome: { ...valid.outcome, seed: "private" },
    }).success,
  ).toBe(false);
  const seen: { url: string; authorization: string | null }[] = [];
  let payload = valid;
  const api = ApplicationApi.create(
    "https://api.example",
    async (input, options) => {
      seen.push({
        url: String(input),
        authorization: new Headers(options?.headers).get("authorization"),
      });
      return new Response(JSON.stringify({ status: "success", data: payload }));
    },
  );
  if (!api) throw new Error("Expected client");
  expect((await api.packOutcome("original", target, request)).status).toBe(
    "success",
  );
  expect(seen).toEqual([
    {
      url: `https://api.example/v1/packs/openings/${request}`,
      authorization: "Bearer original",
    },
  ]);
  payload = { ...valid, accountId: request };
  expect((await api.packOutcome("original", target, request)).status).toBe(
    "error",
  );
  payload = { ...valid, outcome: { ...valid.outcome, id: target } };
  expect((await api.packOutcome("original", target, request)).status).toBe(
    "error",
  );
});
const commands: EconomyCommand[] = [
  {
    kind: "trade_create",
    command: {
      requestId: request,
      recipientId: target,
      give: [],
      request: [{ assetId: target, quantity: big }],
    },
  },
  {
    kind: "trade_response",
    command: { requestId: request, tradeId: target, action: "accept" },
  },
  {
    kind: "listing_create",
    command: {
      requestId: request,
      lot: { assetId: target, quantity: big },
      totalPrice: big,
    },
  },
  {
    kind: "listing_cancel",
    command: { requestId: request, listingId: target },
  },
  {
    kind: "listing_buy",
    command: {
      requestId: request,
      listingId: target,
      expectedRevision: "1",
      expectedTotal: big,
    },
  },
  {
    kind: "pack_buy",
    command: {
      requestId: request,
      recipeId: target,
      quantity: "1000",
      expectedPolicyId: target,
      expectedRevision: "0",
      expectedTotal: big,
    },
  },
  {
    kind: "pack_open",
    command: { requestId: request, inventoryAssetId: target },
  },
];
test("all economy intent shapes retain exact terms and reject authority injection", () => {
  for (const input of commands) {
    expect(economyCommandSchema.parse(input)).toEqual(input);
    for (const field of ["actorId", "seed", "estimatedValue", "holdId"]) {
      expect(
        economyCommandSchema.safeParse({
          ...input,
          command: { ...input.command, [field]: target },
        }).success,
      ).toBe(false);
    }
    expect(
      economyCommandSchema.safeParse({
        ...input,
        command: {
          ...input.command,
          requestId: "00000000-0000-0000-0000-000000000000",
        },
      }).success,
    ).toBe(false);
  }
  const line = { assetId: target, quantity: "1" };
  for (const give of [
    [],
    [line, line],
    [{ ...line, quantity: "0" }],
    [{ ...line, quantity: 1 }],
  ]) {
    expect(
      economyCommandSchema.safeParse({
        kind: "trade_create",
        command: { requestId: request, recipientId: target, give, request: [] },
      }).success,
    ).toBe(false);
  }
});
test("economy delivery and recovery use the original bearer with immutable exact intent", async () => {
  const seen: { url: string; bearer: string | null; body: unknown }[] = [];
  const api = ApplicationApi.create(
    "https://api.example",
    async (input, options) => {
      const url = String(input);
      seen.push({
        url,
        bearer: new Headers(options?.headers).get("authorization"),
        body: JSON.parse(String(options?.body)),
      });
      return new Response(
        JSON.stringify({
          status: "success",
          data: url.endsWith("/disposition")
            ? {
                state: "accepted",
                response: { status: "success", data: { resourceId: target } },
              }
            : { resourceId: target },
        }),
      );
    },
  );
  if (!api) throw new Error("Expected client");
  for (const command of commands) {
    expect((await api.economyCommand("original", command)).status).toBe(
      "success",
    );
    expect(
      (await api.economyCommandDisposition("original", command)).status,
    ).toBe("success");
  }
  expect(seen).toEqual(
    commands.flatMap((command) =>
      ["", "/disposition"].map((suffix) => ({
        url: `https://api.example/v1/economy/commands${suffix}`,
        bearer: "Bearer original",
        body: command,
      })),
    ),
  );
});
test("foreign fixed-target economy acknowledgments never confirm an attempt", async () => {
  const api = ApplicationApi.create(
    "https://api.example",
    async (input) =>
      new Response(
        JSON.stringify({
          status: "success",
          data: String(input).endsWith("/disposition")
            ? {
                state: "accepted",
                response: { status: "success", data: { resourceId: request } },
              }
            : { resourceId: request },
        }),
      ),
  );
  if (!api) throw new Error("Expected client");
  for (const command of commands.filter((command) =>
    ["trade_response", "listing_cancel", "listing_buy"].includes(command.kind),
  )) {
    expect((await api.economyCommand("original", command)).status).toBe(
      "error",
    );
    expect(
      (await api.economyCommandDisposition("original", command)).status,
    ).toBe("error");
  }
});

import { expect, test } from "bun:test";
import {
  predictionCommandSchema,
  type PredictionCommand,
} from "@spellwake/application-contracts";
import { ApplicationApi } from "./application-api";
const requestId = "11111111-1111-4111-8111-111111111111";
const target = "22222222-2222-4222-8222-222222222222";
const place: PredictionCommand = {
  kind: "place_order",
  command: {
    requestId,
    marketId: target,
    side: "yes",
    limitCost: "60",
    quantity: "9007199254740993",
  },
};
const cancel: PredictionCommand = {
  kind: "cancel_order",
  command: { requestId, orderId: target },
};
test("prediction client preserves exact original request for execution and disposition", async () => {
  let calls = 0;
  const api = ApplicationApi.create(
    "https://api.example",
    async (url, init) => {
      expect(new Headers(init?.headers).get("Authorization")).toBe(
        "Bearer original",
      );
      expect(JSON.parse(String(init?.body))).toEqual(
        calls < 2 ? place : cancel,
      );
      calls++;
      return Response.json({
        status: "success",
        data: String(url).endsWith("disposition")
          ? {
              state: "accepted",
              response: { status: "success", data: { resourceId: target } },
            }
          : { resourceId: target },
      });
    },
  );
  for (const input of [place, cancel]) {
    expect((await api?.predictionCommand("original", input))?.status).toBe(
      "success",
    );
    expect(
      (await api?.predictionCommandDisposition("original", input))?.status,
    ).toBe("success");
  }
  expect(calls).toBe(4);
});
test("prediction contracts reject claimed authority, bad bids and unbackable totals", () => {
  for (const command of [
    { ...place.command, actorId: target },
    { ...place.command, limitCost: "100" },
    { ...place.command, quantity: "0" },
    { ...place.command, quantity: "9".repeat(78) },
    { ...place.command, quantity: 1 },
  ])
    expect(
      predictionCommandSchema.safeParse({ kind: "place_order", command })
        .success,
    ).toBe(false);
  expect(
    predictionCommandSchema.safeParse({
      kind: "sell_position",
      command: { requestId, orderId: target },
    }).success,
  ).toBe(false);
});
test("prediction cancellation rejects foreign resource acknowledgments and private disposition fields", async () => {
  const api = ApplicationApi.create("https://api.example", async (url) =>
    Response.json({
      status: "success",
      data: String(url).endsWith("disposition")
        ? {
            state: "accepted",
            response: {
              status: "success",
              data: { resourceId: target, holdId: requestId },
            },
          }
        : { resourceId: requestId },
    }),
  );
  expect((await api?.predictionCommand("original", cancel))?.status).toBe(
    "error",
  );
  expect(
    (await api?.predictionCommandDisposition("original", cancel))?.status,
  ).toBe("error");
});

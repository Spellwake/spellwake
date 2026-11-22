import { expect, test } from "bun:test";
import {
  setSummarySchema,
  setPageSchema,
} from "@spellwake/application-contracts";
import { ApplicationApi } from "./application-api";
const id = "11111111-1111-4111-8111-111111111111";
test("set discovery preserves literal search and missing optional code, without product availability guesses", async () => {
  const data = {
    items: [{ id, name: "A %_ set", code: null }],
    nextCursor: null,
  };
  let calls = 0;
  const api = ApplicationApi.create(
    "https://api.example",
    async (url, init) => {
      calls++;
      expect(String(url)).toBe(
        "https://api.example/v1/sets?query=%25_&limit=40",
      );
      expect(new Headers(init?.headers).get("Authorization")).toBe(
        "Bearer caller",
      );
      return Response.json({ status: "success", data });
    },
  );
  expect(await api?.sets("caller", "%_")).toEqual({ status: "success", data });
  expect((await api?.sets("caller", "x".repeat(1025)))?.status).toBe("fail");
  expect(calls).toBe(1);
  expect(setSummarySchema.safeParse({ id, name: "Set" }).success).toBe(false);
  expect(
    setSummarySchema.safeParse({ id, name: "Set", code: null, price: "1000" })
      .success,
  ).toBe(false);
  expect(
    setPageSchema.safeParse({
      items: [...data.items, ...data.items],
      nextCursor: null,
    }).success,
  ).toBe(false);
});
test("set pages reject stale cursors and hidden source metadata", async () => {
  const api = ApplicationApi.create("https://api.example", async () =>
    Response.json({
      status: "success",
      data: { items: [{ id, name: "Set", code: "SET" }], nextCursor: id },
    }),
  );
  expect((await api?.sets("caller"))?.status).toBe("error");
  expect(
    setSummarySchema.safeParse({
      id,
      name: "Set",
      code: "SET",
      catalogueReceiptId: id,
    }).success,
  ).toBe(false);
});

import { expect, test } from "bun:test";
import { ApplicationApi } from "./application-api";

const definitionId = "10000000-0000-4000-8000-000000000001";
const printingId = "20000000-0000-4000-8000-000000000001";
const foreignId = "30000000-0000-4000-8000-000000000001";
const card = { id: definitionId, name: "Literal %_★", oracleRef: "oracle" };
const printing = {
  id: printingId,
  definitionId,
  externalPrintingRef: "source-printing",
  rarity: "rare",
  finish: "foil",
  collectorNumber: "123a★",
  language: "en",
  set: { id: foreignId, name: "Set", code: "tst" },
};

test("catalogue queries preserve literal hints and original caller credentials", async () => {
  const requests: { url: URL; bearer: string | null }[] = [];
  const api = ApplicationApi.create(
    "https://api.example",
    async (input, init) => {
      requests.push({
        url: new URL(String(input)),
        bearer: new Headers(init?.headers).get("authorization"),
      });
      return Response.json({
        status: "success",
        data: { items: [], nextCursor: null },
      });
    },
  )!;
  await api.cards("owner", card.name, true);
  await api.namedPrintings("owner", card.name, {
    setCode: "TST",
    collectorNumber: "123a★",
  });
  expect(requests.map((request) => request.bearer)).toEqual([
    "Bearer owner",
    "Bearer owner",
  ]);
  expect(requests[0]?.url.searchParams.get("query")).toBe(card.name);
  expect(requests[0]?.url.searchParams.get("exact")).toBe("true");
  expect(requests[1]?.url.pathname).toBe("/v1/cards/printings");
  expect(requests[1]?.url.searchParams.get("collectorNumber")).toBe("123a★");
  expect(requests[1]?.url.searchParams.get("setCode")).toBe("TST");
});

test("label resolution permits missing labels but rejects unsolicited and duplicate identities", async () => {
  let labels: unknown = { cards: [], printings: [] };
  let calls = 0;
  const api = ApplicationApi.create("https://api.example", async () => {
    calls += 1;
    return Response.json({ status: "success", data: labels });
  })!;
  const query = { definitionIds: [definitionId], printingIds: [printingId] };
  expect((await api.catalogue("owner", query)).status).toBe("success");
  labels = { cards: [card], printings: [printing] };
  expect((await api.catalogue("owner", query)).status).toBe("success");
  labels = { cards: [{ ...card, id: foreignId }], printings: [] };
  expect((await api.catalogue("owner", query)).status).toBe("error");
  labels = { cards: [card, card], printings: [] };
  expect((await api.catalogue("owner", query)).status).toBe("error");
  const before = calls;
  expect(
    (
      await api.catalogue("owner", {
        ...query,
        definitionIds: [definitionId, definitionId],
      })
    ).status,
  ).toBe("fail");
  expect(calls).toBe(before);
});

test("printing lookup rejects another definition and malformed pagination without choosing a variant", async () => {
  let items: unknown[] = [{ ...printing, definitionId: foreignId }];
  const api = ApplicationApi.create("https://api.example", async () =>
    Response.json({
      status: "success",
      data: { items, nextCursor: null },
    }),
  )!;
  expect((await api.printings("owner", definitionId)).status).toBe("error");
  items = [printing, { ...printing, id: foreignId, finish: "nonfoil" }];
  const variants = await api.namedPrintings("owner", card.name);
  expect(variants.status).toBe("success");
  if (variants.status === "success")
    expect(variants.data.items.map((item) => item.finish)).toEqual([
      "foil",
      "nonfoil",
    ]);
  items = [printing];
  expect((await api.printings("owner", definitionId, printingId)).status).toBe(
    "error",
  );
});

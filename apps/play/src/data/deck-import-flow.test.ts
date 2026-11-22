import { expect, test } from "bun:test";
import { ApplicationApi } from "./application-api";
import { importEntries } from "./deck-import-flow";

const id = (n: number) =>
  `10000000-0000-4000-8000-${String(n).padStart(12, "0")}`;
const cards: Record<string, string> = {
  "Lightning Bolt": id(1),
  "Goblin Guide": id(2),
  Shock: id(3),
};
const printings: Record<string, string> = { "Lightning Bolt|MSC|806": id(11) };

/** A stand-in catalogue. `outage` names cards whose lookups fail that many times. */
function catalogue(outage: Record<string, number> = {}) {
  const failures = { ...outage };
  return ApplicationApi.create("https://api.example", async (input) => {
    const url = new URL(String(input));
    const name =
      url.searchParams.get("query") ?? url.searchParams.get("name") ?? "";
    if ((failures[name] ?? 0) > 0) {
      failures[name] = (failures[name] ?? 0) - 1;
      return new Response("down", { status: 503 });
    }
    const definition = cards[name];
    if (url.pathname === "/v1/cards/printings") {
      const key = `${name}|${url.searchParams.get("setCode")}|${url.searchParams.get("collectorNumber")}`;
      const printing = printings[key];
      return Response.json({
        status: "success",
        data: {
          items:
            printing && definition
              ? [
                  {
                    id: printing,
                    definitionId: definition,
                    externalPrintingRef: "ref",
                    rarity: "common",
                    finish: "nonfoil",
                    collectorNumber: url.searchParams.get("collectorNumber"),
                    language: "en",
                    set: {
                      id: id(90),
                      name: "Set",
                      code: (
                        url.searchParams.get("setCode") ?? ""
                      ).toLowerCase(),
                    },
                  },
                ]
              : [],
          nextCursor: null,
        },
      });
    }
    return Response.json({
      status: "success",
      data: {
        items: definition
          ? [{ id: definition, name, oracleRef: "oracle" }]
          : [],
        nextCursor: null,
      },
    });
  })!;
}

test("found cards are kept, unknown and unreadable lines are left out", async () => {
  const result = await importEntries(
    catalogue(),
    "owner",
    [
      "4 Lightning Bolt (MSC) 806",
      "2 Not A Real Card",
      "this line is not a card",
      "",
      "Sideboard",
      "3 Shock",
    ].join("\n"),
  );
  expect(result).toEqual({
    status: "found",
    entries: [
      {
        definitionId: id(1),
        printingId: id(11),
        quantity: "4",
        section: "main",
      },
      {
        definitionId: id(3),
        printingId: null,
        quantity: "3",
        section: "sideboard",
      },
    ],
  });
});

test("a card whose named printing is unknown is kept without a printing", async () => {
  const result = await importEntries(
    catalogue(),
    "owner",
    "1 Goblin Guide (ZZZ) 1",
  );
  expect(result).toEqual({
    status: "found",
    entries: [
      { definitionId: id(2), printingId: null, quantity: "1", section: "main" },
    ],
  });
});

test("a lookup that fails is retried, and still failing fails the import", async () => {
  expect(
    await importEntries(catalogue({ Shock: 2 }), "owner", "3 Shock"),
  ).toMatchObject({ status: "found" });
  expect(
    await importEntries(catalogue({ Shock: 3 }), "owner", "3 Shock"),
  ).toEqual({ status: "unavailable" });
});

test("a list with no known cards imports as an empty deck", async () => {
  expect(await importEntries(catalogue(), "owner", "1 Nothing Here")).toEqual({
    status: "found",
    entries: [],
  });
});

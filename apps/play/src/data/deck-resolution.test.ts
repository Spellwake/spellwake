import { expect, test } from "bun:test";
import { ApplicationApi } from "./application-api";
import { parseDeckIntake } from "./deck-text";
import {
  assembleDeckImport,
  mergeDeckEntries,
  resolveDeckLines,
} from "./deck-resolution";

const id = "10000000-0000-4000-8000-000000000001";
const other = "20000000-0000-4000-8000-000000000001";
const printing = {
  id: other,
  definitionId: id,
  externalPrintingRef: "source",
  rarity: "rare",
  finish: "foil",
  collectorNumber: "123a★/b",
  language: "en",
  set: { id, name: "Set", code: "tst" },
};

test("neutral intake preserves original lines, section and printing hints without allocating identities", () => {
  const source =
    "Deck\r\n2x Literal %_★ (TST) 123a★/b\r\nSideboard:\r\n1 Another";
  const result = parseDeckIntake(source);
  expect(result.status).toBe("success");
  if (result.status === "success") {
    expect(result.lines).toEqual([
      {
        line: 2,
        name: "Literal %_★",
        quantity: "2",
        section: "main",
        sourceText: "2x Literal %_★ (TST) 123a★/b",
        printingHint: { setCode: "TST", collectorNumber: "123a★/b" },
      },
      {
        line: 4,
        name: "Another",
        quantity: "1",
        section: "sideboard",
        sourceText: "1 Another",
        printingHint: null,
      },
    ]);
  }
  for (const invalid of [
    "1 Valid\nnot a card line",
    "0 Invalid",
    "01 Invalid",
    "1 Bad\0name",
    "Deck\nSideboard",
    "1 Card\n".repeat(1001),
  ])
    expect(parseDeckIntake(invalid).status).toBe("fail");
});

test("canonical resolution retains missing and ambiguous lines, deduplicates reads and never falls back from a printing hint", async () => {
  const intake = parseDeckIntake(
    "2 Unique\n3 Unique\n1 Missing\n1 Ambiguous\n1 Specific (TST) 123a★/b",
  );
  if (intake.status !== "success") throw new Error("fixture intake");
  const queries: string[] = [];
  const api = ApplicationApi.create("https://api.example", async (input) => {
    const url = new URL(String(input));
    const name = url.searchParams.get("query") ?? url.searchParams.get("name");
    queries.push(name ?? "");
    const items =
      name === "Unique"
        ? [{ id, name, oracleRef: "oracle" }]
        : name === "Ambiguous"
          ? [
              { id, name, oracleRef: "oracle" },
              { id: other, name, oracleRef: "other-oracle" },
            ]
          : [];
    return Response.json({
      status: "success",
      data: { items, nextCursor: null },
    });
  })!;
  const result = await resolveDeckLines(api, "owner", intake.lines);
  expect(result.map((row) => row.state)).toEqual([
    "resolved",
    "resolved",
    "missing",
    "ambiguous",
    "missing",
  ]);
  expect(queries.filter((name) => name === "Unique")).toHaveLength(1);
  expect(queries.filter((name) => name === "Specific")).toHaveLength(1);
  expect(result.map((row) => row.line.sourceText)).toEqual(
    intake.lines.map((line) => line.sourceText),
  );
  expect(assembleDeckImport(result).status).toBe("fail");
  expect(assembleDeckImport(result.slice(0, 2))).toEqual({
    status: "success",
    entries: [
      { definitionId: id, printingId: null, quantity: "5", section: "main" },
    ],
  });
});

test("printing resolution preserves its exact identity and distinguishes unavailable from missing", async () => {
  const intake = parseDeckIntake("1 Specific (TST) 123a★/b\n1 Offline");
  if (intake.status !== "success") throw new Error("fixture intake");
  const api = ApplicationApi.create("https://api.example", async (input) =>
    new URL(String(input)).pathname === "/v1/cards/printings"
      ? Response.json({
          status: "success",
          data: { items: [printing], nextCursor: null },
        })
      : Response.json(
          { status: "error", message: "Unavailable" },
          { status: 503 },
        ),
  )!;
  const result = await resolveDeckLines(api, "owner", intake.lines);
  expect(result[0]).toMatchObject({
    state: "resolved",
    identity: { definitionId: id, printingId: other },
  });
  expect(result[1]?.state).toBe("unavailable");
  expect(assembleDeckImport(result).status).toBe("fail");
});

test("canonical tuple merging uses exact arithmetic, preserves sections and rejects overflow atomically", () => {
  const entry = {
    definitionId: id,
    printingId: null,
    quantity: "9007199254740993",
    section: "main",
  };
  expect(mergeDeckEntries([entry, entry])).toEqual({
    status: "success",
    entries: [{ ...entry, quantity: "18014398509481986" }],
  });
  const original = [entry, { ...entry, quantity: "9".repeat(78) }];
  const copy = structuredClone(original);
  expect(mergeDeckEntries(original).status).toBe("fail");
  expect(original).toEqual(copy);
  const variants = [
    entry,
    { ...entry, section: "sideboard" },
    { ...entry, printingId: other },
  ];
  expect(mergeDeckEntries(variants)).toEqual({
    status: "success",
    entries: variants,
  });
  expect(mergeDeckEntries([{ ...entry, quantity: "abc" }]).status).toBe("fail");
});

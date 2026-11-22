import { expect, test } from "bun:test";
import { z } from "zod";
import {
  predictionCommandSchema,
  ownedPredictionPublicationSourceSchema,
  publishPredictionSchema,
  predictionSourcePreparationSchema,
} from "@spellwake/application-contracts";
import { ApplicationApi } from "./application-api";
const f = z
  .strictObject({
    command: predictionCommandSchema,
    source: ownedPredictionPublicationSourceSchema,
  })
  .parse(
    await Bun.file(
      new URL(
        "../../../../services/application/tests/fixtures/prediction-publication.json",
        import.meta.url,
      ),
    ).json(),
  );
const printing = f.source.source.printingId,
  owner = f.source.accountId;
test("publication API preserves exact source and command without actor or source evidence fields", async () => {
  const urls: string[] = [];
  const api = ApplicationApi.create(
    "https://api.example",
    async (url, init) => {
      const path = String(url);
      urls.push(path);
      expect(new Headers(init?.headers).get("Authorization")).toBe(
        "Bearer original",
      );
      if (path.endsWith("/source"))
        return Response.json({ status: "success", data: f.source });
      expect(JSON.parse(String(init?.body))).toEqual(f.command);
      return Response.json({
        status: "success",
        data: path.endsWith("disposition")
          ? {
              state: "accepted",
              response: { status: "success", data: { resourceId: printing } },
            }
          : { resourceId: printing },
      });
    },
  );
  expect(
    await api?.predictionPublicationSource("original", owner, printing),
  ).toEqual({ status: "success", data: f.source });
  expect((await api?.predictionCommand("original", f.command))?.status).toBe(
    "success",
  );
  expect(
    (await api?.predictionCommandDisposition("original", f.command))?.status,
  ).toBe("success");
  expect(urls).toEqual([
    `https://api.example/v1/predictions/printings/${printing}/source`,
    "https://api.example/v1/predictions/commands",
    "https://api.example/v1/predictions/commands/disposition",
  ]);
  expect(
    (await api?.predictionPublicationSource("original", printing, printing))
      ?.status,
  ).toBe("error");
});
test("publication schemas reject zero targets, excess precision, wrong expiry and raw source payloads", () => {
  if (f.command.kind !== "publish_market")
    throw new Error("Publication fixture required");
  for (const command of [
    { ...f.command.command, targetUsd: "0.00" },
    { ...f.command.command, tradingClosesAt: "2030-01-01T00:00:00.000000001Z" },
    { ...f.command.command, actorId: owner },
    { ...f.command.command, probeId: "00000000-0000-0000-0000-000000000000" },
  ])
    expect(publishPredictionSchema.safeParse(command).success).toBe(false);
  expect(
    ownedPredictionPublicationSourceSchema.safeParse({
      ...f.source,
      source: {
        ...f.source.source,
        usableUntil: "2026-11-04T12:00:00.123456Z",
      },
    }).success,
  ).toBe(false);
  expect(
    ownedPredictionPublicationSourceSchema.safeParse({
      ...f.source,
      source: { ...f.source.source, artifactRef: "private" },
    }).success,
  ).toBe(false);
  expect(
    ownedPredictionPublicationSourceSchema.safeParse({
      ...f.source,
      source: { ...f.source.source, currentUsd: "0.00" },
    }).success,
  ).toBe(true);
});

test("source preparation is an explicit empty POST with owner and printing checked", async () => {
  for (const state of ["requested", "completed", "unavailable"] as const) {
    let calls = 0;
    const data = {
      accountId: owner,
      printingId: printing,
      slot: "2026-11-02",
      state,
    };
    const api = ApplicationApi.create(
      "https://api.example",
      async (url, init) => {
        calls++;
        expect(String(url)).toBe(
          `https://api.example/v1/predictions/printings/${printing}/source`,
        );
        expect(init?.method).toBe("POST");
        expect(JSON.parse(String(init?.body))).toEqual({});
        expect(new Headers(init?.headers).get("Authorization")).toBe(
          "Bearer original",
        );
        return Response.json(
          { status: "success", data },
          { status: state === "requested" ? 202 : 200 },
        );
      },
    );
    expect(
      await api?.preparePredictionSource("original", owner, printing),
    ).toEqual({ status: "success", data });
    expect(
      (await api?.preparePredictionSource("original", printing, printing))
        ?.status,
    ).toBe("error");
    expect(calls).toBe(2);
    expect(
      predictionSourcePreparationSchema.safeParse({
        ...data,
        artifactRef: "private",
      }).success,
    ).toBe(false);
    expect(
      predictionSourcePreparationSchema.safeParse({
        ...data,
        slot: "2026-02-30",
      }).success,
    ).toBe(false);
  }
});

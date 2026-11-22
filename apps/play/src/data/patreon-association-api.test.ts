import { expect, test } from "bun:test";
import {
  patreonLinkRequestSchema,
  patreonCompleteRequestSchema,
  patreonLinkStartSchema,
  patreonLinkStatusSchema,
  patreonAuthorizationUrlSchema,
} from "@spellwake/application-contracts";
import { ApplicationApi } from "./application-api";
import { newPatreonLinkRequest } from "./patreon-association";
const f: unknown = await Bun.file(
  new URL(
    "../../../../services/application/tests/fixtures/patreon-association.json",
    import.meta.url,
  ),
).json();
import { z } from "zod";
const fixture = z
  .object({
    flow: patreonLinkRequestSchema,
    complete: patreonCompleteRequestSchema,
    start: patreonLinkStartSchema,
    status: patreonLinkStatusSchema,
  })
  .parse(f);
test("Patreon source fixture uses strict owner/request/URL contracts and original empty-selector bodies", async () => {
  const paths: string[] = [];
  const api = ApplicationApi.create(
    "https://api.example",
    async (url, init) => {
      const path = String(url);
      paths.push(path);
      expect(init?.method).toBe("POST");
      expect(new Headers(init?.headers).get("Authorization")).toBe(
        "Bearer original",
      );
      expect(JSON.parse(String(init?.body))).toEqual(
        path.endsWith("complete") ? fixture.complete : fixture.flow,
      );
      return Response.json({
        status: "success",
        data: path.endsWith("start") ? fixture.start : fixture.status,
      });
    },
  );
  expect(
    await api?.patreonStart("original", fixture.status.accountId, fixture.flow),
  ).toEqual({ status: "success", data: fixture.start });
  expect(
    await api?.patreonComplete(
      "original",
      fixture.status.accountId,
      fixture.complete,
    ),
  ).toEqual({ status: "success", data: fixture.status });
  expect(
    await api?.patreonStatus(
      "original",
      fixture.status.accountId,
      fixture.flow,
    ),
  ).toEqual({ status: "success", data: fixture.status });
  expect(paths).toEqual([
    "https://api.example/v1/account/patreon/start",
    "https://api.example/v1/account/patreon/complete",
    "https://api.example/v1/account/patreon/status",
  ]);
  expect(
    (
      await api?.patreonComplete(
        "original",
        fixture.flow.requestId,
        fixture.complete,
      )
    )?.status,
  ).toBe("error");
});
test("Patreon responses cannot retarget ownership, request, state or public metadata", async () => {
  for (const invalid of [
    { ...fixture.start, accountId: fixture.flow.requestId },
    { ...fixture.start, requestId: fixture.start.accountId },
    {
      ...fixture.start,
      authorizationUrl: fixture.start.authorizationUrl.replace(
        fixture.flow.stateKey,
        "B".repeat(42) + "A",
      ),
    },
    { ...fixture.start, authorizationUrl: "not a URL" },
    { ...fixture.start, serverTerms: { clientSecret: "private" } },
  ]) {
    const api = ApplicationApi.create("https://api.example", async () =>
      Response.json({ status: "success", data: invalid }),
    );
    expect(
      (
        await api?.patreonStart(
          "original",
          fixture.start.accountId,
          fixture.flow,
        )
      )?.status,
    ).toBe("error");
  }
  for (const value of [
    { ...fixture.status, phase: "claimed" },
    { ...fixture.status, expiresAt: null },
    { ...fixture.status, expiresAt: "2030-01-01T00:10:00.123456789Z" },
    { ...fixture.status, providerUserId: "private" },
  ])
    expect(patreonLinkStatusSchema.safeParse(value).success).toBe(false);
  const { associationId: omitted, ...missing } = fixture.status;
  expect(omitted).toBeString();
  expect(patreonLinkStatusSchema.safeParse(missing).success).toBe(false);
});
test("Patreon authorization URLs require exact official endpoint, identity scope and safe fixed redirect", () => {
  for (const value of [
    fixture.start.authorizationUrl.replace(
      "www.patreon.com",
      "www.patreon.com.evil.invalid",
    ),
    fixture.start.authorizationUrl + "&state=" + fixture.flow.stateKey,
    fixture.start.authorizationUrl.replace("scope=identity", "scope=campaigns"),
    fixture.start.authorizationUrl + "#state",
    fixture.start.authorizationUrl.replace("/oauth2/authorize", "/other"),
  ])
    expect(patreonAuthorizationUrlSchema.safeParse(value).success).toBe(false);
  const unsafe = new URL(fixture.start.authorizationUrl);
  unsafe.searchParams.set(
    "redirect_uri",
    "https://user:secret@example.invalid/callback",
  );
  expect(patreonAuthorizationUrlSchema.safeParse(unsafe.href).success).toBe(
    false,
  );
});
test("new Patreon authorization uses host random identity and canonical32-byte state without persistence", () => {
  const one = newPatreonLinkRequest(),
    two = newPatreonLinkRequest();
  expect(patreonLinkRequestSchema.safeParse(one).success).toBe(true);
  expect(one.requestId).not.toBe(two.requestId);
  expect(one.stateKey).not.toBe(two.stateKey);
  expect(
    patreonLinkRequestSchema.safeParse({
      ...fixture.flow,
      stateKey: "A".repeat(42) + "B",
    }).success,
  ).toBe(false);
  expect(
    patreonCompleteRequestSchema.safeParse({
      ...fixture.complete,
      code: "short",
    }).success,
  ).toBe(false);
  expect(
    patreonCompleteRequestSchema.safeParse({
      ...fixture.complete,
      accountId: fixture.status.accountId,
    }).success,
  ).toBe(false);
});

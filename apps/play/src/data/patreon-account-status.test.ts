import { expect, test } from "bun:test";
import { ApplicationApi } from "./application-api";
import {
  completionFor,
  parsePatreonReturn,
  type PatreonLinkRecord,
} from "./patreon-link";

const ACCOUNT_ID = "00000000-0000-4000-8000-000000000002";
const TIER_ID = "00000000-0000-4000-8000-000000000077";
const REQUEST_ID = "00000000-0000-4000-8000-000000000088";
const STATE_KEY = "A".repeat(43);

test("patreonAccountStatus parses active supporter, linked-inactive, and unlinked payloads", async () => {
  const payloads = [
    {
      accountId: ACCOUNT_ID,
      linked: true,
      active: true,
      supporter: true,
      tier: { id: TIER_ID, name: "Founder", rank: 2 },
    },
    {
      accountId: ACCOUNT_ID,
      linked: true,
      active: false,
      supporter: false,
      tier: null,
    },
    {
      accountId: ACCOUNT_ID,
      linked: false,
      active: false,
      supporter: false,
      tier: null,
    },
  ];
  for (const data of payloads) {
    const api = ApplicationApi.create("https://api.example", async (url, init) => {
      expect(String(url)).toBe("https://api.example/v1/account/patreon");
      expect(init?.method).toBe("GET");
      return Response.json({ status: "success", data });
    });
    expect(await api?.patreonAccountStatus("bearer-token")).toEqual({
      status: "success",
      data,
    });
  }
});

test("patreonAccountStatus rejects malformed status responses", async () => {
  for (const malformed of [
    { accountId: "not-a-uuid", linked: true, active: true, supporter: true, tier: null },
    { accountId: ACCOUNT_ID, linked: "yes", active: false, supporter: false, tier: null },
    {
      accountId: ACCOUNT_ID,
      linked: true,
      active: true,
      supporter: true,
      tier: { id: TIER_ID, name: "Founder", rank: 1.5 },
    },
  ]) {
    const api = ApplicationApi.create("https://api.example", async () =>
      Response.json({ status: "success", data: malformed }),
    );
    const result = await api?.patreonAccountStatus("bearer-token");
    expect(result?.status).toBe("error");
  }
});

test("parsePatreonReturn rejects oversized queries and handles error/missing params honestly", () => {
  expect(parsePatreonReturn(`?code=${"x".repeat(9000)}&state=${STATE_KEY}`)).toBeNull();
  expect(parsePatreonReturn("?error=access_denied&state=" + STATE_KEY)).toEqual({
    kind: "refused",
  });
  expect(parsePatreonReturn("?code=only-code")).toBeNull();
  expect(parsePatreonReturn("?state=" + STATE_KEY)).toBeNull();
  expect(parsePatreonReturn("")).toBeNull();
});

test("completionFor refuses replay once a saved link record is already linked", () => {
  const linkedRecord: PatreonLinkRecord = {
    version: 1,
    requestId: REQUEST_ID,
    stateKey: STATE_KEY,
    phase: "linked",
  };
  expect(
    completionFor(linkedRecord, {
      kind: "code",
      code: "fresh-code-001",
      state: STATE_KEY,
    }),
  ).toBeNull();
});

import { expect, test } from "bun:test";
import { ApplicationApi } from "./application-api";
import { isAccountPayloadKey } from "./account-storage";
import { ownsDeletionQuery } from "./deletion-recovery";
import {
  completionFor,
  parsePatreonReturn,
  patreonLinkKey,
  readPatreonLink,
  type PatreonLinkRecord,
} from "./patreon-link";

const API = "https://api.example";
const ACCOUNT = "61234567-1234-4234-9234-123456789abc";
const STATE = "AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA";
const record: PatreonLinkRecord = {
  version: 1,
  requestId: "51234567-1234-4234-9234-123456789abc",
  stateKey: STATE,
  phase: "opened",
};
const stored = (value: unknown) => ({
  getItem: (key: string) =>
    key === patreonLinkKey(API, ACCOUNT) ? JSON.stringify(value) : null,
});

test("a return address is read as a code, a refusal, or nothing", () => {
  expect(parsePatreonReturn(`?code=abc&state=${STATE}`)).toEqual({
    kind: "code",
    code: "abc",
    state: STATE,
  });
  expect(parsePatreonReturn("?error=access_denied&state=x")).toEqual({
    kind: "refused",
  });
  expect(parsePatreonReturn("?code=abc")).toBeNull();
  expect(parsePatreonReturn("")).toBeNull();
  expect(parsePatreonReturn(`?code=${"a".repeat(9000)}&state=x`)).toBeNull();
});

test("only a return for the saved, still-open request is completed", () => {
  const code = "patreon_test_code/+=?&";
  const returned = { kind: "code", code, state: STATE } as const;
  expect(completionFor(record, returned)).toEqual({
    requestId: record.requestId,
    stateKey: STATE,
    code,
  });
  // Someone else's state, no saved request, a refusal, or a request already linked.
  expect(
    completionFor(record, { ...returned, state: STATE.replace("A", "B") }),
  ).toBeNull();
  expect(completionFor(null, returned)).toBeNull();
  expect(completionFor(record, { kind: "refused" })).toBeNull();
  expect(completionFor({ ...record, phase: "linked" }, returned)).toBeNull();
  // A code the server would refuse outright is not sent.
  expect(completionFor(record, { ...returned, code: "short" })).toBeNull();
});

test("a saved request is read only when it is whole", () => {
  expect(readPatreonLink(API, ACCOUNT, stored(record))).toEqual(record);
  expect(readPatreonLink(API, ACCOUNT, stored({ ...record, extra: 1 }))).toBe(
    null,
  );
  expect(
    readPatreonLink(API, ACCOUNT, stored({ ...record, stateKey: "short" })),
  ).toBe(null);
  expect(readPatreonLink(API, ACCOUNT, { getItem: () => "{not json" })).toBe(
    null,
  );
  expect(readPatreonLink(API, ACCOUNT, { getItem: () => null })).toBe(null);
});

test("deleting the account takes the saved request and its reads with it", () => {
  expect(isAccountPayloadKey(patreonLinkKey(API, ACCOUNT), API, ACCOUNT)).toBe(
    true,
  );
  const other = "71234567-1234-4234-9234-123456789abc";
  expect(isAccountPayloadKey(patreonLinkKey(API, other), API, ACCOUNT)).toBe(
    false,
  );
  for (const family of ["patreon-link", "own-profile"])
    expect(
      ownsDeletionQuery(
        { queryKey: [family, ACCOUNT, "session"], state: {} as never },
        { accountId: ACCOUNT, subjectId: other },
      ),
    ).toBe(true);
});

test("the own profile read accepts only the caller's own account", async () => {
  const answer = (accountId: string) =>
    ApplicationApi.create(API, (url, init) => {
      expect(String(url)).toBe(`${API}/v1/account/profile`);
      expect(init?.method).toBe("GET");
      return Promise.resolve(
        Response.json({
          status: "success",
          data: { accountId, cardArtProfile: null, supporter: true },
        }),
      );
    })?.ownProfile("original", ACCOUNT);
  expect(await answer(ACCOUNT)).toEqual({
    status: "success",
    data: { accountId: ACCOUNT, cardArtProfile: null, supporter: true },
  });
  expect((await answer("71234567-1234-4234-9234-123456789abc"))?.status).toBe(
    "error",
  );
});

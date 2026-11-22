import { expect, test } from "bun:test";
import { createStore } from "jotai";
import {
  deletionJournalSchema,
  createDeletionJournal,
  deletionJournalKey,
  readDeletionJournal,
  newDeletionSeed,
  deletionObserver,
  deletionStatusHash,
} from "./deletion-journal";

const scope = {
  apiOrigin: "https://api.example",
  authOrigin: "https://auth.example",
  accountId: "10000000-0000-4000-8000-000000000001",
  subjectId: "20000000-0000-4000-8000-000000000002",
};
const requestId = "30000000-0000-4000-8000-000000000003";
const seed = "A".repeat(43);
function record() {
  return deletionJournalSchema.parse({
    version: 1,
    ...scope,
    requestId,
    statusSeed: seed,
    state: "prepared",
  });
}
function local() {
  const values = new Map<string, string>();
  return {
    values,
    getItem: (key: string) => values.get(key) ?? null,
    setItem: (key: string, value: string) => {
      values.set(key, value);
    },
  };
}

test("deletion capability follows the independently calculated fixed UTF8/JSON/SHA256 vector and072decoded-byte hash", async () => {
  const value = record();
  expect(await deletionObserver(value)).toEqual({
    accountId: scope.accountId,
    requestId,
    statusKey: "zrHxUQLDb7Fh-oStDhMd0zFb4O2Ue8a42SrnPevPWZc",
  });
  expect(await deletionStatusHash(value)).toBe(
    "8d55513f344f046571961a6b8895f99a639170ef206e99fe2f8ad4706a3dc3d5",
  );
  const accepted = deletionJournalSchema.parse({
    ...value,
    state: "accepted",
    ack: { resourceId: scope.accountId },
  });
  expect(await deletionObserver(accepted)).toEqual(
    await deletionObserver(value),
  );
  expect(newDeletionSeed()).toMatch(/^[A-Za-z0-9_-]{42}[AEIMQUYcgkosw048]$/);
});

test("every observer identity and seed is bound; valid-looking subject substitution cannot reuse the accepted nonce", async () => {
  const value = record();
  const acceptedHash = await deletionStatusHash(value);
  for (const patch of [
    { accountId: crypto.randomUUID() },
    { subjectId: crypto.randomUUID() },
    { requestId: crypto.randomUUID() },
    { authOrigin: "https://other-auth.example" },
    { apiOrigin: "https://other-api.example" },
    { statusSeed: newDeletionSeed() },
  ]) {
    const changed = deletionJournalSchema.parse({ ...value, ...patch });
    expect(await deletionStatusHash(changed)).not.toBe(acceptedHash);
    expect((await deletionObserver(changed)).statusKey).not.toBe(
      (await deletionObserver(value)).statusKey,
    );
  }
});

test("journal persists immutable consent before acknowledgment, retains exact identity on retry and accepts only its account ack", () => {
  const storage = local();
  const atoms = createStore();
  const store = createDeletionJournal(scope, () => storage);
  const prepared = atoms.set(store.actionAtom, {
    type: "prepare",
    requestId,
    statusSeed: seed,
  });
  expect(prepared).toEqual(record());
  expect(JSON.parse(storage.values.get(store.key)!)).toEqual(record());
  const repeated = atoms.set(store.actionAtom, {
    type: "prepare",
    requestId: crypto.randomUUID(),
    statusSeed: newDeletionSeed(),
  });
  expect(repeated).toEqual(prepared);
  if (!prepared) throw new Error("Expected prepared record");
  expect(
    atoms.set(store.actionAtom, {
      type: "accept",
      expected: prepared,
      resourceId: crypto.randomUUID(),
    }),
  ).toBeNull();
  const accepted = atoms.set(store.actionAtom, {
    type: "accept",
    expected: prepared,
    resourceId: scope.accountId,
  });
  expect(accepted?.state).toBe("accepted");
  expect(
    atoms.set(store.actionAtom, {
      type: "accept",
      expected: prepared,
      resourceId: scope.accountId,
    }),
  ).toEqual(accepted);
  expect(
    atoms.set(store.actionAtom, {
      type: "accept",
      expected: { ...prepared, statusSeed: newDeletionSeed() },
      resourceId: scope.accountId,
    }),
  ).toBeNull();
  expect(
    atoms.set(store.actionAtom, {
      type: "prepare",
      requestId: crypto.randomUUID(),
      statusSeed: newDeletionSeed(),
    }),
  ).toEqual(accepted);
  const remounted = createDeletionJournal(scope, () => storage);
  expect(createStore().set(remounted.actionAtom, { type: "refresh" })).toEqual(
    accepted,
  );
});

test("corrupt, foreign and mismatched ack records stay untouched; dropped or denied writes cannot prepare consent", () => {
  const key = deletionJournalKey(scope.apiOrigin, scope.accountId);
  for (const raw of [
    "corrupt bytes",
    JSON.stringify({ ...record(), subjectId: crypto.randomUUID() }),
    JSON.stringify({
      ...record(),
      state: "accepted",
      ack: { resourceId: crypto.randomUUID() },
    }),
  ]) {
    const storage = local();
    storage.values.set(key, raw);
    const store = createDeletionJournal(scope, () => storage);
    expect(
      createStore().set(store.actionAtom, {
        type: "prepare",
        requestId,
        statusSeed: seed,
      }),
    ).toBeNull();
    expect(storage.values.get(key)).toBe(raw);
  }
  for (const setItem of [
    () => {},
    () => {
      throw new Error("quota fixture");
    },
  ]) {
    const storage = { getItem: () => null, setItem };
    const store = createDeletionJournal(scope, () => storage);
    expect(
      createStore().set(store.actionAtom, {
        type: "prepare",
        requestId,
        statusSeed: seed,
      }),
    ).toBeNull();
  }
});

test("root record reader is exact API/Auth/key scoped and bounds corrupt payloads without erasing them", () => {
  const storage = local();
  const key = deletionJournalKey(scope.apiOrigin, scope.accountId);
  storage.values.set(key, JSON.stringify(record()));
  expect(
    readDeletionJournal(key, scope.apiOrigin, scope.authOrigin, storage),
  ).toEqual(record());
  for (const [candidate, api, auth] of [
    [key, "https://other-api.example", scope.authOrigin],
    [key, scope.apiOrigin, "https://other-auth.example"],
    [
      deletionJournalKey(scope.apiOrigin, crypto.randomUUID()),
      scope.apiOrigin,
      scope.authOrigin,
    ],
  ]) {
    storage.values.set(candidate!, JSON.stringify(record()));
    expect(() =>
      readDeletionJournal(candidate!, api!, auth!, storage),
    ).toThrow();
  }
  storage.values.set(key, " ".repeat(8193));
  expect(() =>
    readDeletionJournal(key, scope.apiOrigin, scope.authOrigin, storage),
  ).toThrow();
  expect(storage.values.get(key)?.length).toBe(8193);
});

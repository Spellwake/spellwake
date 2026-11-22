import { test, expect } from "bun:test";
import { createStore } from "jotai";
import { createCheckoutIntentStore } from "./checkout-intent";
const account = "7b0ab1bc-8bf0-4159-b951-4d15ca766c1d";
const cosmetic = "8b0ab1bc-8bf0-4159-b951-4d15ca766c1d";
const purchase = "9b0ab1bc-8bf0-4159-b951-4d15ca766c1d";
test("rejected request replacement is durable and cannot overwrite a stale or acknowledged intent", () => {
  const values = new Map<string, string>();
  const storage = {
    getItem: (key: string) => values.get(key) ?? null,
    setItem: (key: string, value: string) => {
      values.set(key, value);
    },
  };
  const first = createCheckoutIntentStore(
    "https://api.example",
    account,
    cosmetic,
    () => storage,
  );
  const state = createStore();
  const original = state.set(first.actionAtom, { type: "prepare" })!;
  const next = state.set(first.actionAtom, {
    type: "replace_rejected",
    requestId: original.requestId,
  })!;
  expect(next.requestId).not.toBe(original.requestId);
  const otherTab = createCheckoutIntentStore(
    "https://api.example",
    account,
    cosmetic,
    () => storage,
  );
  expect(state.set(otherTab.actionAtom, { type: "prepare" })).toEqual(next);
  expect(
    state.set(first.actionAtom, {
      type: "replace_rejected",
      requestId: original.requestId,
    }),
  ).toBeNull();
  state.set(otherTab.actionAtom, {
    type: "acknowledge",
    requestId: next.requestId,
    purchaseId: purchase,
  });
  expect(
    state.set(first.actionAtom, {
      type: "replace_rejected",
      requestId: next.requestId,
    }),
  ).toBeNull();
  expect(state.set(first.actionAtom, { type: "prepare" })?.purchaseId).toBe(
    purchase,
  );
});
test("checkout request survives remount and unknown acknowledgment, and products have independent identities", () => {
  const values = new Map<string, string>();
  const storage = {
    getItem: (key: string) => values.get(key) ?? null,
    setItem: (key: string, value: string) => {
      values.set(key, value);
    },
  };
  const first = createCheckoutIntentStore(
    "https://api.example",
    account,
    cosmetic,
    () => storage,
  );
  const state = createStore();
  const prepared = state.set(first.actionAtom, { type: "prepare" });
  expect(prepared).not.toBeNull();
  expect(values.size).toBe(1);
  const remounted = createCheckoutIntentStore(
    "https://api.example",
    account,
    cosmetic,
    () => storage,
  );
  expect(state.set(remounted.actionAtom, { type: "prepare" })?.requestId).toBe(
    prepared?.requestId,
  );
  expect(
    state.set(remounted.actionAtom, {
      type: "acknowledge",
      requestId: prepared!.requestId,
      purchaseId: purchase,
    })?.purchaseId,
  ).toBe(purchase);
  expect(state.set(first.actionAtom, { type: "prepare" })?.purchaseId).toBe(
    purchase,
  );
  expect(
    state.set(first.actionAtom, {
      type: "replace_settled",
      requestId: prepared!.requestId,
      purchaseId: cosmetic,
    }),
  ).toBeNull();
  const replacement = state.set(first.actionAtom, {
    type: "replace_settled",
    requestId: prepared!.requestId,
    purchaseId: purchase,
  });
  expect(replacement?.requestId).not.toBe(prepared?.requestId);
  expect(replacement?.purchaseId).toBeNull();
  const other = createCheckoutIntentStore(
    "https://api.example",
    account,
    purchase,
    () => storage,
  );
  expect(state.set(other.actionAtom, { type: "prepare" })?.requestId).not.toBe(
    prepared?.requestId,
  );
});
test("silent failed writes and corrupt recovery identities block dispatch preparation without erasing evidence", () => {
  const silent = createCheckoutIntentStore(
    "https://api.example",
    account,
    cosmetic,
    () => ({ getItem: () => null, setItem: () => undefined }),
  );
  const state = createStore();
  expect(state.set(silent.actionAtom, { type: "prepare" })).toBeNull();
  expect(state.get(silent.stateAtom).status).toBe("unavailable");
  let writes = 0;
  const corrupt = createCheckoutIntentStore(
    "https://api.example",
    account,
    cosmetic,
    () => ({
      getItem: () => "{broken",
      setItem: () => {
        writes++;
      },
    }),
  );
  expect(state.set(corrupt.actionAtom, { type: "prepare" })).toBeNull();
  expect(writes).toBe(0);
});

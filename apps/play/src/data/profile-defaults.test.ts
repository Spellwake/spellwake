import { expect, test } from "bun:test";
import { createStore } from "jotai";
import {
  changeProfileDefaultsSchema,
  profileDefaultsDispositionSchema,
} from "@spellwake/application-contracts";
import { createProfileDefaultsJournal } from "./profile-defaults";

test("defaults journal preserves both snapshots and refuses another owner's persisted attempt", () => {
  const origin = "https://api.example";
  const account = "30000000-0000-4000-8000-000000000001";
  const other = "40000000-0000-4000-8000-000000000001";
  const values = new Map<string, string>();
  const storage = {
    getItem: (key: string) => values.get(key) ?? null,
    setItem: (key: string, value: string) => {
      values.set(key, value);
    },
  };
  const store = createProfileDefaultsJournal(origin, account, () => storage);
  const atoms = createStore();
  const appearance = {
    backId: account,
    sleeveId: account,
    diceId: account,
    playmatId: null,
  };
  const command = changeProfileDefaultsSchema.parse({
    requestId: crypto.randomUUID(),
    expected: appearance,
    appearance: { ...appearance, diceId: other },
  });
  const first = atoms.set(store.actionAtom, {
    type: "prepare",
    expected: null,
    command,
  });
  expect(first?.state).toBe("unresolved");
  const persisted = values.get(store.key);
  command.appearance.diceId = account;
  expect(values.get(store.key)).toBe(persisted);
  const reloaded = createProfileDefaultsJournal(origin, account, () => storage);
  const recovered = createStore().set(reloaded.actionAtom, { type: "refresh" });
  expect(recovered).toEqual(first);
  expect(
    atoms.set(store.actionAtom, { type: "prepare", expected: first, command }),
  ).toBeNull();
  expect(values.get(store.key)).toBe(persisted);
  if (!recovered) throw new Error("expected saved defaults");
  const terminal = profileDefaultsDispositionSchema.parse({
    state: "accepted",
    response: { status: "success", data: { resourceId: other } },
  });
  expect(
    atoms.set(store.actionAtom, {
      type: "observe",
      command: recovered.command,
      disposition: terminal,
    })?.state,
  ).toBe("accepted");
  // The request is terminal, but the same ID cannot acquire new selected terms.
  const accepted = atoms.set(store.actionAtom, { type: "refresh" });
  expect(
    atoms.set(store.actionAtom, {
      type: "prepare",
      expected: accepted,
      command,
    }),
  ).toBeNull();
  const foreign = createProfileDefaultsJournal(origin, other, () => storage);
  expect(foreign.key).not.toBe(store.key);
  if (!persisted) throw new Error("expected retained bytes");
  values.set(foreign.key, persisted);
  expect(createStore().set(foreign.actionAtom, { type: "refresh" })).toBeNull();
  expect(values.get(foreign.key)).toBe(persisted);
});

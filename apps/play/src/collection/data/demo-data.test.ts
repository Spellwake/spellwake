import { describe, expect, test } from "bun:test";
import { createStore } from "jotai";
import { parseDeckText } from "./deck-import";
import {
  createDemoStore,
  DEMO_STORAGE_KEY,
  type DemoStorage,
} from "./demo-store";
import { demoLibrarySchema } from "./demo-model";
import { createDemoSeed, createStressDecks } from "./demo-seed";

function memoryStorage(initial: string | null = null) {
  let value = initial;
  let failWrites = false;
  const storage: DemoStorage = {
    getItem: () => value,
    setItem: (key, next) => {
      expect(key).toBe(DEMO_STORAGE_KEY);
      if (failWrites) throw new Error("quota");
      value = next;
    },
  };
  return {
    storage,
    read: () => value,
    fail: (next: boolean) => {
      failWrites = next;
    },
  };
}

describe("demo imports", () => {
  test("retains exact large quantities, section identity, and original Arena lines", () => {
    const result = parseDeckText(
      "Deck\r\n100000000000000000001 Island (DMU) 267\r\nSideboard\r\n4x Negate\r\nCommander:\r\n1 Atraxa, Praetors' Voice\r\nConsidering\r\n2 Forest",
    );
    expect(result.status).toBe("success");
    if (result.status !== "success") return;
    expect(
      result.entries.map(({ quantity, name, section }) => ({
        quantity,
        name,
        section,
      })),
    ).toEqual([
      { quantity: "100000000000000000001", name: "Island", section: "main" },
      { quantity: "4", name: "Negate", section: "sideboard" },
      { quantity: "1", name: "Atraxa, Praetors' Voice", section: "commander" },
      { quantity: "2", name: "Forest", section: "considering" },
    ]);
    expect(result.entries[0]?.sourceText).toBe(
      "100000000000000000001 Island (DMU) 267",
    );
  });
  test("reports every invalid line instead of accepting a partial import", () => {
    expect(
      parseDeckText(
        "4 Island\n0 Negate\n01 Forest\nunknown header\n1.5 Plains",
      ),
    ).toMatchObject({
      status: "fail",
      errors: [{ line: 2 }, { line: 3 }, { line: 4 }, { line: 5 }],
    });
    expect(parseDeckText("Deck\nSideboard").status).toBe("fail");
  });
  test("inherited object properties are not accepted as section headings", () => {
    for (const header of ["constructor", "__proto__", "toString"]) {
      expect(parseDeckText(`${header}\nDeck\n4 Island`)).toMatchObject({
        status: "fail",
        errors: [{ line: 1 }],
      });
    }
  });
});

describe("demo persistence", () => {
  test("create and edit survive a new store and stale revisions cannot replace saved content", () => {
    const memory = memoryStorage();
    const atoms = createDemoStore(() => memory.storage);
    const store = createStore();
    expect(store.get(atoms.libraryAtom).library?.decks).toHaveLength(6);
    expect(memory.read()).toBeNull();
    const created = store.set(atoms.actionAtom, {
      type: "create",
      input: { name: "My deck", format: "Modern" },
    });
    if (created?.status !== "success") throw new Error("create failed");
    const saved = store.set(atoms.actionAtom, {
      type: "update",
      id: created.deck.id,
      expectedRevision: 1,
      input: { name: "Renamed", format: "Modern" },
    });
    expect(saved).toMatchObject({ status: "success", deck: { revision: 2 } });
    expect(
      store.set(atoms.actionAtom, {
        type: "update",
        id: created.deck.id,
        expectedRevision: 1,
        input: { name: "Stale", format: "Modern" },
      }),
    ).toMatchObject({ status: "fail", reason: "conflict" });
    const reopened = createDemoStore(() => memory.storage);
    expect(
      createStore().get(reopened.libraryAtom).library?.decks.at(-1)?.name,
    ).toBe("Renamed");
  });
  test("storage failure rolls back the mutation and preserves saved bytes until explicit retry", () => {
    const memory = memoryStorage(JSON.stringify(createDemoSeed()));
    const original = memory.read();
    const atoms = createDemoStore(() => memory.storage);
    const store = createStore();
    memory.fail(true);
    expect(
      store.set(atoms.actionAtom, {
        type: "create",
        input: { name: "Unsaved", format: "Standard" },
      }),
    ).toMatchObject({ status: "fail", reason: "storage" });
    expect(store.get(atoms.libraryAtom)).toMatchObject({ status: "error" });
    expect(store.get(atoms.libraryAtom).library?.decks).toHaveLength(6);
    expect(memory.read()).toBe(original);
    memory.fail(false);
    store.set(atoms.actionAtom, { type: "retry" });
    expect(store.get(atoms.libraryAtom).status).toBe("ready");
    expect(memory.read()).toBe(original);
  });
  test("corrupt saved data is not silently replaced with seed data", () => {
    const memory = memoryStorage("{broken}");
    const atoms = createDemoStore(() => memory.storage);
    const store = createStore();
    expect(store.get(atoms.libraryAtom)).toMatchObject({
      status: "error",
      library: null,
    });
    expect(
      store.set(atoms.actionAtom, {
        type: "create",
        input: { name: "Blocked", format: "Standard" },
      }),
    ).toMatchObject({ status: "fail", reason: "storage" });
    expect(memory.read()).toBe("{broken}");
    store.set(atoms.actionAtom, { type: "reset" });
    expect(store.get(atoms.libraryAtom).status).toBe("ready");
    expect(
      demoLibrarySchema.parse(JSON.parse(memory.read() ?? "null")).decks,
    ).toHaveLength(6);
  });
  test("another store's newer revision is detected even before a storage event", () => {
    const memory = memoryStorage(JSON.stringify(createDemoSeed()));
    const first = createDemoStore(() => memory.storage);
    const second = createDemoStore(() => memory.storage);
    const one = createStore();
    const two = createStore();
    const deck = two.get(second.libraryAtom).library?.decks[0];
    if (!deck) throw new Error("seed missing");
    expect(
      one.set(first.actionAtom, {
        type: "update",
        id: deck.id,
        expectedRevision: 1,
        input: { name: "New title", format: "Standard" },
      }),
    ).toMatchObject({ status: "success" });
    expect(
      two.set(second.actionAtom, {
        type: "update",
        id: deck.id,
        expectedRevision: 1,
        input: { name: "Stale title", format: "Standard" },
      }),
    ).toMatchObject({ reason: "conflict" });
    expect(two.get(second.libraryAtom).library?.decks[0]?.name).toBe(
      "New title",
    );
  });
  test("stress examples are valid but never modify storage", () => {
    expect(
      demoLibrarySchema.parse({ version: 1, decks: createStressDecks() }).decks,
    ).toHaveLength(160);
  });
});

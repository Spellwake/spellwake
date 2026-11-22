import { describe, expect, test } from "bun:test";
import { createStore } from "jotai";
import { createDemoStore, type DemoStorage } from "./demo-store";

function memoryStorage(): DemoStorage {
  let value: string | null = null;
  return {
    getItem: () => value,
    setItem: (_key, next) => {
      value = next;
    },
  };
}

describe("demo deck deletion", () => {
  test("removes only the named deck and refuses a stale revision", () => {
    const store = createStore();
    const demo = createDemoStore(memoryStorage);
    const decksNow = () => store.get(demo.libraryAtom).library?.decks ?? [];
    const first = decksNow()[0];
    if (!first) throw new Error("The seed has no decks.");
    const before = decksNow().length;

    const stale = store.set(demo.actionAtom, {
      type: "delete",
      id: first.id,
      expectedRevision: first.revision + 1,
    });
    expect(stale).toMatchObject({ status: "fail", reason: "conflict" });
    expect(decksNow()).toHaveLength(before);

    const removed = store.set(demo.actionAtom, {
      type: "delete",
      id: first.id,
      expectedRevision: first.revision,
    });
    expect(removed).toMatchObject({ status: "success" });
    expect(decksNow()).toHaveLength(before - 1);
    expect(decksNow().some((deck) => deck.id === first.id)).toBe(false);
  });
});

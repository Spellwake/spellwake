import { atom } from "jotai";
import { atomWithStorage } from "jotai/utils";
import { z } from "zod";

const durableChangeEvent = "spellwake.durable-storage-changed.v1";
/** Notify same-window cells to read actual bytes after a verified external transition. */
export function notifyDurableStorage(keys: readonly string[]): void {
  if (typeof window !== "undefined" && keys.length)
    window.dispatchEvent(
      new CustomEvent(durableChangeEvent, { detail: [...keys] }),
    );
}
/** Event payloads only select a re-read; they never supply persisted values. */
export function subscribeDurableStorage(
  key: string,
  changed: () => void,
): () => void {
  if (typeof window === "undefined") return () => {};
  const native = (event: StorageEvent) => {
    if (event.key === key || event.key === null) changed();
  };
  const local = (event: Event) => {
    if (
      event instanceof CustomEvent &&
      Array.isArray(event.detail) &&
      event.detail.some((candidate: unknown) => candidate === key)
    )
      changed();
  };
  window.addEventListener("storage", native);
  window.addEventListener(durableChangeEvent, local);
  return () => {
    window.removeEventListener("storage", native);
    window.removeEventListener(durableChangeEvent, local);
  };
}

export type DurableState<T> =
  | { status: "empty" }
  | { status: "saved"; value: T }
  | { status: "unavailable" };

/** Validated non-erasable storage; domain transitions and cross-tab locks belong to callers. */
export function createDurableCell<T extends object>(
  key: string,
  schema: z.ZodType<T>,
  getStorage: () => Pick<Storage, "getItem" | "setItem">,
) {
  let isRefreshing = false;
  function read(): DurableState<T> {
    try {
      const raw = getStorage().getItem(key);
      if (raw === null) return { status: "empty" };
      const parsed = schema.safeParse(JSON.parse(raw));
      if (parsed.success) return { status: "saved", value: parsed.data };
    } catch {
      /* Preserve the unreadable bytes. */
    }
    console.warn("durable_cell code=storage_unavailable");
    return { status: "unavailable" };
  }
  const stateAtom = atomWithStorage<DurableState<T>>(
    key,
    { status: "empty" },
    {
      getItem: () => read(),
      setItem: (_key, state) => {
        if (isRefreshing) return;
        if (state.status !== "saved")
          throw new Error("Durable record cannot be erased.");
        const raw = JSON.stringify(schema.parse(state.value));
        const storage = getStorage();
        storage.setItem(key, raw);
        if (storage.getItem(key) !== raw)
          throw new Error("Durable write was not confirmed.");
      },
      removeItem: () => {
        throw new Error("Durable record cannot be erased.");
      },
      subscribe: (_key, callback) => {
        return subscribeDurableStorage(key, () => callback(read()));
      },
    },
    { getOnInit: true },
  );
  const refreshAtom = atom(null, (_get, set, state: DurableState<T>) => {
    isRefreshing = true;
    try {
      set(stateAtom, state);
    } finally {
      isRefreshing = false;
    }
  });
  const writeAtom = atom(null, (_get, set, input: T): T | null => {
    try {
      const value = schema.parse(input);
      set(stateAtom, { status: "saved", value });
      // The caller never receives the mutable object owned by the published atom.
      return schema.parse(value);
    } catch {
      console.warn("durable_cell code=save_unconfirmed");
      set(refreshAtom, { status: "unavailable" });
      return null;
    }
  });
  return { key, stateAtom, read, refreshAtom, writeAtom };
}

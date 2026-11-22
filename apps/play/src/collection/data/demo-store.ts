import { atom, createStore, useAtomValue, useSetAtom } from "jotai";
import { atomWithStorage } from "jotai/utils";
import { parseDeckText } from "./deck-import";
import {
  demoDeckInputSchema,
  demoDeckSchema,
  demoLibrarySchema,
  type DemoDeck,
  type DemoDeckInput,
  type DemoLibrary,
} from "./demo-model";
import { createDemoSeed } from "./demo-seed";

export const DEMO_STORAGE_KEY = "spellwake:collection:demo:v1";
export type DemoMutationResult =
  | { status: "success"; deck: DemoDeck }
  | {
      status: "fail";
      reason: "validation" | "conflict" | "storage";
      message: string;
    };
export type DemoLibraryState =
  | { status: "ready"; library: DemoLibrary }
  | { status: "error"; library: DemoLibrary | null; message: string };
export type DemoStorage = Pick<Storage, "getItem" | "setItem">;
type DemoAction =
  | { type: "create"; input: DemoDeckInput }
  | {
      type: "update";
      id: string;
      expectedRevision: number;
      input: DemoDeckInput;
    }
  | { type: "delete"; id: string; expectedRevision: number }
  | { type: "retry" }
  | { type: "reset" };

/** Invalid or inaccessible storage is retained intact until the user explicitly resets it. */
export function readDemoStorage(
  storage: DemoStorage,
  key = DEMO_STORAGE_KEY,
  seed = createDemoSeed,
): DemoLibraryState {
  try {
    const raw = storage.getItem(key);
    if (raw === null) return { status: "ready", library: seed() };
    const parsed = demoLibrarySchema.safeParse(JSON.parse(raw));
    if (parsed.success) return { status: "ready", library: parsed.data };
    console.error("Saved demo library failed validation.");
    return {
      status: "error",
      library: null,
      message: "Saved demo data could not be read. Retry or reset the demo.",
    };
  } catch {
    console.error("Demo library storage could not be read.");
    return {
      status: "error",
      library: null,
      message:
        "Demo storage is unavailable. Retry when browser storage is available.",
    };
  }
}

function unavailableState(): DemoLibraryState {
  return {
    status: "error",
    library: null,
    message: "Demo storage is unavailable. Allow browser storage and retry.",
  };
}

/** The injectable storage keeps persistence failure and revision behavior testable. */
export function createDemoStore(
  getStorage: () => DemoStorage,
  key = DEMO_STORAGE_KEY,
  seed = createDemoSeed,
) {
  let isRefreshing = false;
  function read(): DemoLibraryState {
    try {
      return readDemoStorage(getStorage(), key, seed);
    } catch {
      console.error("Demo storage access was denied.");
      return unavailableState();
    }
  }
  const adapter = {
    getItem: (): DemoLibraryState => read(),
    setItem: (_key: string, state: DemoLibraryState): void => {
      if (!isRefreshing && state.status === "ready")
        getStorage().setItem(key, JSON.stringify(state.library));
    },
    removeItem: (): void => {
      /* Reset is an explicit validated library write. */
    },
    subscribe: (
      _key: string,
      callback: (state: DemoLibraryState) => void,
    ): (() => void) => {
      if (typeof window === "undefined") return () => undefined;
      const listener = (event: StorageEvent): void => {
        if (event.key === key || event.key === null) callback(read());
      };
      window.addEventListener("storage", listener);
      return () => window.removeEventListener("storage", listener);
    },
  };
  const libraryAtom = atomWithStorage<DemoLibraryState>(
    key,
    { status: "ready", library: seed() },
    adapter,
    { getOnInit: true },
  );
  const actionAtom = atom(
    null,
    (get, set, action: DemoAction): DemoMutationResult | null => {
      const prior = get(libraryAtom);
      const refresh = (state: DemoLibraryState): void => {
        isRefreshing = true;
        try {
          set(libraryAtom, state);
        } finally {
          isRefreshing = false;
        }
      };
      const write = (library: DemoLibrary): boolean => {
        try {
          set(libraryAtom, { status: "ready", library });
          return true;
        } catch {
          console.error("Demo library could not be saved.");
          // atomWithStorage publishes before its synchronous adapter writes. Roll back
          // within this atom transaction so a failed write never becomes a saved deck.
          refresh({
            status: "error",
            library: prior.library,
            message:
              "Changes were not saved. Demo storage is unavailable; retry before saving again.",
          });
          return false;
        }
      };
      if (action.type === "reset") {
        write(seed());
        return null;
      }
      if (action.type === "retry") {
        refresh(read());
        return null;
      }
      if (prior.status === "error")
        return { status: "fail", reason: "storage", message: prior.message };
      const latest = read();
      if (latest.status === "error") {
        refresh({ ...latest, library: prior.library });
        return { status: "fail", reason: "storage", message: latest.message };
      }
      if (action.type === "delete") {
        const existing = latest.library.decks.find(
          (item) => item.id === action.id,
        );
        if (!existing || existing.revision !== action.expectedRevision) {
          refresh(latest);
          return {
            status: "fail",
            reason: "conflict",
            message: "This demo deck changed. Reopen it before deleting.",
          };
        }
        return write({
          version: 1,
          decks: latest.library.decks.filter((item) => item.id !== action.id),
        })
          ? { status: "success", deck: existing }
          : {
              status: "fail",
              reason: "storage",
              message: "The deck was not deleted.",
            };
      }
      const parsed = demoDeckInputSchema.safeParse(action.input);
      if (!parsed.success)
        return {
          status: "fail",
          reason: "validation",
          message: parsed.error.issues[0]?.message ?? "Check the deck details.",
        };
      const now = new Date().toISOString();
      let deck: DemoDeck;
      if (action.type === "update") {
        const existing = latest.library.decks.find(
          (item) => item.id === action.id,
        );
        if (!existing || existing.revision !== action.expectedRevision) {
          refresh(latest);
          return {
            status: "fail",
            reason: "conflict",
            message: "This demo deck changed. Reopen it before saving.",
          };
        }
        const updated = demoDeckSchema.safeParse({
          ...parsed.data,
          id: existing.id,
          revision: existing.revision + 1,
          createdAt: existing.createdAt,
          updatedAt: now,
        });
        if (!updated.success)
          return {
            status: "fail",
            reason: "validation",
            message:
              updated.error.issues[0]?.message ?? "Check the deck details.",
          };
        deck = updated.data;
      } else {
        const created = demoDeckSchema.safeParse({
          ...parsed.data,
          id: crypto.randomUUID(),
          revision: 1,
          createdAt: now,
          updatedAt: now,
        });
        if (!created.success)
          return {
            status: "fail",
            reason: "validation",
            message:
              created.error.issues[0]?.message ?? "Check the deck details.",
          };
        deck = created.data;
      }
      const library = demoLibrarySchema.safeParse({
        version: 1,
        decks:
          action.type === "create"
            ? [...latest.library.decks, deck]
            : latest.library.decks.map((item) =>
                item.id === deck.id ? deck : item,
              ),
      });
      if (!library.success)
        return {
          status: "fail",
          reason: "validation",
          message: "The demo library is full or contains invalid data.",
        };
      return write(library.data)
        ? { status: "success", deck }
        : {
            status: "fail",
            reason: "storage",
            message:
              "The deck was not saved. Retry browser storage before saving again.",
          };
    },
  );
  return { libraryAtom, actionAtom };
}

/** Isolated, synchronous access to the same validated library and revision-checked writes as the demo UI. */
export function createSessionDeckLibrary(key: string, seed: () => DemoLibrary) {
  const store = createStore();
  const atoms = createDemoStore(() => window.sessionStorage, key, seed);
  return {
    read: () => store.get(atoms.libraryAtom),
    write: (action: Extract<DemoAction, { type: "create" | "update" }>) => {
      store.set(atoms.actionAtom, { type: "retry" });
      return store.set(atoms.actionAtom, action);
    },
  };
}

const demoStore = createDemoStore(() => window.localStorage);

/** Explicit local demo data, entirely separate from account/API persistence. */
export function useDemoDecks() {
  const state = useAtomValue(demoStore.libraryAtom);
  const dispatch = useSetAtom(demoStore.actionAtom);
  const createDeck = (input: DemoDeckInput): DemoMutationResult =>
    dispatch({ type: "create", input }) ?? {
      status: "fail",
      reason: "storage",
      message: "The deck was not saved.",
    };
  return {
    decks: state.library?.decks ?? [],
    error: state.status === "error" ? state.message : null,
    createDeck,
    importDeck: (input: DemoDeckInput, text: string): DemoMutationResult => {
      const parsed = parseDeckText(text);
      if (parsed.status === "fail")
        return {
          status: "fail",
          reason: "validation",
          message: parsed.errors
            .map((error) => `Line ${error.line}: ${error.message}`)
            .join("\n"),
        };
      return createDeck({
        ...input,
        entries: parsed.entries,
        sourceText: text,
      });
    },
    updateDeck: (
      id: string,
      expectedRevision: number,
      input: DemoDeckInput,
    ): DemoMutationResult =>
      dispatch({ type: "update", id, expectedRevision, input }) ?? {
        status: "fail",
        reason: "storage",
        message: "The deck was not saved.",
      },
    retryStorage: (): void => {
      dispatch({ type: "retry" });
    },
    resetDemo: (): void => {
      dispatch({ type: "reset" });
    },
  };
}

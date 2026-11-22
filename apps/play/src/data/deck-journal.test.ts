import { expect, test } from "bun:test";
import { createStore } from "jotai";
import { z } from "zod";
import {
  deckCommandSchema,
  deckCommandDispositionSchema,
} from "@spellwake/application-contracts";
import { createDeckJournalStore } from "./deck-journal";
import { createDurableCell } from "./durable-cell";

const account = "7b0ab1bc-8bf0-4159-b951-4d15ca766c1d";
const deck = "8b0ab1bc-8bf0-4159-b951-4d15ca766c1d";
const revision = "9b0ab1bc-8bf0-4159-b951-4d15ca766c1d";
const origin = "https://api.example";
function storage() {
  const values = new Map<string, string>();
  return {
    values,
    getItem: (key: string) => values.get(key) ?? null,
    setItem: (key: string, value: string) => {
      values.set(key, value);
    },
  };
}
function save(requestId = crypto.randomUUID()) {
  return deckCommandSchema.parse({
    kind: "save",
    command: {
      requestId,
      deckId: deck,
      expectedRevisionId: null,
      entries: [
        {
          definitionId: account,
          printingId: null,
          section: "main",
          quantity: "9007199254740993",
        },
      ],
    },
  });
}
const accepted = deckCommandDispositionSchema.parse({
  state: "accepted",
  response: { status: "success", data: { resourceId: revision } },
});
const rejected = deckCommandDispositionSchema.parse({
  state: "rejected",
  response: {
    status: "fail",
    data: { revision: "deck changed; reload before saving" },
  },
});

test("deck journal snapshots terms across mutation, remount and unknown disposition", () => {
  const local = storage();
  const state = createStore();
  const first = createDeckJournalStore(
    origin,
    account,
    { kind: "save", deckId: deck },
    () => local,
  );
  const command = save();
  const prepared = state.set(first.actionAtom, {
    type: "prepare",
    expected: null,
    command,
  })!;
  const original = JSON.stringify(prepared.command);
  if (command.kind === "save") command.command.entries[0]!.quantity = "2";
  if (prepared.command.kind === "save")
    prepared.command.command.entries[0]!.quantity = "3";
  const remounted = createDeckJournalStore(
    origin,
    account,
    { kind: "save", deckId: deck },
    () => local,
  );
  const retained = state.set(remounted.actionAtom, { type: "refresh" })!;
  expect(JSON.stringify(retained.command)).toBe(original);
  const unknown = state.set(remounted.actionAtom, {
    type: "observe",
    command: retained.command,
    disposition: { state: "unknown", response: null },
  })!;
  expect(unknown.state).toBe("unresolved");
  expect(JSON.stringify(unknown.command)).toBe(original);
  expect(
    state.set(remounted.actionAtom, {
      type: "prepare",
      expected: retained,
      command: save(),
    }),
  ).toBeNull();
  expect(local.values.size).toBe(1);
});

test("terminal proofs are monotonic, stale observed state cannot replace a command, and later callbacks are fenced", () => {
  const local = storage();
  const state = createStore();
  const journal = createDeckJournalStore(
    origin,
    account,
    { kind: "save", deckId: deck },
    () => local,
  );
  const original = state.set(journal.actionAtom, {
    type: "prepare",
    expected: null,
    command: save(),
  })!;
  const terminal = state.set(journal.actionAtom, {
    type: "observe",
    command: original.command,
    disposition: accepted,
  })!;
  expect(terminal.state).toBe("accepted");
  expect(
    state.set(journal.actionAtom, {
      type: "observe",
      command: original.command,
      disposition: accepted,
    }),
  ).toEqual(terminal);
  expect(
    state.set(journal.actionAtom, {
      type: "observe",
      command: original.command,
      disposition: rejected,
    }),
  ).toBeNull();
  expect(
    state.set(journal.actionAtom, {
      type: "prepare",
      expected: original,
      command: save(),
    }),
  ).toBeNull();
  const next = state.set(journal.actionAtom, {
    type: "prepare",
    expected: terminal,
    command: save(),
  })!;
  expect(next.state).toBe("unresolved");
  expect(next.command.command.requestId).not.toBe(
    original.command.command.requestId,
  );
  expect(
    state.set(journal.actionAtom, {
      type: "observe",
      command: original.command,
      disposition: accepted,
    }),
  ).toBeNull();
  expect(
    state.set(journal.actionAtom, {
      type: "observe",
      command: next.command,
      disposition: rejected,
    })?.state,
  ).toBe("rejected");
  for (const data of [
    { session: "unauthorized" },
    { request: "conflict" },
    { account: "unavailable" },
  ])
    expect(
      deckCommandDispositionSchema.safeParse({
        state: "rejected",
        response: { status: "fail", data },
      }).success,
    ).toBe(false);
});

test("journals retain corrupt or wrong-scope bytes and refuse unconfirmed writes", () => {
  const local = storage();
  const state = createStore();
  const journal = createDeckJournalStore(
    origin,
    account,
    { kind: "save", deckId: deck },
    () => local,
  );
  local.values.set(journal.key, "{broken");
  expect(
    state.set(journal.actionAtom, {
      type: "prepare",
      expected: null,
      command: save(),
    }),
  ).toBeNull();
  expect(local.values.get(journal.key)).toBe("{broken");
  const silent = createDeckJournalStore(
    origin,
    account,
    { kind: "save", deckId: deck },
    () => ({ getItem: () => null, setItem: () => {} }),
  );
  expect(
    state.set(silent.actionAtom, {
      type: "prepare",
      expected: null,
      command: save(),
    }),
  ).toBeNull();
  expect(state.get(silent.stateAtom).status).toBe("unavailable");
  const other = createDeckJournalStore(
    origin,
    revision,
    { kind: "save", deckId: deck },
    () => local,
  );
  expect(other.key).not.toBe(journal.key);
});

test("shared durable cell refresh does not write and returned objects do not alias published state", () => {
  let writes = 0;
  let raw: string | null = null;
  const cell = createDurableCell(
    "cell-test",
    z.strictObject({ entries: z.array(z.strictObject({ name: z.string() })) }),
    () => ({
      getItem: () => raw,
      setItem: (_key, value) => {
        writes++;
        raw = value;
      },
    }),
  );
  const state = createStore();
  const returned = state.set(cell.writeAtom, {
    entries: [{ name: "Original" }],
  })!;
  returned.entries[0]!.name = "Changed";
  const published = state.get(cell.stateAtom);
  expect(published.status === "saved" && published.value.entries[0]?.name).toBe(
    "Original",
  );
  state.set(cell.refreshAtom, cell.read());
  expect(writes).toBe(1);
  expect(cell.read()).toEqual({
    status: "saved",
    value: { entries: [{ name: "Original" }] },
  });
});

test("an unconfirmed terminal write retains the original unresolved body for later recovery", () => {
  const local = storage();
  let drop = false;
  const state = createStore();
  const access = {
    getItem: local.getItem,
    setItem: (key: string, value: string) => {
      if (!drop) local.setItem(key, value);
    },
  };
  const journal = createDeckJournalStore(
    origin,
    account,
    { kind: "save", deckId: deck },
    () => access,
  );
  const original = state.set(journal.actionAtom, {
    type: "prepare",
    expected: null,
    command: save(),
  })!;
  drop = true;
  expect(
    state.set(journal.actionAtom, {
      type: "observe",
      command: original.command,
      disposition: accepted,
    }),
  ).toBeNull();
  expect(state.get(journal.stateAtom).status).toBe("unavailable");
  drop = false;
  const remounted = createDeckJournalStore(
    origin,
    account,
    { kind: "save", deckId: deck },
    () => access,
  );
  expect(state.set(remounted.actionAtom, { type: "refresh" })).toEqual(
    original,
  );
  expect(
    state.set(remounted.actionAtom, {
      type: "observe",
      command: original.command,
      disposition: accepted,
    })?.state,
  ).toBe("accepted");
});

test("copying a journal across account or API scopes cannot turn it into a recoverable command", () => {
  const local = storage();
  const state = createStore();
  const original = createDeckJournalStore(
    origin,
    account,
    { kind: "save", deckId: deck },
    () => local,
  );
  state.set(original.actionAtom, {
    type: "prepare",
    expected: null,
    command: save(),
  });
  const bytes = local.values.get(original.key)!;
  for (const [api, owner] of [
    [origin, revision],
    ["https://other.example", account],
  ]) {
    const wrong = createDeckJournalStore(
      api!,
      owner!,
      { kind: "save", deckId: deck },
      () => local,
    );
    local.values.set(wrong.key, bytes);
    expect(state.set(wrong.actionAtom, { type: "refresh" })).toBeNull();
    expect(state.get(wrong.stateAtom).status).toBe("unavailable");
    expect(local.values.get(wrong.key)).toBe(bytes);
  }
});

test("settings recovery is deck-scoped and keeps both snapshots distinct from contents", () => {
  const local = storage();
  const atoms = createStore();
  const store = createDeckJournalStore(
    origin,
    account,
    { kind: "settings", deckId: deck },
    () => local,
  );
  const values = {
    name: "Before",
    mode: "collection",
    formatId: account,
    appearance: {
      backId: account,
      sleeveId: deck,
      diceId: revision,
      playmatId: null,
    },
  };
  const command = deckCommandSchema.parse({
    kind: "settings",
    command: {
      requestId: crypto.randomUUID(),
      deckId: deck,
      expected: values,
      settings: { ...values, name: "After" },
    },
  });
  const first = atoms.set(store.actionAtom, {
    type: "prepare",
    expected: null,
    command,
  });
  expect(first?.state).toBe("unresolved");
  const reloaded = createDeckJournalStore(
    origin,
    account,
    { kind: "settings", deckId: deck },
    () => local,
  );
  expect(createStore().set(reloaded.actionAtom, { type: "refresh" })).toEqual(
    first,
  );
  const contents = createDeckJournalStore(
    origin,
    account,
    { kind: "save", deckId: deck },
    () => local,
  );
  expect(contents.key).not.toBe(store.key);
  expect(atoms.set(contents.actionAtom, { type: "refresh" })).toBeNull();
  const altered = structuredClone(command);
  if (altered.kind !== "settings") throw new Error("expected settings command");
  altered.command.settings.name = "Changed after submission";
  expect(
    atoms.set(store.actionAtom, {
      type: "prepare",
      expected: first,
      command: altered,
    }),
  ).toBeNull();
  const terminal = deckCommandDispositionSchema.parse({
    state: "rejected",
    response: {
      status: "fail",
      data: { settings: "deck settings changed; reload before saving" },
    },
  });
  expect(
    atoms.set(store.actionAtom, {
      type: "observe",
      command,
      disposition: terminal,
    })?.state,
  ).toBe("rejected");
});

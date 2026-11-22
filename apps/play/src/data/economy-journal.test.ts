import { expect, test } from "bun:test";
import { createStore } from "jotai";
import type { Session } from "@supabase/supabase-js";
import {
  economyCommandSchema,
  type EconomyCommand,
} from "@spellwake/application-contracts";
import { intentMutationFeedback } from "./intent-feedback";
import { createEconomyJournal, economyIntentSlot } from "./economy-journal";
import { recoverEconomyIntent } from "./economy-recovery";
import { ApplicationApi } from "./application-api";
import {
  isAccountPayloadKey,
  eraseAccountPayloads,
  accountClosedStorageKey,
} from "./account-storage";
const origin = "https://api.example";
const account = "11111111-1111-4111-8111-111111111111";
const subject = "22222222-2222-4222-8222-222222222222";
const conversation = "33333333-3333-4333-8333-333333333333";
const request = "44444444-4444-4444-8444-444444444444";
const intent: EconomyCommand = {
  kind: "listing_buy",
  command: {
    requestId: request,
    listingId: conversation,
    expectedRevision: "4",
    expectedTotal: "1000",
  },
};
class MemoryStorage implements Storage {
  get length() {
    return Object.keys(this).length;
  }
  clear() {
    for (const key of Object.keys(this)) this.removeItem(key);
  }
  getItem(key: string): string | null {
    const value = Object.hasOwn(this, key) ? Reflect.get(this, key) : null;
    return typeof value === "string" ? value : null;
  }
  setItem(key: string, value: string) {
    Object.defineProperty(this, key, {
      value,
      enumerable: true,
      configurable: true,
      writable: true,
    });
  }
  removeItem(key: string) {
    Reflect.deleteProperty(this, key);
  }
  key(index: number) {
    return Object.keys(this)[index] ?? null;
  }
}
async function withStorageEnvironment(
  work: (storage: MemoryStorage) => Promise<void>,
) {
  const storage = new MemoryStorage();
  const priorWindow = Object.getOwnPropertyDescriptor(globalThis, "window");
  const priorNavigator = Object.getOwnPropertyDescriptor(
    globalThis,
    "navigator",
  );
  const events = Object.assign(new EventTarget(), { localStorage: storage });
  Object.defineProperty(globalThis, "window", {
    value: events,
    configurable: true,
  });
  Object.defineProperty(globalThis, "navigator", {
    value: {
      locks: { request: async (_key: string, work: () => unknown) => work() },
    },
    configurable: true,
  });
  try {
    await work(storage);
  } finally {
    if (priorWindow) Object.defineProperty(globalThis, "window", priorWindow);
    else Reflect.deleteProperty(globalThis, "window");
    if (priorNavigator)
      Object.defineProperty(globalThis, "navigator", priorNavigator);
    else Reflect.deleteProperty(globalThis, "navigator");
  }
}

const owner = {
  state: "active" as const,
  accountId: account,
  subjectId: subject,
  sessionId: request,
  method: "email_code" as const,
};
function authSession(): Session {
  return {
    access_token: "owner-token",
    refresh_token: "unit-refresh",
    token_type: "bearer",
    expires_in: 3600,
    user: {
      id: subject,
      aud: "authenticated",
      app_metadata: {},
      user_metadata: {},
      created_at: "2026-11-02T00:00:00Z",
    },
  };
}

function setup(
  storage: MemoryStorage,
  fetcher: NonNullable<Parameters<typeof ApplicationApi.create>[1]>,
  isCurrent = () => true,
) {
  const atoms = createStore();
  const journal = createEconomyJournal(
    origin,
    account,
    subject,
    economyIntentSlot(intent),
    () => storage,
  );
  const api = ApplicationApi.create(origin, fetcher);
  if (!api) throw new Error("Expected configured API");
  return {
    journal,
    owner,
    services: {
      api,
      auth: {
        getSession: async () => ({
          data: { session: authSession() },
          error: null,
        }),
      },
    },
    act: (
      action: Parameters<typeof recoverEconomyIntent>[1]["act"] extends (
        action: infer A,
      ) => unknown
        ? A
        : never,
    ) => atoms.set(journal.actionAtom, action),
    isCurrent,
    refresh: () => {},
  };
}
test("economic recovery preserves exact price/revision/request after lost acknowledgment and reload", async () =>
  withStorageEnvironment(async (storage) => {
    let posts = 0,
      lookups = 0;
    const fetcher: NonNullable<
      Parameters<typeof ApplicationApi.create>[1]
    > = async (url, init) => {
      expect(JSON.parse(String(init?.body))).toEqual(intent);
      expect(new Headers(init?.headers).get("Authorization")).toBe(
        "Bearer owner-token",
      );
      if (String(url).endsWith("/disposition")) {
        lookups++;
        return Response.json({
          status: "success",
          data:
            lookups === 1
              ? { state: "unknown", response: null }
              : {
                  state: "accepted",
                  response: {
                    status: "success",
                    data: { resourceId: conversation },
                  },
                },
        });
      }
      posts++;
      throw new Error("Controlled lost acknowledgment");
    };
    const options = setup(storage, fetcher);
    await expect(
      recoverEconomyIntent(
        { type: "submit", expected: null, draft: intent },
        options,
      ),
    ).rejects.toThrow();
    const reload = setup(storage, fetcher);
    const saved = reload.act({ type: "refresh" });
    expect(posts).toBe(1);
    expect(lookups).toBe(1);
    if (!saved) throw new Error("Expected saved intent");
    expect(saved.command.intent).toEqual(intent);
    const result = await recoverEconomyIntent(
      { type: "recover", expected: saved },
      reload,
    );
    expect(result.state).toBe("accepted");
    expect(posts).toBe(1);
    expect(lookups).toBe(2);
  }));
test("unresolved terms cannot change; foreign identity, bad target acknowledgment and corrupt storage stay fenced", () => {
  const storage = new MemoryStorage();
  const atoms = createStore();
  const journal = createEconomyJournal(
    origin,
    account,
    subject,
    economyIntentSlot(intent),
    () => storage,
  );
  const command = { subjectId: subject, intent };
  const saved = atoms.set(journal.actionAtom, {
    type: "prepare",
    expected: null,
    command,
  });
  expect(saved?.state).toBe("unresolved");
  expect(
    atoms.set(journal.actionAtom, {
      type: "prepare",
      expected: saved,
      command: {
        ...command,
        intent: {
          ...intent,
          command: { ...intent.command, expectedTotal: "999" },
        },
      },
    }),
  ).toBeNull();
  expect(
    atoms.set(journal.actionAtom, {
      type: "observe",
      command,
      disposition: {
        state: "accepted",
        response: { status: "success", data: { resourceId: request } },
      },
    }),
  ).toBeNull();
  const foreign = createEconomyJournal(
    origin,
    account,
    conversation,
    economyIntentSlot(intent),
    () => storage,
  );
  expect(createStore().get(foreign.stateAtom).status).toBe("unavailable");
  storage.setItem(journal.key, "invalid-json");
  expect(atoms.set(journal.actionAtom, { type: "refresh" })).toBeNull();
  expect(storage.getItem(journal.key)).toBe("invalid-json");
});
test("deletion recognizes exact economy slots, preserves foreign data and prevents late repopulation", async () =>
  withStorageEnvironment(async (storage) => {
    const options = setup(storage, async () => {
      throw new Error("No network expected");
    });
    const saved = options.act({
      type: "prepare",
      expected: null,
      command: { subjectId: subject, intent },
    });
    expect(saved).not.toBeNull();
    expect(isAccountPayloadKey(options.journal.key, origin, account)).toBe(
      true,
    );
    expect(
      isAccountPayloadKey(options.journal.key + ":extra", origin, account),
    ).toBe(false);
    expect(isAccountPayloadKey(options.journal.key, origin, subject)).toBe(
      false,
    );
    expect(
      isAccountPayloadKey(
        options.journal.key,
        "https://other.example",
        account,
      ),
    ).toBe(false);
    const foreignKey = options.journal.key.replace(account, subject);
    storage.setItem(foreignKey, "Other account");
    await eraseAccountPayloads(origin, account, {
      accountId: account,
      requestId: request,
      phase: "deleted",
      retirementOperationId: request,
      authCleanupReceiptId: conversation,
    });
    expect(storage.getItem(options.journal.key)).toBeNull();
    expect(storage.getItem(foreignKey)).toBe("Other account");
    expect(
      storage.getItem(accountClosedStorageKey(origin, account)),
    ).not.toBeNull();
    if (!saved) throw new Error("Expected saved intent");
    await expect(
      recoverEconomyIntent({ type: "recover", expected: saved }, options),
    ).rejects.toThrow();
    expect(storage.getItem(options.journal.key)).toBeNull();
  }));
test("changed session during credential lookup prevents economic dispatch", async () =>
  withStorageEnvironment(async (storage) => {
    let current = true,
      posts = 0;
    const options = setup(
      storage,
      async () => {
        posts++;
        throw new Error("Unexpected dispatch");
      },
      () => current,
    );
    options.services.auth.getSession = async () => {
      current = false;
      return { data: { session: authSession() }, error: null };
    };
    await expect(
      recoverEconomyIntent(
        { type: "submit", expected: null, draft: intent },
        options,
      ),
    ).rejects.toThrow();
    expect(posts).toBe(0);
  }));

test("deletion-pending accounts can confirm an accepted attempt but cannot resend an unknown commitment", async () =>
  withStorageEnvironment(async (storage) => {
    let unknown = false,
      sends = 0;
    const options = setup(storage, async (url) => {
      if (!String(url).endsWith("/disposition")) {
        sends++;
        throw new Error("Unexpected new commitment");
      }
      return Response.json({
        status: "success",
        data: unknown
          ? { state: "unknown", response: null }
          : {
              state: "accepted",
              response: {
                status: "success",
                data: { resourceId: conversation },
              },
            },
      });
    });
    const saved = options.act({
      type: "prepare",
      expected: null,
      command: { subjectId: subject, intent },
    });
    if (!saved) throw new Error("Expected retained attempt");
    const pending = {
      ...options,
      owner: { ...owner, state: "deletion_pending" as const },
    };
    await expect(
      recoverEconomyIntent(
        { type: "submit", expected: saved, draft: intent },
        pending,
      ),
    ).rejects.toThrow();
    unknown = true;
    await expect(
      recoverEconomyIntent({ type: "recover", expected: saved }, pending),
    ).rejects.toThrow();
    expect(sends).toBe(0);
    unknown = false;
    expect(
      (
        await recoverEconomyIntent(
          { type: "recover", expected: saved },
          pending,
        )
      ).state,
    ).toBe("accepted");
    expect(sends).toBe(0);
  }));
test("session change during acknowledgment leaves the original attempt unresolved", async () =>
  withStorageEnvironment(async (storage) => {
    let current = true;
    const options = setup(
      storage,
      async () => {
        current = false;
        return Response.json({
          status: "success",
          data: { resourceId: conversation },
        });
      },
      () => current,
    );
    await expect(
      recoverEconomyIntent(
        { type: "submit", expected: null, draft: intent },
        options,
      ),
    ).rejects.toThrow();
    const persisted: unknown = JSON.parse(
      storage.getItem(options.journal.key) ?? "null",
    );
    expect(persisted).toMatchObject({
      state: "unresolved",
      command: { subjectId: subject, intent },
    });
  }));

test("economic feedback follows the selected immutable terms, not a previous target or quote", () => {
  const pending = {
    status: "saved" as const,
    value: { state: "unresolved", command: { intent } },
  };
  const error = new Error("Unconfirmed");
  const feedback = (
    selected: EconomyCommand,
    slot = economyIntentSlot(intent),
  ) =>
    intentMutationFeedback(
      selected,
      slot,
      pending,
      true,
      error,
      economyCommandSchema,
      economyIntentSlot,
    );
  expect(feedback(intent)).toEqual({ isPending: true, error });
  expect(
    feedback(intent, { ...economyIntentSlot(intent), targetId: account }),
  ).toEqual({ isPending: false, error: null });
  expect(
    feedback({
      ...intent,
      command: { ...intent.command, expectedTotal: "999" },
    }),
  ).toEqual({ isPending: false, error: null });
  expect(
    intentMutationFeedback(
      intent,
      economyIntentSlot(intent),
      { status: "saved", value: { ...pending.value, state: "accepted" } },
      false,
      error,
      economyCommandSchema,
      economyIntentSlot,
    ),
  ).toEqual({ isPending: false, error: null });
});

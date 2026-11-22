import { expect, test } from "bun:test";
import { createStore } from "jotai";
import type { Session } from "@supabase/supabase-js";
import type { IntentJournalAction } from "./intent-journal";
import type { SocialJournal } from "./social-recovery";
import type { ScopedSocialIntent } from "./social-journal";
import { ApplicationApi } from "./application-api";
import { recoverSocialIntent } from "./social-recovery";
import { markDirectReadSchema } from "@spellwake/application-contracts";
import {
  createSocialDraft,
  createSocialJournal,
  type SocialIntent,
} from "./social-journal";
import {
  accountClosedStorageKey,
  eraseAccountPayloads,
  isAccountPayloadKey,
  withAccountStorageLock,
} from "./account-storage";

const origin = "https://api.example";
const account = "11111111-1111-4111-8111-111111111111";
const subject = "22222222-2222-4222-8222-222222222222";
const conversation = "33333333-3333-4333-8333-333333333333";
const request = "44444444-4444-4444-8444-444444444444";
function memory() {
  const values = new Map<string, string>();
  return {
    values,
    getItem: (key: string) => values.get(key) ?? null,
    setItem: (key: string, value: string) => {
      values.set(key, value);
    },
  };
}
const intent: SocialIntent = {
  kind: "social",
  input: {
    kind: "send_dm",
    command: {
      requestId: request,
      conversationId: conversation,
      body: "Preserve exact text",
    },
  },
};
test("social intent retains immutable identity, body and request across reload", () => {
  const storage = memory();
  const slot = { kind: "send_dm" as const, targetId: conversation };
  const journal = createSocialJournal(
    origin,
    account,
    subject,
    slot,
    () => storage,
  );
  const atoms = createStore();
  const command = { subjectId: subject, intent: structuredClone(intent) };
  const first = atoms.set(journal.actionAtom, {
    type: "prepare",
    expected: null,
    command,
  });
  expect(first?.state).toBe("unresolved");
  const reload = createSocialJournal(
    origin,
    account,
    subject,
    slot,
    () => storage,
  );
  expect(createStore().set(reload.actionAtom, { type: "refresh" })).toEqual(
    first,
  );
  const other = createSocialJournal(
    origin,
    account,
    conversation,
    slot,
    () => storage,
  );
  expect(createStore().get(other.stateAtom).status).toBe("unavailable");
  const replacement = {
    subjectId: subject,
    intent: {
      ...intent,
      input: {
        kind: "open_dm" as const,
        command: { requestId: request, otherAccountId: conversation },
      },
    },
  };
  expect(
    atoms.set(journal.actionAtom, {
      type: "prepare",
      expected: first,
      command: replacement,
    }),
  ).toBeNull();
  expect(storage.values.get(journal.key)).toContain("Preserve exact text");
});

test("read journal rejects foreign acknowledgments and changed observed sets", () => {
  const storage = memory();
  const journal = createSocialJournal(
    origin,
    account,
    subject,
    { kind: "read_dm", targetId: conversation },
    () => storage,
  );
  const atoms = createStore();
  const command = {
    subjectId: subject,
    intent: {
      kind: "read" as const,
      input: {
        requestId: request,
        conversationId: conversation,
        messageIds: [request],
      },
    },
  };
  const first = atoms.set(journal.actionAtom, {
    type: "prepare",
    expected: null,
    command,
  });
  expect(first?.state).toBe("unresolved");
  expect(
    atoms.set(journal.actionAtom, {
      type: "observe",
      command,
      disposition: {
        state: "accepted",
        response: { status: "success", data: { resourceId: account } },
      },
    }),
  ).toBeNull();
  expect(JSON.parse(storage.values.get(journal.key) ?? "{}").state).toBe(
    "unresolved",
  );
  const refreshed = atoms.set(journal.actionAtom, { type: "refresh" });
  expect(
    atoms.set(journal.actionAtom, {
      type: "prepare",
      expected: refreshed,
      command: {
        ...command,
        intent: {
          ...command.intent,
          input: { ...command.intent.input, messageIds: [account] },
        },
      },
    }),
  ).toBeNull();
  expect(
    markDirectReadSchema.safeParse({
      ...command.intent.input,
      messageIds: [request, request],
    }).success,
  ).toBe(false);
});

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

test("confirmed send clears only the exact persisted draft, preserving newer, foreign and corrupt bytes", async () =>
  withStorageEnvironment(async (storage) => {
    const atoms = createStore();
    const draft = createSocialDraft(
      origin,
      account,
      subject,
      conversation,
      () => storage,
    );
    expect(await atoms.set(draft.clearIfTextAtom, "Sent message")).toBe(false);
    expect(await atoms.set(draft.saveAtom, "Sent message")).not.toBeNull();
    const secondWindow = createSocialDraft(
      origin,
      account,
      subject,
      conversation,
      () => storage,
    );
    expect(
      await atoms.set(secondWindow.saveAtom, "Newer draft"),
    ).not.toBeNull();
    expect(await atoms.set(draft.clearIfTextAtom, "Sent message")).toBe(false);
    expect(JSON.parse(storage.getItem(draft.key) ?? "null").text).toBe(
      "Newer draft",
    );
    expect(await atoms.set(draft.clearIfTextAtom, "Newer draft")).toBe(true);
    expect(JSON.parse(storage.getItem(draft.key) ?? "null").text).toBe("");
    const corrupt = JSON.stringify({ subjectId: subject, text: "Preserve" });
    storage.setItem(draft.key, corrupt);
    expect(await atoms.set(draft.clearIfTextAtom, "Preserve")).toBe(false);
    expect(storage.getItem(draft.key)).toBe(corrupt);
    const foreign = createSocialDraft(
      origin,
      account,
      conversation,
      conversation,
      () => storage,
    );
    expect(await atoms.set(foreign.clearIfTextAtom, "Preserve")).toBe(false);
    expect(storage.getItem(draft.key)).toBe(corrupt);
  }));

test("social drafts and attempts join exact-owner erasure and cannot recreate after closure", async () =>
  withStorageEnvironment(async (storage) => {
    const atoms = createStore();
    const draft = createSocialDraft(
      origin,
      account,
      subject,
      conversation,
      () => storage,
    );
    expect(await atoms.set(draft.saveAtom, "Private draft")).not.toBeNull();
    const corrupted = JSON.stringify({
      version: 1,
      apiOrigin: origin,
      accountId: account,
      subjectId: conversation,
      conversationId: conversation,
      text: "Preserve corrupt draft",
    });
    storage.setItem(draft.key, corrupted);
    await expect(
      atoms.set(draft.saveAtom, "Must not replace it"),
    ).rejects.toThrow();
    expect(storage.getItem(draft.key)).toBe(corrupted);

    const journal = createSocialJournal(
      origin,
      account,
      subject,
      { kind: "send_dm", targetId: conversation },
      () => storage,
    );
    await withAccountStorageLock(origin, account, () =>
      atoms.set(journal.actionAtom, {
        type: "prepare",
        expected: null,
        command: { subjectId: subject, intent },
      }),
    );
    const other = `spellwake.social-draft.v1:${origin}:${subject}:${conversation}`;
    storage.setItem(other, "Other account");
    expect(isAccountPayloadKey(journal.key, origin, account)).toBe(true);
    expect(isAccountPayloadKey(`${journal.key}:extra`, origin, account)).toBe(
      false,
    );
    await eraseAccountPayloads(origin, account, {
      phase: "deleted",
      accountId: account,
      requestId: request,
      retirementOperationId: request,
      authCleanupReceiptId: conversation,
    });
    expect(storage.getItem(draft.key)).toBeNull();
    expect(storage.getItem(journal.key)).toBeNull();
    expect(storage.getItem(other)).toBe("Other account");
    expect(
      storage.getItem(accountClosedStorageKey(origin, account)),
    ).not.toBeNull();
    await expect(atoms.set(draft.saveAtom, "Late response")).rejects.toThrow();
    expect(storage.getItem(draft.key)).toBeNull();
  }));

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

test("explicit social recovery checks accepted lost acknowledgment without sending twice", async () =>
  withStorageEnvironment(async (storage) => {
    const atoms = createStore();
    const journal = createSocialJournal(
      origin,
      account,
      subject,
      { kind: "send_dm", targetId: conversation },
      () => storage,
    );
    let posts = 0;
    let lookups = 0;
    let refreshes = 0;
    const api = ApplicationApi.create(origin, async (url, init) => {
      expect(JSON.parse(String(init?.body))).toEqual(
        intent.kind === "social" ? intent.input : {},
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
                    data: { resourceId: request },
                  },
                },
        });
      }
      posts++;
      throw new Error("Controlled lost acknowledgment");
    });
    if (!api) throw new Error("Expected configured test API.");
    const options = {
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
        action: IntentJournalAction<
          ScopedSocialIntent,
          Extract<SocialJournal, { state: "rejected" }>["errors"]
        >,
      ) => atoms.set(journal.actionAtom, action),
      isCurrent: () => true,
      refresh: () => {
        refreshes++;
      },
    };
    await expect(
      recoverSocialIntent(
        { type: "submit", expected: null, draft: intent },
        options,
      ),
    ).rejects.toThrow();
    const saved = atoms.set(journal.actionAtom, { type: "refresh" });
    if (!saved) throw new Error("Expected retained attempt.");
    expect(saved.state).toBe("unresolved");
    expect(posts).toBe(1);
    const result = await recoverSocialIntent(
      { type: "recover", expected: saved },
      options,
    );
    expect(result.state).toBe("accepted");
    expect(posts).toBe(1);
    expect(lookups).toBe(2);
    expect(refreshes).toBe(1);
  }));

test("account change during SDK lookup prevents any social post", async () =>
  withStorageEnvironment(async (storage) => {
    const atoms = createStore();
    const journal = createSocialJournal(
      origin,
      account,
      subject,
      { kind: "send_dm", targetId: conversation },
      () => storage,
    );
    let current = true;
    let posts = 0;
    const api = ApplicationApi.create(origin, async () => {
      posts++;
      return Response.json({
        status: "success",
        data: { resourceId: request },
      });
    });
    if (!api) throw new Error("Expected test API.");
    await expect(
      recoverSocialIntent(
        { type: "submit", expected: null, draft: intent },
        {
          journal,
          owner,
          services: {
            api,
            auth: {
              getSession: async () => {
                current = false;
                return { data: { session: authSession() }, error: null };
              },
            },
          },
          act: (action) => atoms.set(journal.actionAtom, action),
          isCurrent: () => current,
          refresh: () => {
            throw new Error("Must not refresh another owner.");
          },
        },
      ),
    ).rejects.toThrow();
    expect(posts).toBe(0);
    expect(atoms.set(journal.actionAtom, { type: "refresh" })?.state).toBe(
      "unresolved",
    );
  }));

test("pending-account read acknowledgment persists accepted result but suppresses stale completion", async () =>
  withStorageEnvironment(async (storage) => {
    const atoms = createStore();
    const journal = createSocialJournal(
      origin,
      account,
      subject,
      { kind: "read_dm", targetId: conversation },
      () => storage,
    );
    let current = true;
    let posts = 0;
    const draft: SocialIntent = {
      kind: "read",
      input: {
        requestId: request,
        conversationId: conversation,
        messageIds: [request],
      },
    };
    const api = ApplicationApi.create(origin, async (url, init) => {
      posts++;
      expect(String(url)).toBe(origin + "/v1/social/read");
      expect(JSON.parse(String(init?.body))).toEqual(draft.input);
      return Response.json({
        status: "success",
        data: { resourceId: conversation },
      });
    });
    if (!api) throw new Error("Expected test API.");
    await expect(
      recoverSocialIntent(
        { type: "submit", expected: null, draft },
        {
          journal,
          owner: { ...owner, state: "deletion_pending" },
          services: {
            api,
            auth: {
              getSession: async () => ({
                data: { session: authSession() },
                error: null,
              }),
            },
          },
          act: (action) => atoms.set(journal.actionAtom, action),
          isCurrent: () => current,
          refresh: () => {
            current = false;
          },
        },
      ),
    ).rejects.toThrow();
    expect(posts).toBe(1);
    expect(atoms.set(journal.actionAtom, { type: "refresh" })?.state).toBe(
      "accepted",
    );
  }));

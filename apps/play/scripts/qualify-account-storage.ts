/** Owned browser storage/locking qualification; no Auth or finalizer authority claim. */
import { chromium, expect, type CDPSession } from "@playwright/test";
import { mkdir, writeFile } from "node:fs/promises";
import { resolve } from "node:path";

/** A failed pause must release the owned browser rather than hang qualification. */
function pausedDebugger(session: CDPSession): Promise<void> {
  return new Promise((done, reject) => {
    const timer = setTimeout(
      () =>
        reject(
          new Error("Owned storage race did not reach its debugger pause."),
        ),
      10000,
    );
    session.once("Debugger.paused", () => {
      clearTimeout(timer);
      done();
    });
  });
}

const app = "http://127.0.0.1:6217";
const origin = "https://api.storage-fixture.example";
const account = crypto.randomUUID();
const other = crypto.randomUUID();
const deck = crypto.randomUUID();
const cosmetic = crypto.randomUUID();
const proof = {
  accountId: account,
  requestId: crypto.randomUUID(),
  phase: "deleted" as const,
  retirementOperationId: crypto.randomUUID(),
  authCleanupReceiptId: crypto.randomUUID(),
};
const keys = [
  `spellwake.deck-command.v1:${origin}:${account}:create`,
  `spellwake.deck-command.v1:${origin}:${account}:save:${deck}`,
  `spellwake.deck-command.v1:${origin}:${account}:settings:${deck}`,
  `spellwake.profile-defaults.v1:${origin}:${account}`,
  `spellwake.checkout.v1:${origin}:${account}:${cosmetic}`,
];
const preserved = [
  `spellwake.deck-command.v1:${origin}:${other}:create`,
  `spellwake.deck-command.v1:https://other.storage-fixture.example:${account}:create`,
  `spellwake.deletion.v1:${origin}:${account}`,
  "spellwake.demo.fixture",
  "unrelated.fixture",
];
const stage = resolve(
  "services/application/target",
  `client-storage-${crypto.randomUUID()}`,
);
await mkdir(stage, { mode: 0o700 });
const browser = await chromium.launch({ headless: true });
const context = await browser.newContext();
const first = await context.newPage();
const second = await context.newPage();
const checks: string[] = [];
try {
  await Promise.all([first.goto(app), second.goto(app)]);
  await first.evaluate(
    ({ keys, preserved }) => {
      for (const key of [...keys, ...preserved])
        localStorage.setItem(key, "owned fixture payload");
    },
    { keys, preserved },
  );
  // Pause the actual writer after its final closure check and before setItem.
  // The debugger is fixture-only: production callbacks remain synchronous.
  const cdp = await context.newCDPSession(first);
  await cdp.send("Debugger.enable");
  await first.evaluate((key) => {
    const original = Storage.prototype.setItem;
    Storage.prototype.setItem = function (
      this: Storage,
      candidate: string,
      value: string,
    ) {
      if (candidate === key) {
        Storage.prototype.setItem = original;
        // eslint-disable-next-line no-debugger -- Fixture-only CDP barrier after the actual fence read.
        debugger;
      }
      return original.call(this, candidate, value);
    };
  }, keys[0]!);
  const paused = pausedDebugger(cdp);
  const writer = first.evaluate(
    async ({ origin, account, key }) => {
      const path = "/src/data/account-storage.ts";
      const mod: typeof import("../src/data/account-storage") = await import(
        path
      );
      return mod.withAccountStorageLock(origin, account, () =>
        localStorage.setItem(key, "late writer bytes"),
      );
    },
    { origin, account, key: keys[0]! },
  );
  await paused;
  const erasure = second.evaluate(
    async ({ origin, account, proof }) => {
      const path = "/src/data/account-storage.ts";
      const mod: typeof import("../src/data/account-storage") = await import(
        path
      );
      await mod.eraseAccountPayloads(origin, account, proof);
    },
    { origin, account, proof },
  );
  await expect
    .poll(() =>
      second.evaluate(
        async ({ origin, account }) => {
          const locks = await navigator.locks.query();
          return (
            locks.pending?.some(
              (lock) =>
                lock.name ===
                `spellwake.account-storage.v1:${origin}:${account}`,
            ) ?? false
          );
        },
        { origin, account },
      ),
    )
    .toBe(true);
  await cdp.send("Debugger.resume");
  await Promise.all([writer, erasure]);
  await cdp.detach();
  checks.push(
    "writer paused after closure check finishes before queued eraser; payloads absent after both finish",
  );
  const snapshot = await second.evaluate(
    ({ keys, preserved, origin, account }) => ({
      removed: keys.every((key) => localStorage.getItem(key) === null),
      preserved: preserved.every(
        (key) => localStorage.getItem(key) === "owned fixture payload",
      ),
      closed: localStorage.getItem(
        `spellwake.account-closed.v1:${origin}:${account}`,
      ),
    }),
    { keys, preserved, origin, account },
  );
  expect(snapshot.removed).toBe(true);
  expect(snapshot.preserved).toBe(true);
  expect(snapshot.closed).not.toBeNull();
  checks.push(
    "exact owner/API families erased; observer, other owner/API, demo and unrelated payloads preserved",
  );
  // A new slot never existed when the eraser took its snapshot.
  const newSlot = `spellwake.deck-command.v1:${origin}:${account}:save:${crypto.randomUUID()}`;
  const refused = await first.evaluate(
    async ({ origin, account, newSlot }) => {
      const path = "/src/data/account-storage.ts";
      const mod: typeof import("../src/data/account-storage") = await import(
        path
      );
      try {
        await mod.withAccountStorageLock(origin, account, () =>
          localStorage.setItem(newSlot, "resurrection"),
        );
        return false;
      } catch {
        return localStorage.getItem(newSlot) === null;
      }
    },
    { origin, account, newSlot },
  );
  expect(refused).toBe(true);
  await first.reload();
  const afterReload = await first.evaluate(
    async ({ origin, account, proof }) => {
      const path = "/src/data/account-storage.ts";
      const mod: typeof import("../src/data/account-storage") = await import(
        path
      );
      await mod.eraseAccountPayloads(origin, account, {
        ...proof,
        requestId: crypto.randomUUID(),
      });
      try {
        await mod.withAccountStorageLock(origin, account, () => true);
        return false;
      } catch {
        return true;
      }
    },
    { origin, account, proof },
  );
  expect(afterReload).toBe(true);
  checks.push(
    "new-slot and late writers rejected; reload and another valid observer retain the same permanent fence",
  );
  // The inverse ordering: erasure has closed the scope and is paused while
  // holding it; a previously absent slot queues behind that erasure.
  const queuedAccount = crypto.randomUUID();
  const queuedProof = { ...proof, accountId: queuedAccount };
  const oldKey = `spellwake.profile-defaults.v1:${origin}:${queuedAccount}`;
  const queuedKey = `spellwake.deck-command.v1:${origin}:${queuedAccount}:save:${crypto.randomUUID()}`;
  const eraseDebugger = await context.newCDPSession(second);
  await eraseDebugger.send("Debugger.enable");
  await second.evaluate((oldKey) => {
    localStorage.setItem(oldKey, "fixture");
    const original = Storage.prototype.removeItem;
    Storage.prototype.removeItem = function (this: Storage, key: string) {
      if (key === oldKey) {
        Storage.prototype.removeItem = original;
        // eslint-disable-next-line no-debugger -- Fixture-only CDP barrier after the actual fence read.
        debugger;
      }
      return original.call(this, key);
    };
  }, oldKey);
  const eraserPaused = pausedDebugger(eraseDebugger);
  const heldErasure = second.evaluate(
    async ({ origin, queuedAccount, queuedProof }) => {
      const path = "/src/data/account-storage.ts";
      const mod: typeof import("../src/data/account-storage") = await import(
        path
      );
      await mod.eraseAccountPayloads(origin, queuedAccount, queuedProof);
    },
    { origin, queuedAccount, queuedProof },
  );
  await eraserPaused;
  const queuedWriter = first.evaluate(
    async ({ origin, queuedAccount, queuedKey }) => {
      const path = "/src/data/account-storage.ts";
      const mod: typeof import("../src/data/account-storage") = await import(
        path
      );
      try {
        await mod.withAccountStorageLock(origin, queuedAccount, () =>
          localStorage.setItem(queuedKey, "resurrection"),
        );
        return false;
      } catch {
        return localStorage.getItem(queuedKey) === null;
      }
    },
    { origin, queuedAccount, queuedKey },
  );
  await expect
    .poll(() =>
      first.evaluate(
        async ({ origin, queuedAccount }) => {
          const locks = await navigator.locks.query();
          return (
            locks.pending?.some(
              (lock) =>
                lock.name ===
                `spellwake.account-storage.v1:${origin}:${queuedAccount}`,
            ) ?? false
          );
        },
        { origin, queuedAccount },
      ),
    )
    .toBe(true);
  await eraseDebugger.send("Debugger.resume");
  await heldErasure;
  expect(await queuedWriter).toBe(true);
  await eraseDebugger.detach();
  expect(
    await first.evaluate(
      ({ oldKey, queuedKey }) =>
        localStorage.getItem(oldKey) === null &&
        localStorage.getItem(queuedKey) === null,
      { oldKey, queuedKey },
    ),
  ).toBe(true);
  checks.push(
    "eraser paused after closure keeps new-slot writer queued, then refuses it after removal",
  );
  const memory = await second.evaluate(
    async ({ origin, proof }) => {
      const source = performance
        .getEntriesByType("resource")
        .map((entry) => new URL(entry.name))
        .find((url) => url.pathname.endsWith("/jotai.js"));
      if (!source) throw new Error("Actual Jotai module missing.");
      const jotai: typeof import("jotai") = await import(source.href);
      const journalPath = "/src/data/deck-journal.ts";
      const journals: typeof import("../src/data/deck-journal") = await import(
        journalPath
      );
      const path = "/src/data/account-storage.ts";
      const mod: typeof import("../src/data/account-storage") = await import(
        path
      );
      const accountId = crypto.randomUUID();
      const exactProof = { ...proof, accountId };
      const record = journals.createDeckJournalStore(
        origin,
        accountId,
        { kind: "create" },
        () => localStorage,
      );
      const atoms = jotai.createStore();
      const stop = atoms.sub(record.stateAtom, () => {});
      const failure = async (work: () => Promise<unknown>) => {
        try {
          await work();
          return false;
        } catch {
          return true;
        }
      };
      const noFence = await failure(() =>
        mod.removeFinalizedAccountObserver(
          origin,
          accountId,
          exactProof,
          "synthetic retained observer",
        ),
      );
      await navigator.locks.request(record.key, () =>
        mod.withAccountStorageLock(origin, accountId, () =>
          atoms.set(record.actionAtom, {
            type: "prepare",
            expected: null,
            command: {
              kind: "create",
              command: {
                requestId: crypto.randomUUID(),
                name: "Private in-memory draft",
                mode: "open",
                formatId: crypto.randomUUID(),
              },
            },
          }),
        ),
      );
      const before = atoms.get(record.stateAtom).status;
      await mod.eraseAccountPayloads(origin, accountId, exactProof);
      const after = atoms.get(record.stateAtom).status;
      const observerKey = `spellwake.deletion.v1:${origin}:${accountId}`;
      localStorage.setItem(observerKey, "synthetic retained observer");
      const wrongProof = await failure(() =>
        mod.removeFinalizedAccountObserver(
          origin,
          accountId,
          { ...exactProof, retirementOperationId: crypto.randomUUID() },
          "synthetic retained observer",
        ),
      );
      const retained = localStorage.getItem(observerKey) !== null;
      const changedRecord = await failure(() =>
        mod.removeFinalizedAccountObserver(
          origin,
          accountId,
          exactProof,
          "different record",
        ),
      );
      const originalRemove = Storage.prototype.removeItem;
      Storage.prototype.removeItem = function (this: Storage, key: string) {
        if (key !== observerKey) originalRemove.call(this, key);
      };
      const removalFailed = await failure(() =>
        mod.removeFinalizedAccountObserver(
          origin,
          accountId,
          exactProof,
          "synthetic retained observer",
        ),
      );
      Storage.prototype.removeItem = originalRemove;
      const retainedAfterFailure =
        localStorage.getItem(observerKey) === "synthetic retained observer";
      await mod.removeFinalizedAccountObserver(
        origin,
        accountId,
        exactProof,
        "synthetic retained observer",
      );
      await mod.removeFinalizedAccountObserver(
        origin,
        accountId,
        exactProof,
        "synthetic retained observer",
      );
      const removed = localStorage.getItem(observerKey) === null;
      const stillClosed = await failure(() =>
        mod.withAccountStorageLock(origin, accountId, () => true),
      );
      stop();
      return {
        before,
        after,
        noFence,
        wrongProof,
        retained,
        changedRecord,
        removalFailed,
        retainedAfterFailure,
        removed,
        stillClosed,
      };
    },
    { origin, proof },
  );
  expect(memory).toEqual({
    before: "saved",
    after: "empty",
    noFence: true,
    wrongProof: true,
    retained: true,
    changedRecord: true,
    removalFailed: true,
    retainedAfterFailure: true,
    removed: true,
    stillClosed: true,
  });
  checks.push(
    "same-window mounted Jotai rereads erased bytes; final cleanup requires exact existing proof and never reopens normal writes",
  );
  const negative = await second.evaluate(
    async ({ origin }) => {
      const path = "/src/data/account-storage.ts";
      const mod: typeof import("../src/data/account-storage") = await import(
        path
      );
      const failed = async (work: () => Promise<unknown>) => {
        try {
          await work();
          return false;
        } catch {
          return true;
        }
      };
      const account = crypto.randomUUID();
      const key = `spellwake.deck-command.v1:${origin}:${account}:create`;
      const closed = mod.accountClosedStorageKey(origin, account);
      const proof = {
        accountId: account,
        requestId: crypto.randomUUID(),
        phase: "deleted" as const,
        retirementOperationId: crypto.randomUUID(),
        authCleanupReceiptId: crypto.randomUUID(),
      };
      localStorage.setItem(key, "fixture");
      const wrongProof = await failed(() =>
        mod.eraseAccountPayloads(origin, crypto.randomUUID(), proof),
      );
      const pending = await failed(() =>
        mod.eraseAccountPayloads(origin, account, {
          accountId: account,
          requestId: proof.requestId,
          phase: "pending",
        }),
      );
      const originalSet = Storage.prototype.setItem;
      Storage.prototype.setItem = function (
        this: Storage,
        k: string,
        value: string,
      ) {
        if (k !== closed) originalSet.call(this, k, value);
      };
      const unconfirmedFence = await failed(() =>
        mod.eraseAccountPayloads(origin, account, proof),
      );
      Storage.prototype.setItem = originalSet;
      const untouched =
        localStorage.getItem(key) === "fixture" &&
        localStorage.getItem(closed) === null;
      const originalRemove = Storage.prototype.removeItem;
      Storage.prototype.removeItem = function (this: Storage, k: string) {
        if (k !== key) originalRemove.call(this, k);
      };
      const removalFailure = await failed(() =>
        mod.eraseAccountPayloads(origin, account, proof),
      );
      Storage.prototype.removeItem = originalRemove;
      const fencedAfterFailure = await failed(() =>
        mod.withAccountStorageLock(origin, account, () =>
          localStorage.setItem(key, "late"),
        ),
      );
      await mod.eraseAccountPayloads(origin, account, proof);
      const repaired = localStorage.getItem(key) === null;
      const conflicting = await failed(() =>
        mod.eraseAccountPayloads(origin, account, {
          ...proof,
          authCleanupReceiptId: crypto.randomUUID(),
        }),
      );
      const corruptAccount = crypto.randomUUID();
      localStorage.setItem(
        mod.accountClosedStorageKey(origin, corruptAccount),
        "not json",
      );
      const corruptFence = await failed(() =>
        mod.withAccountStorageLock(origin, corruptAccount, () => true),
      );
      return {
        wrongProof,
        pending,
        unconfirmedFence,
        untouched,
        removalFailure,
        fencedAfterFailure,
        repaired,
        conflicting,
        corruptFence,
      };
    },
    { origin },
  );
  expect(Object.values(negative).every(Boolean)).toBe(true);
  checks.push(
    "wrong/pending/conflicting proof, corrupt closure, dropped fence write and dropped payload removal fail closed; retry completes",
  );
  await writeFile(
    resolve(stage, "result.json"),
    JSON.stringify(
      {
        passed: true,
        checks,
        limitations: [
          "controlled browser storage/locks only",
          "no actual deletion observer/root/SDK/native erasure acceptance",
        ],
      },
      null,
      2,
    ),
    { mode: 0o600 },
  );
  console.log(JSON.stringify({ passed: true, checks: checks.length, stage }));
} finally {
  await context.close();
  await browser.close();
}

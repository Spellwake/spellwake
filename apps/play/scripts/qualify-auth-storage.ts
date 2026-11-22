/** Actual pinned SDK and React account consumer against controlled HTTP responses. */
import { chromium, expect, type Page, type CDPSession } from "@playwright/test";
import { mkdir, readFile, writeFile } from "node:fs/promises";
import { resolve } from "node:path";
import { DeletionFixture, qualifyDeletionBrowser } from "./deletion-browser";

const sourcePaths = [
  "apps/play/src/auth/AccountSettings.tsx",
  "apps/play/src/auth/account-settings.css.ts",
  "apps/play/src/auth/DeletionRecovery.tsx",
  "apps/play/src/application/Application.tsx",
  "apps/play/src/data/deletion.ts",
  "apps/play/src/data/deletion-recovery.ts",
  "apps/play/src/data/deletion-journal.ts",
  "apps/play/src/data/account-storage.ts",
  "apps/play/src/data/durable-cell.ts",
  "apps/play/src/data/application-api.ts",
  "apps/play/scripts/deletion-browser.ts",
  "apps/play/src/auth/storage.ts",
  "apps/play/src/auth/services.ts",
  "apps/play/src/auth/AccountProvider.tsx",
  "apps/play/scripts/qualify-auth-storage.ts",
  "apps/play/package.json",
  "bun.lock",
  "node_modules/.bun/@supabase+auth-js@2.118.0/node_modules/@supabase/auth-js/src/GoTrueClient.ts",
  "node_modules/.bun/@supabase+auth-js@2.118.0/node_modules/@supabase/auth-js/src/lib/helpers.ts",
];
async function sourceHashes(): Promise<Record<string, string>> {
  const values = await Promise.all(
    sourcePaths.map(
      async (path) =>
        [
          path,
          new Bun.CryptoHasher("sha256")
            .update(await readFile(path))
            .digest("hex"),
        ] as const,
    ),
  );
  return Object.fromEntries(values);
}
const sources = await sourceHashes();

const app = "http://127.0.0.1:6217";
const auth = "http://127.0.0.1:6218";
const api = "http://127.0.0.1:6219";
const users = ["a", "b", "c"].map((name) => ({
  name,
  id: crypto.randomUUID(),
  account: crypto.randomUUID(),
  session: crypto.randomUUID(),
  email: `${name}@sdk-fixture.example`,
}));
const firstUser = users[0]!;
const replacement = users[1]!;
const oauthUser = users[2]!;
const tokens = new Map<string, (typeof users)[number]>();
let revision = 0;
function session(user: (typeof users)[number]) {
  const access_token = [
    Buffer.from(JSON.stringify({ alg: "HS256", typ: "JWT" })).toString(
      "base64url",
    ),
    Buffer.from(
      JSON.stringify({
        sub: user.id,
        session_id: user.session,
        aud: "authenticated",
        role: "authenticated",
        exp: Math.floor(Date.now() / 1000) + 3600,
        iat: Math.floor(Date.now() / 1000),
        rev: ++revision,
      }),
    ).toString("base64url"),
    "c3ludGhldGlj",
  ].join(".");
  tokens.set(access_token, user);
  return {
    access_token,
    refresh_token: `synthetic_refresh_${user.name}`,
    token_type: "bearer",
    expires_in: 3600,
    user: {
      id: user.id,
      aud: "authenticated",
      role: "authenticated",
      email: user.email,
      app_metadata: { provider: "email", providers: ["email"] },
      user_metadata: {},
      created_at: new Date().toISOString(),
    },
  };
}
type Operation =
  | { kind: "read" | "subscribe" | "refresh" | "logout" | "pkce" }
  | { kind: "retire"; subject: string }
  | { kind: "verify"; email: string }
  | { kind: "exchange"; flow: string };
async function perform(page: Page, operation: Operation) {
  return page.evaluate(async (operation) => {
    const source = performance
      .getEntriesByType("resource")
      .map((entry) => new URL(entry.name))
      .find(
        (url) =>
          url.origin === location.origin &&
          url.pathname === "/src/auth/services.ts",
      );
    if (!source)
      throw new Error("Actual application services module was not loaded.");
    const mod: typeof import("../src/auth/services") = await import(
      source.href
    );
    const selected = mod.defaultApplicationServices();
    if (selected.status !== "ready")
      throw new Error("Owned app is unconfigured.");
    const { services } = selected;
    switch (operation.kind) {
      case "read": {
        const value = await services.auth.getSession();
        return {
          subject: value.data.session?.user.id ?? null,
          error: value.error !== null,
        };
      }
      case "subscribe": {
        const events: string[] = [];
        services.auth.onAuthStateChange((event, value) => {
          events.push(`${event}:${value?.user.id ?? "none"}`);
          document.documentElement.dataset["sdkEvents"] =
            JSON.stringify(events);
        });
        return null;
      }
      case "retire":
        await services.persistence.retireSubject(operation.subject);
        return null;
      case "verify": {
        try {
          const value = await services.auth.verifyOtp({
            email: operation.email,
            token: "123456",
            type: "email",
          });
          return {
            rejected: value.error !== null,
            subject: value.data.session?.user.id ?? null,
          };
        } catch {
          return { rejected: true, subject: null };
        }
      }
      case "refresh": {
        const value = await services.auth.refreshSession();
        return {
          subject: value.data.session?.user.id ?? null,
          error: value.error !== null,
        };
      }
      case "logout":
        return { ended: await mod.endCurrentSession(services) };
      case "pkce": {
        const value = await services.auth.signInWithOAuth({
          provider: "google",
          options: {
            skipBrowserRedirect: true,
            redirectTo: `${location.origin}/auth/callback`,
          },
        });
        if (value.error || !value.data.url)
          throw new Error("Controlled PKCE start failed.");
        const target = new URL(
          new URL(value.data.url).searchParams.get("redirect_to") ?? "",
        );
        return { flow: target.searchParams.get("sb_flow_id") };
      }
      case "exchange": {
        const value = await services.auth.exchangeCodeForSession(
          "synthetic_oauth_code",
          { flowId: operation.flow },
        );
        return {
          subject: value.data.session?.user.id ?? null,
          error: value.error !== null,
        };
      }
    }
  }, operation);
}
function paused(session: CDPSession) {
  return new Promise<void>((done, reject) => {
    const timeout = setTimeout(
      () =>
        reject(
          new Error("Owned erasure did not reach compare/remove barrier."),
        ),
      10000,
    );
    session.once("Debugger.paused", () => {
      clearTimeout(timeout);
      done();
    });
  });
}
const stage = resolve(
  "services/application/target",
  `auth-storage-${crypto.randomUUID()}`,
);
await mkdir(stage, { mode: 0o700 });
const browser = await chromium.launch({ headless: true });
const context = await browser.newContext();
const checks: string[] = [];
const apiObservations: string[] = [];
let holdOldRefresh = false;
let refreshWaiting = false;
let releaseOldRefresh = () => {};
const oldRefreshGate = new Promise<void>((done) => {
  releaseOldRefresh = done;
});
try {
  const headers = {
    "access-control-allow-origin": "*",
    "access-control-allow-headers": "*",
    "access-control-allow-methods": "GET,POST,DELETE,OPTIONS",
    "content-type": "application/json",
  };
  const deletion = new DeletionFixture(headers);
  await context.route(`${auth}/auth/v1/**`, async (route) => {
    const request = route.request();
    const url = new URL(request.url());
    if (request.method() === "OPTIONS") {
      await route.fulfill({ status: 204, headers });
      return;
    }
    if (url.pathname.endsWith("/otp")) {
      await route.fulfill({ status: 200, headers, body: "{}" });
      return;
    }
    if (url.pathname.endsWith("/logout")) {
      await route.fulfill({ status: 204, headers });
      return;
    }
    const body: unknown = request.postDataJSON();
    if (typeof body !== "object" || body === null)
      throw new Error("Controlled Auth body missing.");
    let user = oauthUser;
    if (url.pathname.endsWith("/verify")) {
      if (!("email" in body)) throw new Error("Controlled email missing.");
      const found = users.find((item) => item.email === body.email);
      if (!found) throw new Error("Unknown controlled email.");
      user = found;
    } else if (url.searchParams.get("grant_type") === "refresh_token") {
      if (!("refresh_token" in body))
        throw new Error("Controlled refresh missing.");
      const found = users.find(
        (item) => `synthetic_refresh_${item.name}` === body.refresh_token,
      );
      if (!found) throw new Error("Unknown controlled refresh.");
      user = found;
    } else if (url.searchParams.get("grant_type") !== "pkce")
      throw new Error("Unexpected controlled Auth path.");
    if (
      holdOldRefresh &&
      url.searchParams.get("grant_type") === "refresh_token" &&
      user.id === firstUser.id
    ) {
      refreshWaiting = true;
      await oldRefreshGate;
    }
    await route.fulfill({
      status: 200,
      headers,
      body: JSON.stringify(session(user)),
    });
  });
  await context.route(`${api}/**`, async (route) => {
    const request = route.request();
    if (request.method() === "OPTIONS") {
      await route.fulfill({ status: 204, headers });
      return;
    }
    const user = tokens.get(
      (request.headers()["authorization"] ?? "").replace(/^Bearer /, ""),
    );
    if (await deletion.handle(route, user)) return;
    if (!user) {
      await route.fulfill({
        status: 401,
        headers,
        body: JSON.stringify({
          status: "fail",
          data: { session: "unauthorized" },
        }),
      });
      return;
    }
    const path = new URL(request.url()).pathname;
    let data: unknown = { items: [], nextCursor: null };
    if (path === "/v1/session") {
      apiObservations.push(user.id);
      data = {
        subjectId: user.id,
        sessionId: user.session,
        method: "email_code",
        state: deletion.states.has(user.account)
          ? "deletion_pending"
          : "active",
        accountId: user.account,
      };
    }
    if (path === "/v1/account")
      data = {
        accountId: user.account,
        username: `fixture_${user.name}`,
        state: "active",
        appearanceRevision: "1",
        defaults: {
          backId: user.id,
          sleeveId: user.id,
          diceId: user.id,
          playmatId: null,
        },
        wallet: { owned: "100", held: "0", available: "100" },
      };
    await route.fulfill({
      status: 200,
      headers,
      body: JSON.stringify({ status: "success", data }),
    });
  });
  const first = await context.newPage();
  const second = await context.newPage();
  await Promise.all([
    first.goto(`${app}/collection`),
    second.goto(`${app}/collection`),
  ]);
  await first.getByLabel("Email", { exact: true }).fill(firstUser.email);
  await first
    .getByRole("button", { name: "Continue with email", exact: true })
    .click();
  await first.getByLabel("Six-digit code").fill("123456");
  await expect(first.getByLabel("Deck play mode")).toBeVisible();
  expect((await perform(first, { kind: "read" }))?.subject).toBe(firstUser.id);
  await perform(first, { kind: "subscribe" });
  const flow = (await perform(first, { kind: "pkce" }))?.flow;
  if (typeof flow !== "string" || !/^[a-f0-9]{32}$/.test(flow))
    throw new Error("SDK flow ID missing.");
  const key = `spellwake.auth.v1:${new URL(auth).host}`;
  const verifierKey = `${key}-flow-${flow}-code-verifier`;
  const verifier = await first.evaluate(
    (key) => localStorage.getItem(key),
    verifierKey,
  );
  expect(verifier).not.toBeNull();
  checks.push(
    "actual email/code UI -> pinned SDK supported adapter -> React account API authority; independent PKCE slot created",
  );
  holdOldRefresh = true;
  const staleRefresh = perform(second, { kind: "refresh" });
  await expect.poll(() => refreshWaiting).toBe(true);
  const heldDuringNetwork = await first.evaluate(
    async (key) =>
      (await navigator.locks.query()).held?.some(
        (lock) => lock.name === `spellwake.auth-storage.v1:${key}`,
      ) ?? false,
    key,
  );
  expect(heldDuringNetwork).toBe(false);
  const debuggerSession = await context.newCDPSession(first);
  await debuggerSession.send("Debugger.enable");
  await first.evaluate((key) => {
    const original = Storage.prototype.removeItem;
    Storage.prototype.removeItem = function (this: Storage, candidate: string) {
      if (candidate === key) {
        Storage.prototype.removeItem = original;
        // eslint-disable-next-line no-debugger -- Fixture-only pause after the subject comparison, before actual deletion.
        debugger;
      }
      return original.call(this, candidate);
    };
  }, key);
  const barrier = paused(debuggerSession);
  const erasure = perform(first, { kind: "retire", subject: firstUser.id });
  await barrier;
  // A real verifyOtp attempts its SDK setItem while the eraser holds the mutex.
  await second.evaluate(async () => {
    const source = performance
      .getEntriesByType("resource")
      .map((entry) => new URL(entry.name))
      .find((url) => url.pathname === "/src/auth/services.ts");
    if (!source) throw new Error("Services unavailable.");
    const mod: typeof import("../src/auth/services") = await import(
      source.href
    );
    const selected = mod.defaultApplicationServices();
    if (selected.status !== "ready") throw new Error("Unconfigured.");
    const storage = selected.services.persistence.storage;
    const original = storage.setItem.bind(storage);
    storage.setItem = async (candidate, value) => {
      document.documentElement.dataset["sdkSaveAttempted"] = "yes";
      return original(candidate, value);
    };
  });
  const replacementSave = perform(second, {
    kind: "verify",
    email: replacement.email,
  });
  await expect
    .poll(() =>
      second.evaluate(async (key) => {
        const state = await navigator.locks.query();
        return (
          document.documentElement.dataset["sdkSaveAttempted"] === "yes" &&
          state.pending?.some(
            (lock) => lock.name === `spellwake.auth-storage.v1:${key}`,
          )
        );
      }, key),
    )
    .toBe(true);
  await debuggerSession.send("Debugger.resume");
  await erasure;
  expect((await replacementSave)?.subject).toBe(replacement.id);
  await debuggerSession.detach();
  await expect
    .poll(async () => (await perform(first, { kind: "read" }))?.subject)
    .toBe(replacement.id);
  await expect(first.getByLabel("Deck play mode")).toBeVisible();
  expect(
    await first.evaluate((key) => localStorage.getItem(key), verifierKey),
  ).toBe(verifier);
  checks.push(
    "actual eraser paused after compare/before remove, concurrent verifyOtp save queues; replacement persists after both finish and PKCE survives",
  );
  holdOldRefresh = false;
  releaseOldRefresh();
  expect((await staleRefresh)?.error).toBe(true);
  expect((await perform(second, { kind: "read" }))?.subject).toBe(
    replacement.id,
  );
  checks.push(
    "old refresh held in HTTP owns no Auth storage mutex; after erasure and replacement its late response is discarded without overwriting credentials",
  );
  const eventCount = await first.evaluate(
    () =>
      JSON.parse(document.documentElement.dataset["sdkEvents"] ?? "[]")
        .length as number,
  );
  expect(
    (await perform(first, { kind: "verify", email: firstUser.email }))
      ?.rejected,
  ).toBe(true);
  expect((await perform(first, { kind: "read" }))?.subject).toBe(
    replacement.id,
  );
  const lateEvents: unknown = await first.evaluate(
    (start) =>
      JSON.parse(document.documentElement.dataset["sdkEvents"] ?? "[]").slice(
        start,
      ),
    eventCount,
  );
  expect(JSON.stringify(lateEvents)).not.toContain(firstUser.id);
  checks.push(
    "late retired-subject verifyOtp throws before successful SIGNED_IN and cannot overwrite replacement",
  );
  await first
    .getByLabel("Deck play mode")
    .evaluate((element) =>
      element.setAttribute("data-sdk-fixture-retained", "yes"),
    );
  const before = apiObservations.length;
  await second.evaluate((key) => {
    const channel = new BroadcastChannel(key);
    channel.postMessage({ event: "SIGNED_OUT", session: null });
    channel.close();
  }, key);
  await expect.poll(() => apiObservations.length).toBeGreaterThan(before);
  await expect(
    first.locator('[data-sdk-fixture-retained="yes"]'),
  ).toBeVisible();
  expect((await perform(first, { kind: "read" }))?.subject).toBe(
    replacement.id,
  );
  checks.push(
    "late old SIGNED_OUT broadcast triggers public SDK re-read; replacement interface is retained",
  );
  expect((await perform(first, { kind: "exchange", flow }))?.subject).toBe(
    oauthUser.id,
  );
  expect((await perform(first, { kind: "refresh" }))?.subject).toBe(
    oauthUser.id,
  );
  expect((await perform(first, { kind: "logout" }))?.ended).toBe(true);
  await expect(first.getByLabel("Email", { exact: true })).toBeVisible();
  checks.push(
    "public PKCE exchange and refresh persist another subject; ordinary local sign-out still removes its session",
  );
  const negatives = await first.evaluate(async () => {
    const path = "/src/auth/storage.ts";
    const mod: typeof import("../src/auth/storage") = await import(path);
    const persistence = mod.createAuthPersistence(
      "https://negative.sdk-fixture.example",
    );
    const subject = crypto.randomUUID();
    const other = crypto.randomUUID();
    const raw = JSON.stringify({
      user: { id: subject },
      access_token: "synthetic",
    });
    const failure = async (work: () => Promise<unknown>) => {
      try {
        await work();
        return false;
      } catch {
        return true;
      }
    };
    await persistence.storage.setItem(persistence.key, raw);
    await persistence.storage.setItem(
      `${persistence.key}-user`,
      JSON.stringify({ user: { id: other } }),
    );
    const original = Storage.prototype.removeItem;
    Storage.prototype.removeItem = function (this: Storage, key: string) {
      if (key !== persistence.key) original.call(this, key);
    };
    const droppedRemoval = await failure(() =>
      persistence.retireSubject(subject),
    );
    Storage.prototype.removeItem = original;
    const remains = localStorage.getItem(persistence.key) === raw;
    const closedRead = await failure(async () =>
      persistence.storage.getItem(persistence.key),
    );
    await persistence.retireSubject(subject);
    const retryRemoved = localStorage.getItem(persistence.key) === null;
    const companionPreserved =
      JSON.parse(localStorage.getItem(`${persistence.key}-user`) ?? "null")
        ?.user?.id === other;
    const another = crypto.randomUUID();
    const anotherRaw = JSON.stringify({
      user: { id: another },
      access_token: "synthetic_other",
    });
    await persistence.storage.setItem(persistence.key, anotherRaw);
    const originalSet = Storage.prototype.setItem;
    Storage.prototype.setItem = function (
      this: Storage,
      key: string,
      value: string,
    ) {
      if (!key.startsWith(`spellwake.auth-retired.v1:${persistence.key}:`))
        originalSet.call(this, key, value);
    };
    const droppedFence = await failure(() =>
      persistence.retireSubject(another),
    );
    Storage.prototype.setItem = originalSet;
    const preservedWithoutFence =
      localStorage.getItem(persistence.key) === anotherRaw;
    await persistence.retireSubject(another);
    localStorage.setItem(persistence.key, "corrupt fixture bytes");
    const corrupt = await failure(() => persistence.retireSubject(subject));
    const corruptPreserved =
      localStorage.getItem(persistence.key) === "corrupt fixture bytes";
    return {
      droppedRemoval,
      remains,
      closedRead,
      retryRemoved,
      companionPreserved,
      corrupt,
      corruptPreserved,
      droppedFence,
      preservedWithoutFence,
    };
  });
  expect(Object.values(negatives).every(Boolean)).toBe(true);
  checks.push(
    "dropped credential removal blocks success/read, retry removes matching record, unrelated companion and corrupt bytes remain protected",
  );
  await qualifyDeletionBrowser({
    page: first,
    second,
    fixture: deletion,
    api,
    auth,
    users: [oauthUser, replacement],
    signIn: async (page, email) =>
      (await perform(page, { kind: "verify", email }))?.subject ?? null,
    signOut: async (page) =>
      (await perform(page, { kind: "logout" }))?.ended ?? false,
    readSubject: async (page) =>
      (await perform(page, { kind: "read" }))?.subject ?? null,
    checks,
  });
  if (JSON.stringify(await sourceHashes()) !== JSON.stringify(sources))
    throw new Error("Source changed during browser qualification.");
  await writeFile(
    resolve(stage, "result.json"),
    JSON.stringify(
      {
        passed: true,
        sources,
        checks,
        limitations: [
          "controlled HTTP responses, no actual managed Auth authority",
          "controlled deletion root/SDK consumer; no actual managed finalizer/Auth/Admin or native OS qualification",
        ],
      },
      null,
      2,
    ),
    { mode: 0o600 },
  );
  console.log(JSON.stringify({ passed: true, checks: checks.length, stage }));
} catch (error) {
  const page = context.pages().find((value) => !value.isClosed());
  if (page)
    await page
      .screenshot({
        path: resolve(stage, "failed-browser.png"),
        fullPage: true,
      })
      .catch(() => {});
  throw error;
} finally {
  releaseOldRefresh();
  await context.close();
  await browser.close();
}

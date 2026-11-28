/** Connected deletion UI/recovery over the existing controlled SDK/HTTP fixture. */
import { expect, type Page, type Route } from "@playwright/test";
import { mkdir } from "node:fs/promises";
import { resolve } from "node:path";
import {
  requestDeletionSchema,
  observeDeletionSchema,
} from "@spellwake/application-contracts";

export type DeletionUser = {
  id: string;
  account: string;
  session: string;
  email: string;
};
type State = {
  requestId: string;
  hash: string;
  phase: "pending" | "finalizing" | "deleted";
  operation: string;
  cleanup: string;
};
export class DeletionFixture {
  readonly states = new Map<string, State>();
  readonly admissions = new Map<string, number>();
  readonly observations = new Map<string, number>();
  omitNext = false;
  dropAckNext = false;
  private release = () => {};
  private gate: Promise<void> | null = null;
  heldAccount: string | null = null;
  hold(account: string) {
    this.heldAccount = account;
    this.gate = new Promise<void>((done) => {
      this.release = done;
    });
  }
  resume() {
    this.release();
    this.gate = null;
    this.heldAccount = null;
  }
  phase(account: string, phase: State["phase"]) {
    const value = this.states.get(account);
    if (!value) throw new Error("Missing controlled deletion state.");
    value.phase = phase;
  }
  constructor(private readonly headers: Record<string, string>) {}
  async handle(route: Route, user: DeletionUser | undefined): Promise<boolean> {
    const request = route.request();
    const path = new URL(request.url()).pathname;
    if (path === "/v1/account/deletion") {
      if (!user)
        throw new Error("Controlled admission requires original bearer.");
      const input = requestDeletionSchema.parse(request.postDataJSON());
      this.admissions.set(
        user.account,
        (this.admissions.get(user.account) ?? 0) + 1,
      );
      if (this.omitNext) {
        this.omitNext = false;
        await route.abort("failed");
        return true;
      }
      const old = this.states.get(user.account);
      if (
        old &&
        (old.requestId !== input.requestId || old.hash !== input.statusKeyHash)
      )
        throw new Error("Deletion retry changed immutable terms.");
      if (!old)
        this.states.set(user.account, {
          requestId: input.requestId,
          hash: input.statusKeyHash,
          phase: "pending",
          operation: crypto.randomUUID(),
          cleanup: crypto.randomUUID(),
        });
      if (this.dropAckNext) {
        this.dropAckNext = false;
        await route.abort("failed");
        return true;
      }
      await route.fulfill({
        status: 200,
        headers: this.headers,
        body: JSON.stringify({
          status: "success",
          data: { resourceId: user.account },
        }),
      });
      return true;
    }
    if (path === "/v1/account/deletion/observe") {
      if (request.headers()["authorization"])
        throw new Error("Observer leaked an account bearer.");
      const input = observeDeletionSchema.parse(request.postDataJSON());
      this.observations.set(
        input.accountId,
        (this.observations.get(input.accountId) ?? 0) + 1,
      );
      const hash = new Bun.CryptoHasher("sha256")
        .update(Buffer.from(input.statusKey, "base64url"))
        .digest("hex");
      const state = this.states.get(input.accountId);
      if (
        !state ||
        state.requestId !== input.requestId ||
        state.hash !== hash
      ) {
        await route.fulfill({
          status: 404,
          headers: this.headers,
          body: JSON.stringify({
            status: "fail",
            data: { observer: "unavailable" },
          }),
        });
        return true;
      }
      const data = {
        accountId: input.accountId,
        requestId: input.requestId,
        phase: state.phase,
        ...(state.phase !== "pending"
          ? { retirementOperationId: state.operation }
          : {}),
        ...(state.phase === "deleted"
          ? { authCleanupReceiptId: state.cleanup }
          : {}),
      };
      const held = this.heldAccount === input.accountId ? this.gate : null;
      if (held) await held;
      try {
        await route.fulfill({
          status: 200,
          headers: this.headers,
          body: JSON.stringify({ status: "success", data }),
        });
      } catch {
        if (!request.failure())
          throw new Error("Controlled observer reply failed unexpectedly.");
      }
      return true;
    }
    if (user && this.states.get(user.account)?.phase === "deleted") {
      await route.fulfill({
        status: 401,
        headers: this.headers,
        body: JSON.stringify({
          status: "fail",
          data: { session: "unauthorized" },
        }),
      });
      return true;
    }
    return false;
  }
}
async function settings(page: Page) {
  await page
    .getByRole("button", { name: "Open account menu", exact: true })
    .click();
  await page
    .getByRole("menuitem", { name: "Default settings", exact: true })
    .click();
  await expect(page.getByRole("dialog")).toBeVisible();
}
async function confirm(page: Page) {
  await page
    .getByRole("button", { name: /^(Delete account|Retry request)$/ })
    .click();
  await expect(
    page.getByRole("dialog", { name: "Delete your account?" }),
  ).toBeVisible();
}
async function capture(
  page: Page,
  review: string,
  name: string,
  width: number,
) {
  await page.setViewportSize({ width, height: 960 });
  await page.screenshot({ path: resolve(review, name), fullPage: true });
}
export async function qualifyDeletionBrowser(args: {
  page: Page;
  second: Page;
  fixture: DeletionFixture;
  api: string;
  auth: string;
  users: readonly DeletionUser[];
  signIn: (page: Page, email: string) => Promise<string | null>;
  readSubject: (page: Page) => Promise<string | null>;
  signOut: (page: Page) => Promise<boolean>;
  checks: string[];
}) {
  const { page, second, fixture, api, auth, users, signIn, checks } = args;
  const original = users[0]!;
  const replacement = users[1]!;
  const review = resolve("work/review/account-deletion");
  await mkdir(review, { recursive: true });
  const key = (user: DeletionUser) =>
    `spellwake.deletion.v1:${api}:${user.account}`;
  const sdkKey = `spellwake.auth.v1:${new URL(auth).host}`;
  expect(await signIn(page, original.email)).toBe(original.id);
  await expect(page.getByLabel("Deck play mode")).toBeVisible();
  await settings(page);
  await confirm(page);
  await capture(page, review, "desktop.png", 1440);
  await capture(page, review, "mobile.png", 390);
  await page.getByRole("button", { name: "Cancel", exact: true }).click();
  expect(fixture.admissions.get(original.account) ?? 0).toBe(0);
  await confirm(page);
  fixture.omitNext = true;
  await page
    .getByRole("button", { name: "Delete my account", exact: true })
    .click();
  await expect(page.getByRole("alert")).toContainText(
    "Deletion hasn’t been confirmed",
  );
  const raw = await page.evaluate(
    (key) => localStorage.getItem(key),
    key(original),
  );
  if (!raw) throw new Error("Prepared observer missing.");
  await page.reload();
  await expect
    .poll(() => fixture.observations.get(original.account) ?? 0)
    .toBeGreaterThan(0);
  expect(fixture.admissions.get(original.account)).toBe(1);
  await settings(page);
  await expect(
    page.getByText(
      "Deletion hasn’t been confirmed. Check progress or retry the request.",
      { exact: true },
    ),
  ).toBeVisible();
  await page
    .getByRole("button", { name: "Retry request", exact: true })
    .click();
  fixture.dropAckNext = true;
  await page
    .getByRole("button", { name: "Delete my account", exact: true })
    .click();
  await expect.poll(() => fixture.states.has(original.account)).toBe(true);
  await page.reload();
  await settings(page);
  await expect(
    page.getByText(
      "Waiting for committed matches, predictions and payments to finish.",
      { exact: true },
    ),
  ).toBeVisible();
  expect(fixture.admissions.get(original.account)).toBe(2);
  const originalRaw = await page.evaluate(
    (key) => localStorage.getItem(key),
    key(original),
  );
  if (!originalRaw) throw new Error("Recovery source missing.");
  // Persisted metadata remains identical after dropped acknowledgment/reload.
  expect(originalRaw).toBe(raw);
  checks.push(
    "explicit confirmation required; unsent prepared reload never auto-posts; confirmed retry retains exact nonce/request and dropped ack recovers by observer",
  );
  await page.getByRole("button", { name: "Close dialog", exact: true }).click();
  expect(await args.signOut(page)).toBe(true);
  await expect(page.getByLabel("Email", { exact: true })).toBeVisible();
  await page.reload();
  await settings(page);
  await expect(
    page.getByText(
      "Waiting for committed matches, predictions and payments to finish.",
      { exact: true },
    ),
  ).toBeVisible();
  expect(
    await page.evaluate((key) => localStorage.getItem(key), key(original)),
  ).toBe(originalRaw);
  expect(fixture.admissions.get(original.account)).toBe(2);
  await page.getByRole("button", { name: "Close dialog", exact: true }).click();
  expect(await signIn(page, original.email)).toBe(original.id);
  await expect(page.getByLabel("Deck play mode")).toBeVisible();
  checks.push(
    "ordinary sign-out and reload preserve the observer and expose pending progress without bearer or a new deletion POST",
  );
  fixture.phase(original.account, "deleted");
  fixture.hold(original.account);
  const before = fixture.observations.get(original.account) ?? 0;
  await settings(page);
  await page.getByRole("button", { name: "Check again", exact: true }).click();
  await expect
    .poll(() => fixture.observations.get(original.account) ?? 0)
    .toBeGreaterThan(before);
  expect(await signIn(second, replacement.email)).toBe(replacement.id);
  await expect(page.getByLabel("Deck play mode")).toBeVisible();
  await expect(page.getByRole("dialog")).toHaveCount(0);
  await page
    .getByLabel("Deck play mode")
    .evaluate((element) =>
      element.setAttribute("data-deletion-preserved", "yes"),
    );
  const otherKey = `spellwake.profile-defaults.v1:${api}:${replacement.account}`;
  const oldKey = `spellwake.profile-defaults.v1:${api}:${original.account}`;
  await second.evaluate(
    async ({
      originalKey,
      forgedKey,
      originalRaw,
      replacement,
      otherKey,
      oldKey,
    }) => {
      localStorage.setItem(otherKey, "replacement private fixture");
      localStorage.setItem(oldKey, "original private fixture");
      const path = "/src/data/deletion-journal.ts";
      const mod: typeof import("../src/data/deletion-journal") = await import(
        path
      );
      const value = mod.deletionJournalSchema.parse(JSON.parse(originalRaw));
      localStorage.setItem(
        originalKey,
        JSON.stringify({ ...value, subjectId: replacement.id }),
      );
      localStorage.setItem(
        forgedKey,
        JSON.stringify({
          version: 1,
          apiOrigin: value.apiOrigin,
          authOrigin: value.authOrigin,
          accountId: replacement.account,
          subjectId: replacement.id,
          requestId: value.requestId,
          statusSeed: value.statusSeed,
          state: "prepared",
        }),
      );
      const events = "/src/data/deletion.ts";
      const data: typeof import("../src/data/deletion") = await import(events);
      data.refreshDeletionRegistry(originalKey);
      data.refreshDeletionRegistry(forgedKey);
    },
    {
      originalKey: key(original),
      forgedKey: key(replacement),
      originalRaw,
      replacement,
      otherKey,
      oldKey,
    },
  );
  fixture.resume();
  await expect
    .poll(() => fixture.observations.get(replacement.account) ?? 0)
    .toBeGreaterThan(0);
  expect(fixture.admissions.get(replacement.account) ?? 0).toBe(0);
  expect(await args.readSubject(page)).toBe(replacement.id);
  expect(await page.evaluate((key) => localStorage.getItem(key), oldKey)).toBe(
    "original private fixture",
  );
  await expect(page.locator('[data-deletion-preserved="yes"]')).toBeVisible();
  checks.push(
    "late old observer and valid-looking subject/copy tampering cannot erase replacement or auto-admit forged prepared consent",
  );
  const routeBefore = page.url();
  await second.evaluate(
    async ({ originalKey, forgedKey, originalRaw }) => {
      localStorage.removeItem(forgedKey);
      localStorage.setItem(originalKey, originalRaw);
      const path = "/src/data/deletion.ts";
      const data: typeof import("../src/data/deletion") = await import(path);
      data.refreshDeletionRegistry(forgedKey);
      data.refreshDeletionRegistry(originalKey);
    },
    { originalKey: key(original), forgedKey: key(replacement), originalRaw },
  );
  await expect
    .poll(
      () => page.evaluate((key) => localStorage.getItem(key), key(original)),
      { timeout: 20000 },
    )
    .toBeNull();
  expect(await args.readSubject(page)).toBe(replacement.id);
  expect(
    await page.evaluate((key) => localStorage.getItem(key), oldKey),
  ).toBeNull();
  expect(
    await page.evaluate((key) => localStorage.getItem(key), otherKey),
  ).toBe("replacement private fixture");
  await expect(page.locator('[data-deletion-preserved="yes"]')).toBeVisible();
  expect(page.url()).toBe(routeBefore);
  checks.push(
    "restored immutable observer finishes the old account silently while replacement credentials, cache-backed control, payload and route survive",
  );
  await second.close();
  await settings(page);
  await confirm(page);
  expect(
    await page
      .getByRole("dialog")
      .evaluate((element) => element.contains(document.activeElement)),
  ).toBe(true);
  await capture(page, review, "desktop-confirm.png", 1440);
  await capture(page, review, "mobile-confirm.png", 390);
  await page
    .getByRole("button", { name: "Delete my account", exact: true })
    .click();
  await expect(
    page.getByText(
      "Waiting for committed matches, predictions and payments to finish.",
      { exact: true },
    ),
  ).toBeVisible();
  const replacementRaw = await page.evaluate(
    (key) => localStorage.getItem(key),
    key(replacement),
  );
  if (!replacementRaw) throw new Error("Accepted observer missing.");
  await capture(page, review, "desktop-pending.png", 1440);
  await capture(page, review, "mobile-pending.png", 390);
  fixture.phase(replacement.account, "finalizing");
  await page.getByRole("button", { name: "Check again", exact: true }).click();
  await expect(
    page.getByText("Finishing account deletion…", { exact: true }),
  ).toBeVisible();
  await capture(page, review, "desktop-finalizing.png", 1440);
  await capture(page, review, "mobile-finalizing.png", 390);
  await page.evaluate((key) => {
    const original = Storage.prototype.removeItem;
    Storage.prototype.removeItem = function (this: Storage, candidate: string) {
      if (candidate !== key) original.call(this, candidate);
    };
    window.addEventListener(
      "spellwake.fixture.restore-sdk",
      () => {
        Storage.prototype.removeItem = original;
      },
      { once: true },
    );
  }, sdkKey);
  fixture.phase(replacement.account, "deleted");
  await page.getByRole("button", { name: "Check again", exact: true }).click();
  await expect(
    page.getByText("Couldn’t clear local account data. Check again.", {
      exact: true,
    }),
  ).toBeVisible({ timeout: 20000 });
  expect(
    await page.evaluate((key) => localStorage.getItem(key), key(replacement)),
  ).toBe(replacementRaw);
  expect(
    await page.evaluate((key) => localStorage.getItem(key), sdkKey),
  ).not.toBeNull();
  expect(
    await page.evaluate((key) => localStorage.getItem(key), otherKey),
  ).toBeNull();
  await capture(page, review, "desktop-retry.png", 1440);
  await capture(page, review, "mobile-retry.png", 390);
  await page.evaluate(() =>
    window.dispatchEvent(new Event("spellwake.fixture.restore-sdk")),
  );
  await page.getByRole("button", { name: "Check again", exact: true }).click();
  await expect(
    page.getByRole("dialog", { name: "Account deleted", exact: true }),
  ).toBeVisible({ timeout: 20000 });
  expect(
    await page.evaluate((key) => localStorage.getItem(key), key(replacement)),
  ).toBeNull();
  expect(
    await page.evaluate((key) => localStorage.getItem(key), sdkKey),
  ).toBeNull();
  expect(fixture.admissions.get(replacement.account)).toBe(1);
  await capture(page, review, "desktop-complete.png", 1440);
  await capture(page, review, "mobile-complete.png", 390);
  await page.getByRole("button", { name: "Close", exact: true }).click();
  await expect(page.getByLabel("Email", { exact: true })).toBeVisible();
  checks.push(
    "pending/finalizing never imply completion; dropped SDK removal retains observer; explicit check resumes exact-proof cleanup and clears observer only after public SDK verification",
  );
  checks.push(
    "desktop1440 and compact390 confirmation/progress/retry/completion captures; keyboard focus stays inside confirmation",
  );
}

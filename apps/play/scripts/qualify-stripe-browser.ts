/** Actual owned Auth/SMTP/API consumer. Only a reservation acknowledgment is fault-injected. */
import { chromium, expect } from "@playwright/test";
import { readFile, writeFile } from "node:fs/promises";
import { join } from "node:path";
import { z } from "zod";
import {
  saveDeckSchema,
  deckCommandSchema,
  commandAckSchema,
  requestDeletionSchema,
  socialCommandSchema,
} from "@spellwake/application-contracts";

const config = z
  .strictObject({
    app: z.url(),
    api: z.url(),
    auth: z.url(),
    mail: z.url(),
    email: z.email(),
    cosmetic: z.uuid(),
    stage: z.string(),
    csp: z.string(),
    catalogueCard: z.strictObject({
      definitionId: z.uuid(),
      printingId: z.uuid(),
      name: z.string(),
      collectorNumber: z.string(),
      finish: z.string(),
      language: z.string(),
      setName: z.string(),
      setCode: z.string(),
    }),
  })
  .parse(JSON.parse(await readFile(process.argv[2] ?? "", "utf8")));
for (const url of [config.app, config.api, config.auth, config.mail])
  if (new URL(url).hostname !== "127.0.0.1")
    throw new Error("Only the owned loopback services may be tested.");
async function checkpoint(name: string, data: unknown) {
  await writeFile(
    join(config.stage, `browser-${name}.json`),
    JSON.stringify(data),
    { mode: 0o600, flag: "wx" },
  );
}
async function waitFor(name: string, timeoutMs = 90000): Promise<void> {
  const deadline = Date.now() + timeoutMs;
  while (Date.now() < deadline) {
    if (await Bun.file(join(config.stage, `browser-${name}.json`)).exists())
      return;
    await Bun.sleep(100);
  }
  throw new Error(
    "Owned browser phase did not receive its expected completion.",
  );
}
const browser = await chromium.launch({ headless: true });
try {
  const context = await browser.newContext({
    viewport: { width: 1440, height: 960 },
    serviceWorkers: "block",
  });
  // The intercepted static response has no IP address-space metadata. Authorize
  // only this owned fixture's page to contact its loopback Auth/API services.
  await context.grantPermissions(["local-network-access"], {
    origin: config.app,
  });
  // Apply the exact native resource/connect policy to the actual static build.
  await context.route(config.app + "/**", async (route) => {
    const response = await route.fetch();
    await route.fulfill({
      response,
      headers: { ...response.headers(), "content-security-policy": config.csp },
    });
  });
  const page = await context.newPage();
  page.on("dialog", (dialog) => {
    void (dialog.type() === "beforeunload"
      ? dialog.accept()
      : dialog.dismiss());
  });
  const saveBodies: z.infer<typeof saveDeckSchema>[] = [];
  let droppedSave = false;
  let droppedDisposition = false;
  await page.route(config.api + "/v1/decks/*/revisions", async (route) => {
    if (route.request().method() !== "POST") {
      await route.continue();
      return;
    }
    saveBodies.push(saveDeckSchema.parse(route.request().postDataJSON()));
    if (!droppedSave) {
      const response = await route.fetch();
      expect(response.status()).toBe(202);
      z.strictObject({
        status: z.literal("success"),
        data: commandAckSchema,
      }).parse(await response.json());
      droppedSave = true;
      await route.abort("failed");
      return;
    }
    await route.continue();
  });
  await page.route(
    config.api + "/v1/decks/commands/disposition",
    async (route) => {
      const input = deckCommandSchema.parse(route.request().postDataJSON());
      if (input.kind === "save" && !droppedDisposition) {
        droppedDisposition = true;
        await route.abort("failed");
        return;
      }
      await route.continue();
    },
  );
  const network = await context.newCDPSession(page);
  await network.send("Network.enable");
  network.on("Network.loadingFailed", (event) => {
    console.error(
      JSON.stringify({
        kind: "owned-browser-network-failure",
        error: event.errorText,
        blocked: event.blockedReason,
        cors: event.corsErrorStatus,
      }),
    );
  });
  // Record only endpoint and status, never query strings, payloads or credentials.
  page.on("requestfailed", (request) => {
    const url = new URL(request.url());
    console.error(
      `owned-browser request-failed ${request.method()} ${url.origin}${url.pathname} ${request.failure()?.errorText ?? "unknown"}`,
    );
  });
  page.on("response", (response) => {
    const url = new URL(response.url());
    if (url.origin === config.auth || url.origin === config.api)
      console.error(
        `owned-browser response ${response.request().method()} ${url.pathname} ${response.status()}`,
      );
  });
  let dropped = false;
  const requestIds: string[] = [];
  await page.route(config.api + "/v1/purchases", async (route) => {
    if (route.request().method() !== "POST") {
      await route.continue();
      return;
    }
    const input = z
      .strictObject({ requestId: z.uuid(), cosmeticId: z.uuid() })
      .parse(route.request().postDataJSON());
    requestIds.push(input.requestId);
    if (!dropped) {
      const actual = await route.fetch();
      if (actual.status() === 409) {
        await route.fulfill({ response: actual });
        return;
      }
      dropped = true;
      expect(actual.status()).toBe(202);
      await route.abort("failed");
      return;
    }
    await route.continue();
  });
  await page.goto(config.app + "/collection", {
    waitUntil: "domcontentloaded",
  });
  await expect(
    page.getByRole("heading", { name: "Sign in to Spellwake", exact: true }),
  ).toBeVisible();
  await page.getByLabel("Email", { exact: true }).fill(config.email);
  const sentCode = page.waitForResponse(
    (response) =>
      new URL(response.url()).pathname === "/auth/v1/otp" &&
      response.request().method() === "POST",
  );
  await page
    .getByRole("button", { name: "Continue with email", exact: true })
    .click();
  const codeResponse = await sentCode;
  if (!codeResponse.ok()) {
    const failure = z
      .object({
        code: z.string().optional(),
        error_code: z.string().optional(),
      })
      .parse(await codeResponse.json());
    throw new Error(
      `Owned Auth code delivery failed (${codeResponse.status()}, ${failure.code ?? failure.error_code ?? "unknown"}).`,
    );
  }
  await expect(
    page.getByRole("heading", { name: "Enter your code", exact: true }),
  ).toBeVisible();
  let otp: string | null = null;
  const deadline = Date.now() + 30000;
  while (!otp && Date.now() < deadline) {
    const listed: unknown = await (
      await fetch(config.mail + "/api/v1/messages")
    ).json();
    const messages = z
      .object({
        messages: z.array(
          z.object({
            ID: z.string(),
            To: z.array(z.object({ Address: z.string() })),
          }),
        ),
      })
      .parse(listed);
    for (const message of messages.messages.filter((message) =>
      message.To.some((to) => to.Address === config.email),
    )) {
      const detail: unknown = await (
        await fetch(
          config.mail + "/api/v1/message/" + encodeURIComponent(message.ID),
        )
      ).json();
      const body = z
        .object({ HTML: z.string(), Text: z.string() })
        .parse(detail);
      otp =
        /(?:^|\D)([0-9]{6})(?:\D|$)/.exec(body.Text + " " + body.HTML)?.[1] ??
        null;
      if (otp) break;
    }
    if (!otp) await Bun.sleep(100);
  }
  if (!otp)
    throw new Error(
      "The owned SMTP inbox did not receive a numeric Auth code.",
    );
  await page.getByLabel("Six-digit code").fill(otp);
  await expect(
    page.getByRole("heading", { name: "Choose your username", exact: true }),
  ).toBeVisible();
  await page.getByLabel("Username", { exact: true }).fill("browser_qualified");
  await page
    .getByRole("button", { name: "Create account", exact: true })
    .click();
  await expect(
    page.getByRole("heading", {
      name: "No Collection Play decks",
      exact: true,
    }),
  ).toBeVisible();
  await expect(page.locator('[aria-label="14,500 coins"]')).toBeVisible();
  await checkpoint("registered", { smtp: true, canonicalAccount: true });
  // Use the same real Auth/API realm for social delivery and later deletion.
  // Fault injection discards actual acknowledgments, never fabricates outcomes.
  let droppedMessage = false;
  let droppedSocialLookup = false;
  const socialSends: z.infer<typeof socialCommandSchema>[] = [];
  await page.route(config.api + "/v1/social/commands", async (route) => {
    const input = socialCommandSchema.parse(route.request().postDataJSON());
    if (input.kind !== "send_dm") {
      await route.continue();
      return;
    }
    socialSends.push(input);
    if (!droppedMessage) {
      droppedMessage = true;
      const response = await route.fetch();
      expect(response.status()).toBe(202);
      await route.abort("failed");
      return;
    }
    await route.continue();
  });
  await page.route(
    config.api + "/v1/social/commands/disposition",
    async (route) => {
      const input = socialCommandSchema.parse(route.request().postDataJSON());
      if (input.kind === "send_dm" && !droppedSocialLookup) {
        droppedSocialLookup = true;
        await route.abort("failed");
        return;
      }
      await route.continue();
    },
  );
  await page.getByRole("button", { name: /^Messages/ }).click();
  await page
    .getByRole("button", { name: "Find a conversation", exact: true })
    .click();
  await page
    .getByRole("searchbox", { name: "Search people", exact: true })
    .fill("stripe_qualified");
  await page
    .getByRole("button", { name: /stripe_qualified/ })
    .last()
    .click();
  await page.getByRole("button", { name: "Add friend", exact: true }).click();
  await expect(
    page.getByRole("button", { name: "Cancel friend request", exact: true }),
  ).toBeVisible();
  await checkpoint("social-requested", {});
  await waitFor("social-accepted");
  await expect(
    page.getByRole("button", { name: "Start conversation", exact: true }),
  ).toBeVisible({ timeout: 30000 });
  await page
    .getByRole("button", { name: "Start conversation", exact: true })
    .click();
  const composer = page.getByRole("textbox", {
    name: "Message stripe_qualified",
    exact: true,
  });
  await expect(composer).toBeVisible();
  await composer.fill("Owned social delivery");
  await expect(
    page.getByRole("button", {
      name: "Send message to stripe_qualified",
      exact: true,
    }),
  ).toBeEnabled();
  await page
    .getByRole("button", {
      name: "Send message to stripe_qualified",
      exact: true,
    })
    .click();
  await expect(
    page.getByRole("button", { name: "Recover message", exact: true }),
  ).toBeVisible({ timeout: 30000 });
  await page.reload();
  await page.getByRole("button", { name: /^Messages/ }).click();
  await page
    .getByRole("button", {
      name: "Conversation with stripe_qualified",
      exact: true,
    })
    .click();
  await expect(composer).toHaveValue("Owned social delivery");
  expect(socialSends).toHaveLength(1);
  await page
    .getByRole("button", { name: "Recover message", exact: true })
    .click();
  await expect(composer).toHaveValue("", { timeout: 30000 });
  await expect(
    page
      .getByRole("region", {
        name: "Conversation history with stripe_qualified",
        exact: true,
      })
      .getByText("Owned social delivery", { exact: true }),
  ).toBeVisible();
  expect(socialSends).toHaveLength(1);
  await checkpoint("social-delivered", {
    command: socialSends[0],
    sendPosts: socialSends.length,
  });
  await waitFor("social-incoming");
  await expect(page.getByText("Owned peer reply", { exact: true })).toBeVisible(
    { timeout: 30000 },
  );
  await composer.fill("Retire this unsent social draft");
  await expect(page.getByText("Saving draft…", { exact: true })).toHaveCount(0);
  await expect
    .poll(() =>
      page.evaluate(
        () =>
          Object.keys(localStorage).filter(
            (key) =>
              key.startsWith("spellwake.social-command.v1:") ||
              key.startsWith("spellwake.social-draft.v1:"),
          ).length,
      ),
    )
    .toBeGreaterThanOrEqual(4);
  await page.screenshot({
    path: join(config.stage, "browser-owned-social.png"),
    fullPage: true,
  });
  await page
    .getByRole("button", { name: "Close messages", exact: true })
    .click();
  await checkpoint("social-result", {
    passed: true,
    sendPosts: socialSends.length,
    draftRetained: true,
  });
  await waitFor("library-ready");
  await page.getByRole("button", { name: "Create deck", exact: true }).click();
  await page
    .getByLabel("Deck name", { exact: true })
    .fill("Browser owned deck");
  await page.getByRole("combobox", { name: "Deck format" }).click();
  await page.getByRole("option", { name: "Standard", exact: true }).click();
  await page
    .getByRole("dialog")
    .getByRole("button", { name: "Create deck", exact: true })
    .click();
  await expect(
    page.getByRole("heading", { name: "Browser owned deck", exact: true }),
  ).toBeVisible();
  const deckId = z.uuid().parse(new URL(page.url()).pathname.split("/").at(-1));
  const printingLabel = `${config.catalogueCard.setName} · ${config.catalogueCard.collectorNumber} · ${config.catalogueCard.finish} · ${config.catalogueCard.language}`;
  async function selectSourcePrinting(): Promise<void> {
    await page
      .getByLabel("Card name", { exact: true })
      .fill(config.catalogueCard.name);
    await page
      .getByRole("button", { name: config.catalogueCard.name, exact: true })
      .click();
    for (let count = 0; count < 20; count++) {
      const option = page.getByRole("button", {
        name: printingLabel,
        exact: true,
      });
      if (await option.isVisible()) {
        await option.click();
        return;
      }
      const more = page.getByRole("button", {
        name: "More printings",
        exact: true,
      });
      if (await more.isVisible()) {
        await more.click();
        continue;
      }
      await expect(option).toBeVisible({ timeout: 15000 });
      await option.click();
      return;
    }
    throw new Error(
      "Owned source printing was not reachable through its catalogue pages.",
    );
  }
  await page.getByRole("button", { name: "Add card", exact: true }).click();
  await selectSourcePrinting();
  await page.getByLabel("Quantity", { exact: true }).fill("2");
  await page
    .getByRole("dialog")
    .getByRole("button", { name: "Add card", exact: true })
    .click();
  await page.getByRole("button", { name: "Save deck", exact: true }).click();
  await expect(
    page.getByRole("button", { name: "Recover save", exact: true }),
  ).toBeVisible();
  await page.screenshot({
    path: join(config.stage, "browser-deck-unresolved.png"),
    fullPage: true,
  });
  await page.reload({ waitUntil: "domcontentloaded" });
  await page.getByRole("button", { name: "Recover save", exact: true }).click();
  await expect(
    page.getByText("Save confirmed.", { exact: true }),
  ).toBeVisible();
  expect(saveBodies).toHaveLength(1);
  await page.getByRole("button", { name: "Import list", exact: true }).click();
  await page
    .getByLabel("Deck list", { exact: true })
    .fill(
      `1 ${config.catalogueCard.name} (${config.catalogueCard.setCode}) ${config.catalogueCard.collectorNumber}`,
    );
  await page
    .getByRole("button", { name: "Resolve cards", exact: true })
    .click();
  const correction = page.getByRole("button", {
    name: "Choose card for line 1",
    exact: true,
  });
  await expect(page.getByText(/of 1 lines resolved/)).toBeVisible();
  if (await correction.isVisible()) {
    await correction.click();
    await selectSourcePrinting();
    await page
      .getByRole("dialog")
      .filter({
        has: page.getByRole("heading", { name: "Add card", exact: true }),
      })
      .getByRole("button", { name: "Add card", exact: true })
      .click();
  }
  await page
    .getByRole("button", { name: "Add imported cards", exact: true })
    .click();
  await expect(
    page.getByLabel(`Quantity for ${config.catalogueCard.name}`, {
      exact: true,
    }),
  ).toHaveValue("3");
  await page.getByRole("button", { name: "Save deck", exact: true }).click();
  await expect(page.getByText("Deck saved.", { exact: true })).toBeVisible();
  expect(saveBodies).toHaveLength(2);
  expect(saveBodies[0]?.requestId).not.toBe(saveBodies[1]?.requestId);
  await page.setViewportSize({ width: 390, height: 844 });
  await page.screenshot({
    path: join(config.stage, "browser-deck-saved-compact.png"),
    fullPage: true,
  });
  await checkpoint("deck-saved", {
    deckId,
    saveRequests: saveBodies.map((body) => body.requestId),
  });
  await page.setViewportSize({ width: 1440, height: 960 });
  await page.getByRole("button", { name: "Back", exact: true }).click();
  await expect(
    page.getByRole("button", { name: "Open Browser owned deck", exact: true }),
  ).toBeVisible();
  await page.screenshot({
    path: join(config.stage, "browser-owned-library.png"),
    fullPage: true,
  });
  await page.getByRole("button", { name: "Open account menu" }).click();
  await page.getByRole("menuitem", { name: "Cosmetics", exact: true }).click();
  await expect(
    page.getByRole("heading", {
      name: "Owned qualification back",
      exact: true,
    }),
  ).toBeVisible();
  await page
    .getByRole("button", { name: "View checkout", exact: true })
    .click();
  await checkpoint("reject-ready", {});
  await waitFor("reject-start");
  await page
    .getByRole("button", { name: "Continue to checkout", exact: true })
    .click();
  await expect(page.getByRole("alert")).toContainText(
    "Checkout is unavailable for this cosmetic.",
  );
  await checkpoint("rejected", { requestId: requestIds[0] });
  await waitFor("reactivated");
  await page
    .getByRole("button", { name: "Recover checkout", exact: true })
    .click();
  await expect(page.getByRole("alert")).toContainText(
    "Checkout is still unresolved",
  );
  await page.reload({ waitUntil: "domcontentloaded" });
  await page
    .getByRole("button", { name: "View checkout", exact: true })
    .click();
  await page
    .getByRole("button", { name: "Recover checkout", exact: true })
    .click();
  await expect(
    page.getByRole("link", {
      name: "Review price and pay in Stripe",
      exact: true,
    }),
  ).toBeVisible();
  expect(requestIds.length).toBe(3);
  expect(requestIds[0] === requestIds[1]).toBe(false);
  expect(requestIds[1] === requestIds[2]).toBe(true);
  const purchaseResponse = await page.waitForResponse(
    (response) =>
      new URL(response.url()).pathname.startsWith("/v1/purchases/") &&
      response.request().method() === "GET",
  );
  const purchase = z
    .object({
      status: z.literal("success"),
      data: z.object({ purchaseId: z.uuid(), cosmeticId: z.uuid() }),
    })
    .parse(await purchaseResponse.json());
  expect(purchase.data.cosmeticId === config.cosmetic).toBe(true);
  await checkpoint("purchase-ready", {
    purchaseId: purchase.data.purchaseId,
    sameRequest: true,
  });
  await expect(
    page.getByText("Payment confirmed.", { exact: true }),
  ).toBeVisible({ timeout: 30000 });
  await expect(
    page.getByRole("button", { name: "Available", exact: true }),
  ).toBeDisabled();
  await checkpoint("paid", { purchaseId: purchase.data.purchaseId });
  await waitFor("reverse");
  await expect(
    page.getByText("This payment was reversed.", { exact: true }),
  ).toBeVisible({ timeout: 30000 });
  await expect(
    page.getByRole("button", { name: "View checkout", exact: true }),
  ).toBeEnabled();
  await checkpoint("reversed", { purchaseId: purchase.data.purchaseId });
  await waitFor("restore");
  await expect(
    page.getByText("Payment confirmed.", { exact: true }),
  ).toBeVisible({ timeout: 30000 });
  await expect(
    page.getByRole("button", { name: "Available", exact: true }),
  ).toBeDisabled();
  await page.screenshot({
    path: join(config.stage, "browser-restored-cosmetic.png"),
    fullPage: true,
  });
  await page.setViewportSize({ width: 390, height: 844 });
  expect(
    await page.evaluate(
      () =>
        document.documentElement.scrollWidth <=
        document.documentElement.clientWidth,
    ),
  ).toBe(true);
  await page.screenshot({
    path: join(config.stage, "browser-restored-compact.png"),
    fullPage: true,
  });
  await page.getByRole("button", { name: "Open account menu" }).click();
  await page.getByRole("menuitem", { name: "Sign out", exact: true }).click();
  await expect(
    page.getByRole("heading", { name: "Sign in to Spellwake", exact: true }),
  ).toBeVisible();
  await checkpoint("result", {
    passed: true,
    smtp: true,
    ownerLibrary: true,
    unknownAck: true,
    reversal: true,
    restoration: true,
    signedOut: true,
  });
  // Continue the same owned browser/realm through actual deletion, rather
  // than replace the Auth/API/worker chain with controlled terminal replies.
  await page.getByLabel("Email", { exact: true }).fill(config.email);
  await page
    .getByRole("button", { name: "Continue with email", exact: true })
    .click();
  await expect(page.getByLabel("Six-digit code")).toBeVisible();
  await checkpoint("deletion-code-needed", {});
  await waitFor("deletion-code");
  const login = z
    .strictObject({ code: z.string().regex(/^[0-9]{6}$/) })
    .parse(
      JSON.parse(
        await readFile(
          join(config.stage, "browser-deletion-code.json"),
          "utf8",
        ),
      ),
    );
  await page.getByLabel("Six-digit code").fill(login.code);
  await expect(
    page.getByRole("heading", { name: "Cosmetics", exact: true }),
  ).toBeVisible();
  const deletionRequests: { requestId: string; statusKeyHash: string }[] = [];
  await page.route(config.api + "/v1/account/deletion", async (route) => {
    const input = requestDeletionSchema.parse(route.request().postDataJSON());
    deletionRequests.push(input);
    const response = await route.fetch();
    expect(response.status()).toBe(202);
    z.strictObject({
      status: z.literal("success"),
      data: commandAckSchema,
    }).parse(await response.json());
    await route.abort("failed");
  });
  await page
    .getByRole("button", { name: "Open account menu", exact: true })
    .click();
  await page
    .getByRole("menuitem", { name: "Default settings", exact: true })
    .click();
  await page
    .getByRole("button", { name: "Delete account", exact: true })
    .click();
  await expect(
    page.getByRole("dialog", { name: "Delete your account?" }),
  ).toBeVisible();
  await page
    .getByRole("button", { name: "Delete my account", exact: true })
    .click();
  await expect(page.getByRole("alert")).toContainText(
    "Deletion hasn’t been confirmed",
  );
  await page.reload();
  await page
    .getByRole("button", { name: "Open account menu", exact: true })
    .click();
  await page
    .getByRole("menuitem", { name: "Default settings", exact: true })
    .click();
  await expect(
    page.getByText(
      "Waiting for committed matches, predictions and payments to finish.",
      { exact: true },
    ),
  ).toBeVisible({ timeout: 30000 });
  expect(deletionRequests.length).toBe(1);
  await checkpoint("deletion-ready", {
    request: deletionRequests[0],
    unknownAck: true,
    reloaded: true,
  });
  await waitFor("deletion-worker-settled", 180000);
  await page.getByRole("button", { name: "Check again", exact: true }).click();
  // The root must verify actual observer proof and finish local payload/Auth
  // erasure before completion. This is not a browser-supplied terminal flag.
  await expect(
    page.getByRole("dialog", { name: "Account deleted", exact: true }),
  ).toBeVisible({ timeout: 30000 });
  const erasure = await page.evaluate(
    ({ api, auth }) => ({
      remainingPayloads: Object.keys(localStorage).filter(
        (key) =>
          key.startsWith(`spellwake.deck-command.v1:${api}:`) ||
          key.startsWith(`spellwake.profile-defaults.v1:${api}:`) ||
          key.startsWith(`spellwake.checkout.v1:${api}:`) ||
          key.startsWith(`spellwake.deletion.v1:${api}:`) ||
          key.startsWith(`spellwake.social-command.v1:${api}:`) ||
          key.startsWith(`spellwake.social-draft.v1:${api}:`),
      ).length,
      sdkSession:
        localStorage.getItem(`spellwake.auth.v1:${new URL(auth).host}`) !==
        null,
      closures: Object.keys(localStorage).filter((key) =>
        key.startsWith(`spellwake.account-closed.v1:${api}:`),
      ).length,
    }),
    { api: config.api, auth: config.auth },
  );
  expect(erasure).toEqual({
    remainingPayloads: 0,
    sdkSession: false,
    closures: 1,
  });
  await page.screenshot({
    path: join(config.stage, "browser-deletion-complete.png"),
    fullPage: true,
  });
  await page.getByRole("button", { name: "Close", exact: true }).click();
  await expect(
    page.getByRole("heading", { name: "Sign in to Spellwake", exact: true }),
  ).toBeVisible();
  await checkpoint("deletion-result", {
    passed: true,
    ...erasure,
    requests: deletionRequests.length,
  });
  await context.close();
} finally {
  await browser.close();
}

import { expect, test } from "bun:test";
import {
  accountSessionSchema,
  inventoryItemSchema,
  purchaseSchema,
  hostedCheckoutUrlSchema,
  usernameSchema,
  createDeckSchema,
  saveDeckSchema,
  changeDeckSettingsSchema,
  changeProfileDefaultsSchema,
  accountSummarySchema,
  requestDeletionSchema,
  observeDeletionSchema,
  deletionObservationSchema,
  socialCommandSchema,
  socialCommandDispositionSchema,
} from "@spellwake/application-contracts";
import { ApplicationApi, publicOrigin } from "./application-api";
import { createApplicationServices } from "../auth/services";
import { openNativeCheckout } from "../lib/checkout";

const id = "7b0ab1bc-8bf0-4159-b951-4d15ca766c1d";
const other = "8b0ab1bc-8bf0-4159-b951-4d15ca766c1d";
test("social protocol preserves Unicode scalar bounds and denies caller identity", () => {
  const input = {
    kind: "send_dm",
    command: { requestId: id, conversationId: other, body: "🎲".repeat(4000) },
  };
  expect(socialCommandSchema.safeParse(input).success).toBe(true);
  expect(
    socialCommandSchema.safeParse({
      ...input,
      command: { ...input.command, body: "🎲".repeat(4001) },
    }).success,
  ).toBe(false);
  for (const body of ["", " \n\t", "hidden\0text"]) {
    expect(
      socialCommandSchema.safeParse({
        ...input,
        command: { ...input.command, body },
      }).success,
    ).toBe(false);
  }
  expect(socialCommandSchema.safeParse({ ...input, actorId: id }).success).toBe(
    false,
  );
  expect(
    socialCommandSchema.safeParse({
      ...input,
      command: { ...input.command, actorId: id },
    }).success,
  ).toBe(false);
  expect(
    socialCommandDispositionSchema.safeParse({
      state: "rejected",
      response: { status: "fail", data: { internal: "private details" } },
    }).success,
  ).toBe(false);
});

test("social transport sends exact explicit intent and rejects a foreign block acknowledgment", async () => {
  const input = socialCommandSchema.parse({
    kind: "block",
    command: { requestId: id, otherAccountId: other, blocked: true },
  });
  let calls = 0;
  const api = ApplicationApi.create(
    "https://api.example",
    async (url, init) => {
      calls++;
      expect(new Headers(init?.headers).get("Authorization")).toBe(
        "Bearer original-session",
      );
      expect(init?.credentials).toBe("omit");
      expect(init?.cache).toBe("no-store");
      expect(JSON.parse(String(init?.body))).toEqual(input);
      const lookup = String(url).endsWith("/disposition");
      expect(String(url)).toBe(
        `https://api.example/v1/social/commands${lookup ? "/disposition" : ""}`,
      );
      const acknowledgment = { resourceId: calls <= 2 ? other : id };
      return Response.json({
        status: "success",
        data: lookup
          ? {
              state: "accepted",
              response: { status: "success", data: acknowledgment },
            }
          : acknowledgment,
      });
    },
  );
  expect(api).not.toBeNull();
  if (!api) throw new Error("Expected configured test API.");
  expect((await api.socialCommand("original-session", input)).status).toBe(
    "success",
  );
  expect(
    (await api.socialCommandDisposition("original-session", input)).status,
  ).toBe("success");
  expect((await api.socialCommand("original-session", input)).status).toBe(
    "error",
  );
  expect(
    (await api.socialCommandDisposition("original-session", input)).status,
  ).toBe("error");
  const injected = Object.assign({}, input, { actorId: id });
  expect((await api.socialCommand("original-session", injected)).status).toBe(
    "fail",
  );
  expect(calls).toBe(4);
});

test("private deletion observation survives logout without sending a bearer or URL secret", async () => {
  const input = { accountId: id, requestId: other, statusKey: "A".repeat(43) };
  const api = ApplicationApi.create(
    "https://api.example",
    async (url, init) => {
      expect(String(url)).toBe(
        "https://api.example/v1/account/deletion/observe",
      );
      expect(init?.method).toBe("POST");
      expect(new Headers(init?.headers).has("Authorization")).toBe(false);
      expect(init?.credentials).toBe("omit");
      expect(init?.cache).toBe("no-store");
      expect(init?.redirect).toBe("error");
      expect(JSON.parse(String(init?.body))).toEqual(input);
      return Response.json({
        status: "success",
        data: { accountId: id, requestId: other, phase: "pending" },
      });
    },
  );
  expect(await api?.observeDeletion(input)).toEqual({
    status: "success",
    data: { accountId: id, requestId: other, phase: "pending" },
  });
  const wrong = ApplicationApi.create("https://api.example", async () =>
    Response.json({
      status: "success",
      data: { accountId: other, requestId: other, phase: "pending" },
    }),
  );
  expect((await wrong?.observeDeletion(input))?.status).toBe("error");
});
test("deletion capabilities reject actor injection, key aliases and unproven completion", () => {
  const command = { requestId: id, statusKeyHash: "a".repeat(64) };
  expect(requestDeletionSchema.safeParse(command).success).toBe(true);
  expect(
    requestDeletionSchema.safeParse({ ...command, accountId: other }).success,
  ).toBe(false);
  expect(
    requestDeletionSchema.safeParse({
      ...command,
      statusKeyHash: "A".repeat(64),
    }).success,
  ).toBe(false);
  const observer = {
    accountId: id,
    requestId: other,
    statusKey: "A".repeat(43),
  };
  expect(observeDeletionSchema.safeParse(observer).success).toBe(true);
  for (const statusKey of [
    "A".repeat(42),
    "A".repeat(44),
    `${"A".repeat(42)}B`,
    `${"A".repeat(43)}=`,
  ]) {
    expect(
      observeDeletionSchema.safeParse({ ...observer, statusKey }).success,
    ).toBe(false);
  }
  expect(
    deletionObservationSchema.safeParse({
      accountId: id,
      requestId: other,
      phase: "deleted",
      retirementOperationId: id,
    }).success,
  ).toBe(false);
  expect(
    deletionObservationSchema.safeParse({
      accountId: id,
      requestId: other,
      phase: "pending",
      username: "private",
    }).success,
  ).toBe(false);
});
test("malformed decimal fields fail validation before balance arithmetic", () => {
  for (const invalid of ["1.5", "not-coins", "", "-1"]) {
    expect(
      accountSummarySchema.shape.wallet.safeParse({
        owned: invalid,
        held: "0",
        available: "0",
      }).success,
    ).toBe(false);
    expect(
      inventoryItemSchema.safeParse({
        id,
        assetId: other,
        revision: "1",
        owned: invalid,
        held: "0",
        available: "0",
        asset: { kind: "coin", currencyKey: "coin" },
      }).success,
    ).toBe(false);
  }
});
test("owned deck contracts keep canonical identities, exact quantities and distinct entries", () => {
  const create = {
    requestId: id,
    name: "Owned",
    mode: "collection",
    formatId: other,
  };
  expect(createDeckSchema.safeParse(create).success).toBe(true);
  expect(createDeckSchema.safeParse({ ...create, actorId: id }).success).toBe(
    false,
  );
  expect(
    createDeckSchema.safeParse({ ...create, formatId: "Standard" }).success,
  ).toBe(false);
  const entry = {
    definitionId: other,
    printingId: null,
    section: "main",
    quantity: "9007199254740993",
  };
  const save = {
    requestId: id,
    deckId: other,
    expectedRevisionId: null,
    entries: [entry],
  };
  expect(saveDeckSchema.parse(save).entries[0]?.quantity).toBe(
    "9007199254740993",
  );
  for (const quantity of [0, "0", "01", "1.5", "-1"])
    expect(
      saveDeckSchema.safeParse({ ...save, entries: [{ ...entry, quantity }] })
        .success,
    ).toBe(false);
  expect(
    saveDeckSchema.safeParse({ ...save, entries: [entry, entry] }).success,
  ).toBe(false);
  expect(
    saveDeckSchema.safeParse({
      ...save,
      entries: [{ ...entry, printingId: "Lightning Bolt" }],
    }).success,
  ).toBe(false);
});

test("deck transport preserves original bearer and rejects mismatched or unsealed detail", async () => {
  const seen: { path: string; bearer: string | null; body: unknown }[] = [];
  const api = ApplicationApi.create(
    "https://api.example",
    async (input, init) => {
      seen.push({
        path: String(input),
        bearer: new Headers(init?.headers).get("authorization"),
        body: init?.body ? JSON.parse(String(init.body)) : null,
      });
      return Response.json({ status: "success", data: { resourceId: other } });
    },
  )!;
  const request = createDeckSchema.parse({
    requestId: id,
    name: "Owned",
    mode: "collection",
    formatId: other,
  });
  expect((await api.createDeck("original-owner", request)).status).toBe(
    "success",
  );
  expect(seen[0]).toEqual({
    path: "https://api.example/v1/decks",
    bearer: "Bearer original-owner",
    body: request,
  });
  const appearance = {
    backId: id,
    sleeveId: other,
    diceId: id,
    playmatId: null,
  };
  const detail = {
    deck: {
      id,
      name: "Owned",
      mode: "collection",
      currentRevisionId: other,
      format: { id: other, key: "standard", name: "Standard" },
      appearance,
    },
    revision: null,
  };
  let returned: unknown = detail;
  const invalid = ApplicationApi.create("https://api.example", async () =>
    Response.json({ status: "success", data: returned }),
  )!;
  expect((await invalid.deck("original-owner", id)).status).toBe("error");
  returned = { ...detail, deck: { ...detail.deck, currentRevisionId: null } };
  expect((await invalid.deck("original-owner", other)).status).toBe("error");
});
test("hosted checkout navigation uses the exact provider origin without credentials", async () => {
  expect(
    hostedCheckoutUrlSchema.safeParse(
      "https://checkout.stripe.com/c/pay/synthetic",
    ).success,
  ).toBe(true);
  for (const url of [
    "https://checkout.stripe.com:444/c/pay/synthetic",
    "https://checkout.stripe.com.evil.example/c/pay/synthetic",
    "https://user@checkout.stripe.com/c/pay/synthetic",
    "http://checkout.stripe.com/c/pay/synthetic",
    "file:///tmp/checkout",
  ])
    expect(hostedCheckoutUrlSchema.safeParse(url).success).toBe(false);
  expect(
    await openNativeCheckout(
      "https://checkout.stripe.com/c/pay/synthetic",
      "2020-01-01T00:00:00Z",
    ),
  ).toBe(false);
  expect(
    await openNativeCheckout(
      "https://checkout.stripe.com/c/pay/synthetic",
      "not-a-date",
    ),
  ).toBe(false);
});
test("public origins and client configuration cannot select a private-key or remote plaintext backend", () => {
  for (const value of [
    "http://remote.example",
    "https://u:p@example.com",
    "https://example.com/path",
    "https://example.com?x=1",
    "javascript:alert(1)",
  ]) {
    expect(publicOrigin(value)).toBeNull();
    expect(ApplicationApi.create(value)).toBeNull();
  }
  expect(publicOrigin("http://127.0.0.1:1234")).toBe("http://127.0.0.1:1234");
  expect(
    createApplicationServices({
      VITE_SUPABASE_URL: "https://auth.example",
      VITE_SUPABASE_PUBLISHABLE_KEY: "sb_secret_do_not_ship",
      VITE_APPLICATION_API_URL: "https://api.example",
    }).status,
  ).toBe("unconfigured");
});
test("wire contracts reject a claimed active account without identity and preserve exact held quantities", () => {
  expect(
    accountSessionSchema.safeParse({
      subjectId: id,
      sessionId: other,
      method: "email_code",
      state: "active",
      accountId: null,
    }).success,
  ).toBe(false);
  const item = {
    id,
    assetId: other,
    revision: "2",
    owned: "9007199254740993",
    held: "1",
    available: "9007199254740992",
    asset: { kind: "coin", currencyKey: "coin" },
  };
  expect(inventoryItemSchema.safeParse(item).success).toBe(true);
  expect(
    inventoryItemSchema.safeParse({ ...item, available: "9007199254740993" })
      .success,
  ).toBe(false);
  expect(
    inventoryItemSchema.safeParse({ ...item, owned: 9007199254740992 }).success,
  ).toBe(false);
  expect(usernameSchema.safeParse("someone_1").success).toBe(true);
  expect(usernameSchema.safeParse("someone@example.com").success).toBe(false);
});
test("original bearer stays in headers, reads refuse redirects and server bodies are schema validated", async () => {
  const calls: RequestInit[] = [];
  const api = ApplicationApi.create(
    "https://api.example",
    async (url, init) => {
      expect(String(url)).toBe("https://api.example/v1/session");
      calls.push(init ?? {});
      return Response.json({
        status: "success",
        data: {
          subjectId: id,
          sessionId: other,
          method: "email_code",
          state: "needs_registration",
          accountId: null,
        },
      });
    },
  );
  expect((await api?.session("synthetic-bearer"))?.status).toBe("success");
  const call = calls[0];
  expect(new Headers(call?.headers).get("Authorization")).toBe(
    "Bearer synthetic-bearer",
  );
  expect(call?.redirect).toBe("error");
  expect(call?.credentials).toBe("omit");
  const malformed = ApplicationApi.create("https://api.example", async () =>
    Response.json({
      status: "success",
      data: { state: "active", PRIVATE_CANARY: "do not copy" },
    }),
  );
  const result = await malformed?.session("synthetic-bearer");
  expect(result?.status).toBe("error");
  expect(JSON.stringify(result)).not.toContain("PRIVATE_CANARY");
});
test("wrong purchase identity or non-Stripe navigation is never accepted", async () => {
  const purchase = {
    purchaseId: other,
    cosmeticId: id,
    createdAt: "2026-11-01T12:00:00Z",
    state: "pending",
    revision: "2",
    price: { currency: "usd", minorUnits: "500" },
    checkout: {
      status: "ready",
      url: "https://checkout.stripe.com/c/pay/synthetic",
      expiresAt: "2026-11-01T13:00:00Z",
    },
  };
  expect(
    purchaseSchema.safeParse({
      ...purchase,
      checkout: {
        ...purchase.checkout,
        url: "https://checkout.stripe.com.evil.example/",
      },
    }).success,
  ).toBe(false);
  const api = ApplicationApi.create("https://api.example", async () =>
    Response.json({ status: "success", data: purchase }),
  );
  expect((await api?.purchase("synthetic-bearer", id))?.status).toBe("error");
});
test("an unsolicited pagination cursor and an oversized body cannot become owner data", async () => {
  const cursor = ApplicationApi.create("https://api.example", async () =>
    Response.json({ status: "success", data: { items: [], nextCursor: id } }),
  );
  expect((await cursor?.inventory("synthetic-bearer"))?.status).toBe("error");
  const oversized = ApplicationApi.create(
    "https://api.example",
    async () => new Response(" ".repeat(1024 * 1024 + 1)),
  );
  expect((await oversized?.session("synthetic-bearer"))?.status).toBe("error");
});

test("inventory continuations use the server's after parameter and summaries keep held coins exact", async () => {
  const api = ApplicationApi.create("https://api.example", async (url) => {
    expect(String(url)).toBe(
      `https://api.example/v1/inventory?limit=100&after=${id}`,
    );
    return Response.json({
      status: "success",
      data: { items: [], nextCursor: null },
    });
  });
  expect((await api?.inventory("synthetic-bearer", id))?.status).toBe(
    "success",
  );
  const summary = ApplicationApi.create("https://api.example", async () =>
    Response.json({
      status: "success",
      data: {
        accountId: id,
        username: "owner_1",
        state: "active",
        appearanceRevision: "0",
        defaults: { backId: id, sleeveId: other, diceId: id, playmatId: null },
        wallet: {
          owned: "9007199254740993",
          held: "1",
          available: "9007199254740992",
        },
      },
    }),
  );
  const result = await summary?.account("synthetic-bearer");
  expect(result?.status).toBe("success");
  if (result?.status === "success")
    expect(result.data.wallet.available).toBe("9007199254740992");
});

test("settings transport binds both snapshots and refuses another deck acknowledgement", async () => {
  const expected = {
    name: "Before",
    mode: "collection",
    formatId: other,
    appearance: { backId: id, sleeveId: other, diceId: id, playmatId: null },
  };
  const input = changeDeckSettingsSchema.parse({
    requestId: other,
    deckId: id,
    expected,
    settings: { ...expected, name: "After" },
  });
  let target = other;
  let isDisposition = false;
  const api = ApplicationApi.create(
    "https://api.example",
    async (url, init) => {
      expect(String(url)).toBe(
        isDisposition
          ? "https://api.example/v1/decks/commands/disposition"
          : `https://api.example/v1/decks/${id}/settings`,
      );
      expect(new Headers(init?.headers).get("authorization")).toBe(
        "Bearer owner",
      );
      expect(JSON.parse(String(init?.body))).toEqual(
        isDisposition ? { kind: "settings", command: input } : input,
      );
      const accepted = { status: "success", data: { resourceId: target } };
      return Response.json(
        isDisposition
          ? {
              status: "success",
              data: { state: "accepted", response: accepted },
            }
          : accepted,
      );
    },
  )!;
  expect((await api.changeDeckSettings("owner", input)).status).toBe("error");
  target = id;
  expect((await api.changeDeckSettings("owner", input)).status).toBe("success");
  isDisposition = true;
  target = other;
  expect(
    (
      await api.deckCommandDisposition("owner", {
        kind: "settings",
        command: input,
      })
    ).status,
  ).toBe("error");
  target = id;
  expect(
    (
      await api.deckCommandDisposition("owner", {
        kind: "settings",
        command: input,
      })
    ).status,
  ).toBe("success");
  const missing = structuredClone(input);
  const malformed = { ...missing.settings.appearance };
  expect(
    changeDeckSettingsSchema.safeParse({
      ...input,
      settings: {
        ...input.settings,
        appearance: {
          backId: malformed.backId,
          sleeveId: malformed.sleeveId,
          diceId: malformed.diceId,
        },
      },
    }).success,
  ).toBe(false);
});

test("profile defaults use their own actorless route and exact bound disposition", async () => {
  const values = { backId: id, sleeveId: other, diceId: id, playmatId: null };
  const command = changeProfileDefaultsSchema.parse({
    requestId: other,
    expected: values,
    appearance: { ...values, diceId: other },
  });
  const seen: string[] = [];
  const api = ApplicationApi.create(
    "https://api.example",
    async (url, init) => {
      seen.push(String(url));
      expect(new Headers(init?.headers).get("authorization")).toBe(
        "Bearer owner",
      );
      expect(JSON.parse(String(init?.body))).toEqual(command);
      return Response.json(
        String(url).endsWith("/disposition")
          ? {
              status: "success",
              data: {
                state: "accepted",
                response: { status: "success", data: { resourceId: id } },
              },
            }
          : { status: "success", data: { resourceId: id } },
      );
    },
  )!;
  expect((await api.changeProfileDefaults("owner", command)).status).toBe(
    "success",
  );
  expect((await api.profileDefaultsDisposition("owner", command)).status).toBe(
    "success",
  );
  expect(seen).toEqual([
    "https://api.example/v1/account/defaults",
    "https://api.example/v1/account/defaults/disposition",
  ]);
  expect(
    changeProfileDefaultsSchema.safeParse({ ...command, accountId: id })
      .success,
  ).toBe(false);
  expect(
    changeProfileDefaultsSchema.safeParse({
      requestId: other,
      appearance: values,
    }).success,
  ).toBe(false);
});

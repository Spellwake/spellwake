import { expect, test } from "bun:test";
import { ApplicationApi } from "./application-api";

const REQUEST_ID = "00000000-0000-4000-8000-000000000081";
const COSMETIC_ID = "00000000-0000-4000-8000-000000000056";
const PURCHASE_ID = "00000000-0000-4000-8000-000000000090";

test("createBillingPortalSession sends returnUrl and validates Stripe portal URL", async () => {
  const api = ApplicationApi.create("https://api.example", async (url, init) => {
    expect(String(url)).toBe("https://api.example/v1/billing/portal");
    expect(init?.method).toBe("POST");
    expect(JSON.parse(String(init?.body))).toEqual({
      returnUrl: "https://app.spellwake.example/play/collection/decks?account=billing",
    });
    return Response.json({
      status: "success",
      data: { url: "https://billing.stripe.com/p/session/test_123" },
    });
  });
  expect(
    await api?.createBillingPortalSession(
      "bearer-token",
      "https://app.spellwake.example/play/collection/decks?account=billing",
    ),
  ).toEqual({
    status: "success",
    data: { url: "https://billing.stripe.com/p/session/test_123" },
  });
});

test("createBillingPortalSession preserves no_customer fail and rejects non-URL responses", async () => {
  const failApi = ApplicationApi.create("https://api.example", async () =>
    Response.json(
      { status: "fail", data: { billing: "no_customer" } },
      { status: 404 },
    ),
  );
  expect(
    await failApi?.createBillingPortalSession(
      "bearer-token",
      "https://app.spellwake.example/play",
    ),
  ).toEqual({ status: "fail", data: { billing: "no_customer" } });

  const badUrlApi = ApplicationApi.create("https://api.example", async () =>
    Response.json({ status: "success", data: { url: "not-a-valid-url" } }),
  );
  const bad = await badUrlApi?.createBillingPortalSession(
    "bearer-token",
    "https://app.spellwake.example/play",
  );
  expect(bad?.status).toBe("error");
});

test("checkout and purchase enforce Stripe hosted checkout URL and purchase schemas", async () => {
  const api = ApplicationApi.create("https://api.example", async (url, init) => {
    const path = String(url);
    if (path.endsWith("/v1/purchases")) {
      expect(init?.method).toBe("POST");
      expect(JSON.parse(String(init?.body))).toEqual({
        requestId: REQUEST_ID,
        cosmeticId: COSMETIC_ID,
      });
      return Response.json(
        { status: "success", data: { resourceId: PURCHASE_ID } },
        { status: 202 },
      );
    }
    return Response.json({
      status: "success",
      data: {
        purchaseId: PURCHASE_ID,
        cosmeticId: COSMETIC_ID,
        createdAt: "2026-11-27T12:00:00Z",
        state: "pending",
        revision: "1",
        price: { currency: "usd", minorUnits: "299" },
        checkout: {
          status: "ready",
          expiresAt: "2030-01-01T00:00:00Z",
          url: "https://checkout.stripe.com/c/pay/cs_test_123",
        },
      },
    });
  });

  expect(await api?.checkout("bearer-token", REQUEST_ID, COSMETIC_ID)).toEqual({
    status: "success",
    data: { resourceId: PURCHASE_ID },
  });
  const read = await api?.purchase("bearer-token", PURCHASE_ID);
  expect(read?.status).toBe("success");
});

test("purchase rejects tampered non-Stripe hosted checkout URLs", async () => {
  const api = ApplicationApi.create("https://api.example", async () =>
    Response.json({
      status: "success",
      data: {
        purchaseId: PURCHASE_ID,
        cosmeticId: COSMETIC_ID,
        createdAt: "2026-11-27T12:00:00Z",
        state: "pending",
        revision: "1",
        price: { currency: "usd", minorUnits: "299" },
        checkout: {
          status: "ready",
          expiresAt: "2030-01-01T00:00:00Z",
          url: "https://evil.example.com/fake-checkout",
        },
      },
    }),
  );
  const read = await api?.purchase("bearer-token", PURCHASE_ID);
  expect(read?.status).toBe("error");
});

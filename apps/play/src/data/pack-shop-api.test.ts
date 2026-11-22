import { expect, test } from "bun:test";
import { z } from "zod";
import {
  packProductPageSchema,
  ownedPackQuoteSchema,
} from "@spellwake/application-contracts";
import { ApplicationApi } from "./application-api";
const f = z
  .strictObject({
    products: packProductPageSchema,
    purchase: ownedPackQuoteSchema,
    valuation: ownedPackQuoteSchema,
  })
  .parse(
    await Bun.file(
      new URL(
        "../../../../services/application/tests/fixtures/pack-shop.json",
        import.meta.url,
      ),
    ).json(),
  );
const account = f.products.accountId;
const recipe = f.purchase.quote.recipeId;

test("pack shop client preserves exact identity, whole quantities, sequential total and exhausted valuation", async () => {
  const calls: string[] = [];
  const api = ApplicationApi.create(
    "https://api.example",
    async (url, init) => {
      calls.push(String(url));
      expect(new Headers(init?.headers).get("Authorization")).toBe(
        "Bearer caller",
      );
      const data = String(url).endsWith("products?limit=32")
        ? f.products
        : String(url).endsWith("quantity=11")
          ? f.valuation
          : f.purchase;
      return Response.json({ status: "success", data });
    },
  );
  expect((await api?.packProducts("caller", account))?.status).toBe("success");
  expect((await api?.packQuote("caller", account, recipe, "2"))?.status).toBe(
    "success",
  );
  expect((await api?.packQuote("caller", account, recipe, "11"))?.status).toBe(
    "success",
  );
  expect(calls).toEqual([
    "https://api.example/v1/packs/products?limit=32",
    `https://api.example/v1/packs/products/${recipe}/quote?quantity=2`,
    `https://api.example/v1/packs/products/${recipe}/quote?quantity=11`,
  ]);
  expect(
    (await api?.packQuote("caller", account, recipe, "1001"))?.status,
  ).toBe("fail");
  expect(calls).toHaveLength(3);
});
test("pack shop rejects foreign owner, wrong quantity, missing nullable total and private evidence", async () => {
  for (const data of [
    { ...f.purchase, accountId: recipe },
    { ...f.purchase, quote: { ...f.purchase.quote, quantity: "1" } },
    { ...f.purchase, quote: { ...f.purchase.quote, evidenceRef: "private" } },
  ]) {
    const api = ApplicationApi.create("https://api.example", async () =>
      Response.json({ status: "success", data }),
    );
    expect((await api?.packQuote("caller", account, recipe, "2"))?.status).toBe(
      "error",
    );
  }
  const { total, ...missing } = f.valuation.quote;
  expect(total).toBeNull();
  expect(
    ownedPackQuoteSchema.safeParse({ ...f.valuation, quote: missing }).success,
  ).toBe(false);
  expect(
    ownedPackQuoteSchema.safeParse({
      ...f.valuation,
      quote: { ...f.valuation.quote, purpose: "purchase" },
    }).success,
  ).toBe(false);
  expect(
    ownedPackQuoteSchema.safeParse({
      ...f.purchase,
      quote: { ...f.purchase.quote, remaining: "not-a-number" },
    }).success,
  ).toBe(false);
  const api = ApplicationApi.create("https://api.example", async () =>
    Response.json({
      status: "success",
      data: {
        ...f.products,
        items: [...f.products.items, ...f.products.items],
      },
    }),
  );
  expect((await api?.packProducts("caller", account))?.status).toBe("error");
});

import { afterEach, describe, expect, test } from "bun:test";
import {
  acquireCustomArtUrl,
  clearCustomArtCache,
  customArtCacheSize,
  releaseCustomArtUrl,
} from "../data/custom-art-cache";

describe("custom-art-cache ref-counted blob lifecycle", () => {
  const originalFetch = globalThis.fetch;
  const originalCreateObjectURL = URL.createObjectURL;
  const originalRevokeObjectURL = URL.revokeObjectURL;

  afterEach(() => {
    clearCustomArtCache();
    globalThis.fetch = originalFetch;
    URL.createObjectURL = originalCreateObjectURL;
    URL.revokeObjectURL = originalRevokeObjectURL;
  });

  test("deduplicates in-flight fetches and revokes object URL only when ref count hits zero", async () => {
    let fetchCalls = 0;
    const revoked: string[] = [];
    let counter = 0;

    globalThis.fetch = (async () => {
      fetchCalls += 1;
      return new Response(new Uint8Array([137, 80, 78, 71]), {
        status: 200,
        headers: { "Content-Type": "image/png" },
      });
    }) as unknown as typeof fetch;

    URL.createObjectURL = () => {
      counter += 1;
      return `blob:spellwake-test-${counter}`;
    };
    URL.revokeObjectURL = (url: string) => {
      revoked.push(url);
    };

    const path = "/v1/custom-cards/revisions/11111111-1111-4111-8111-111111111111/art/0";
    const [u1, u2] = await Promise.all([
      acquireCustomArtUrl("https://api.example.test", path, "tok-1"),
      acquireCustomArtUrl("https://api.example.test", path, "tok-1"),
    ]);

    expect(fetchCalls).toBe(1);
    expect(u1).toBe("blob:spellwake-test-1");
    expect(u2).toBe("blob:spellwake-test-1");
    expect(customArtCacheSize()).toBe(1);

    releaseCustomArtUrl("https://api.example.test", path);
    expect(revoked).toEqual([]);
    expect(customArtCacheSize()).toBe(1);

    releaseCustomArtUrl("https://api.example.test", path);
    expect(revoked).toEqual(["blob:spellwake-test-1"]);
    expect(customArtCacheSize()).toBe(0);
  });

  test("passes through data: and blob: URLs without fetching", async () => {
    let fetchCalls = 0;
    globalThis.fetch = (async () => {
      fetchCalls += 1;
      return new Response(null, { status: 500 });
    }) as unknown as typeof fetch;

    const dataUrl = "data:image/png;base64,iVBORw0KGgo=";
    const resolved = await acquireCustomArtUrl("https://api.example.test", dataUrl, "tok-1");
    expect(resolved).toBe(dataUrl);
    expect(fetchCalls).toBe(0);
  });
});

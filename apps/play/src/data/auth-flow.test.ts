import { expect, test } from "bun:test";
import {
  parseAuthReturn,
  NATIVE_AUTH_CALLBACK,
  browserAuthCallback,
} from "../auth/oauth";
import { createClient } from "@supabase/supabase-js";
import { parsePublicConfiguration } from "../lib/public-config";
import {
  nativeDevOrigin,
  nativeSecurityOverlay,
  withNativeOverlay,
} from "../../scripts/native-config";

const publicValues = {
  VITE_SUPABASE_URL: "https://auth.example",
  VITE_APPLICATION_API_URL: "https://api.example",
  VITE_SUPABASE_PUBLISHABLE_KEY: "sb_publishable_synthetic_only",
};
test("one public config supplies exact CSP origins without keys or wildcard development access", () => {
  expect(parsePublicConfiguration({}).status).toBe("unconfigured");
  expect(
    parsePublicConfiguration({ VITE_SUPABASE_URL: "https://auth.example" })
      .status,
  ).toBe("invalid");
  expect(
    parsePublicConfiguration({
      ...publicValues,
      VITE_SUPABASE_URL: "https://auth.example;script-src",
    }).status,
  ).toBe("invalid");
  expect(
    parsePublicConfiguration({
      ...publicValues,
      VITE_SUPABASE_URL: "https://*.example.com",
    }).status,
  ).toBe("invalid");
  const selected = parsePublicConfiguration(publicValues);
  expect(selected.status).toBe("ready");
  if (selected.status !== "ready") return;
  const overlay = nativeSecurityOverlay(
    "default-src 'self'; connect-src 'self' blob: ipc: http://ipc.localhost",
    selected.config,
    nativeDevOrigin("192.168.1.4"),
  );
  const text = JSON.stringify(overlay);
  expect(text).not.toContain(publicValues.VITE_SUPABASE_PUBLISHABLE_KEY);
  expect(overlay.app.security.csp).toContain(
    "https://auth.example https://api.example",
  );
  expect(overlay.app.security.devCsp).toContain(
    "http://192.168.1.4:6204 ws://192.168.1.4:6204",
  );
  expect(overlay.app.security.devCsp).not.toContain("http: ws:");
  expect(
    withNativeOverlay(
      ["build", "--config", "release.json", "--", "--locked"],
      overlay,
    ).slice(-2),
  ).toEqual(["--", "--locked"]);
  expect(withNativeOverlay(["android", "build"], overlay).slice(0, 3)).toEqual([
    "android",
    "build",
    "--config",
  ]);
});
test("callbacks require exact target, one PKCE flow and code, and bounded safe return destinations", () => {
  const flow = "a".repeat(32);
  const code = "synthetic_code";
  const callback =
    NATIVE_AUTH_CALLBACK + `?code=${code}&sb_flow_id=${flow}&next=%2Fcosmetics`;
  expect(parseAuthReturn(callback, NATIVE_AUTH_CALLBACK)).toEqual({
    code,
    flowId: flow,
    next: "/cosmetics",
  });
  for (const url of [
    callback.replace("://auth/", "://evil/"),
    callback + "&code=other",
    callback.replace(flow, "invalid"),
    callback + "#access_token=synthetic",
    NATIVE_AUTH_CALLBACK + "?code=unsolicited",
  ])
    expect(parseAuthReturn(url, NATIVE_AUTH_CALLBACK)).toBeNull();
  expect(
    parseAuthReturn(
      callback.replace("%2Fcosmetics", "https://evil.example"),
      NATIVE_AUTH_CALLBACK,
    )?.next,
  ).toBe("/collection");
  expect(browserAuthCallback("https://app.example", "/play/")).toBe(
    "https://app.example/play/auth/callback",
  );
});
test("the pinned SDK creates independent verifier slots and redirects with in-memory protocol storage", async () => {
  // This protocol-only test uses SDK memory. Production persistence requires
  // real Web Locks and is exercised by the controlled browser qualification.
  const client = createClient(
    publicValues.VITE_SUPABASE_URL,
    publicValues.VITE_SUPABASE_PUBLISHABLE_KEY,
    {
      auth: {
        flowType: "pkce",
        persistSession: false,
        autoRefreshToken: false,
        experimental: { appendPkceFlowIdToRedirects: true },
      },
    },
  );
  const first = await client.auth.signInWithOAuth({
    provider: "google",
    options: { redirectTo: NATIVE_AUTH_CALLBACK, skipBrowserRedirect: true },
  });
  const second = await client.auth.signInWithOAuth({
    provider: "discord",
    options: { redirectTo: NATIVE_AUTH_CALLBACK, skipBrowserRedirect: true },
  });
  expect(first.error).toBeNull();
  expect(second.error).toBeNull();
  const firstUrl = new URL(first.data.url!);
  const secondUrl = new URL(second.data.url!);
  const firstReturn = new URL(firstUrl.searchParams.get("redirect_to")!);
  const secondReturn = new URL(secondUrl.searchParams.get("redirect_to")!);
  expect(firstReturn.searchParams.get("sb_flow_id")).toMatch(/^[a-f0-9]{32}$/);
  expect(firstReturn.searchParams.get("sb_flow_id")).not.toBe(
    secondReturn.searchParams.get("sb_flow_id"),
  );
  expect(firstUrl.searchParams.get("code_challenge_method")).toBe("s256");
});

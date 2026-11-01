import { describe, expect, test } from "bun:test";
import { releaseErrors } from "./release-preflight";
import { assetPublicationErrors } from "./release-asset-source";

function preflight(
  values: Record<string, string>,
  assetScope: "internal-study" | "publication-cleared" = "internal-study",
): {
  code: number;
  log: string;
} {
  const errors = releaseErrors(values, assetScope);
  return { code: errors.length === 0 ? 0 : 1, log: errors.join("\n") };
}

describe("release publication boundary", () => {
  test("candidate builds require no signing or publishing credentials", () => {
    expect(preflight({ SIGNED_RELEASE: "false", PUBLISH: "false" }).code).toBe(
      0,
    );
  });
  test("publishing rejects an unsigned candidate and uncleared assets", () => {
    const result = preflight({
      PUBLISH: "true",
      GITHUB_REF: "refs/heads/main",
    });
    expect(result.code).not.toBe(0);
    expect(result.log).toContain("Publishing requires signed packages");
    expect(result.log).toContain("RELEASE_ASSETS_APPROVED");
    expect(result.log).toContain("RENDER_WEB_DEPLOY_HOOK");
  });
  test("signed builds require main and report missing credentials by name", () => {
    const result = preflight({
      SIGNED_RELEASE: "true",
      GITHUB_REF: "refs/heads/dev",
    });
    expect(result.code).not.toBe(0);
    expect(result.log).toContain("must use main");
    expect(result.log).toContain("IOS_MOBILE_PROVISION");
    expect(result.log).toContain("ANDROID_KEYSTORE_BASE64");
  });
  test("diagnostics never include supplied credential values", () => {
    const result = preflight({
      SIGNED_RELEASE: "true",
      APPLE_CERTIFICATE: "private-test-value",
    });
    expect(result.code).not.toBe(0);
    expect(result.log).not.toContain("private-test-value");
    expect(result.log).not.toContain(
      "Missing release setting: APPLE_CERTIFICATE\n",
    );
  });
  test("a PFX does not bypass the configured Azure signing requirements", () => {
    const result = preflight({
      SIGNED_RELEASE: "true",
      GITHUB_REF: "refs/heads/main",
      WINDOWS_CERTIFICATE: "private-test-pfx",
      WINDOWS_CERTIFICATE_PASSWORD: "private-test-password",
    });
    expect(result.code).not.toBe(0);
    expect(result.log).toContain("WINDOWS_AZURE_CLIENT_ID");
    expect(result.log).toContain("WINDOWS_SIGNING_PROFILE");
    expect(result.log).not.toContain("private-test-pfx");
    expect(result.log).not.toContain("private-test-password");
  });
});

test("publication rejects invalid web destinations without echoing hook credentials", () => {
  const result = preflight({
    PUBLISH: "true",
    SIGNED_RELEASE: "true",
    GITHUB_REF: "refs/heads/main",
    RENDER_WEB_DEPLOY_HOOK: "https://wrong.example/private-hook-token",
    WEB_RELEASE_URL: "http://spellwake.example",
  });
  expect(result.log).toContain("HTTPS Render hook");
  expect(result.log).toContain("must use HTTPS");
  expect(result.log).not.toContain("private-hook-token");
});

test("a configured main release can pass the publication gate", () => {
  const values = Object.fromEntries(
    [
      "APPLE_CERTIFICATE",
      "APPLE_CERTIFICATE_PASSWORD",
      "APPLE_SIGNING_IDENTITY",
      "APPLE_DEVELOPMENT_TEAM",
      "IOS_CERTIFICATE",
      "IOS_CERTIFICATE_PASSWORD",
      "IOS_MOBILE_PROVISION",
      "WINDOWS_AZURE_CLIENT_ID",
      "WINDOWS_AZURE_TENANT_ID",
      "WINDOWS_AZURE_SUBSCRIPTION_ID",
      "WINDOWS_SIGNING_ENDPOINT",
      "WINDOWS_SIGNING_ACCOUNT",
      "WINDOWS_SIGNING_PROFILE",
      "ANDROID_KEYSTORE_BASE64",
      "ANDROID_STORE_PASSWORD",
      "ANDROID_KEY_ALIAS",
      "ANDROID_KEY_PASSWORD",
      "APPLE_API_KEY",
      "APPLE_API_ISSUER",
      "APPLE_API_PRIVATE_KEY",
      "GOOGLE_PLAY_SERVICE_ACCOUNT",
    ].map((name) => [name, "configured-test-value"]),
  );
  const result = preflight(
    {
      ...values,
      SIGNED_RELEASE: "true",
      PUBLISH: "true",
      GITHUB_REF: "refs/heads/main",
      RELEASE_ASSETS_APPROVED: "true",
      RENDER_WEB_DEPLOY_HOOK: "https://api.render.com/deploy/srv-test?key=test",
      WEB_RELEASE_URL: "https://spellwake.example",
    },
    "publication-cleared",
  );
  expect(result).toEqual({ code: 0, log: "" });
});

test("public web and native releases share the immutable asset scope boundary", () => {
  expect(assetPublicationErrors("internal-study", true)).not.toHaveLength(0);
  expect(assetPublicationErrors("publication-cleared", false)).not.toHaveLength(
    0,
  );
  expect(assetPublicationErrors("publication-cleared", true)).toEqual([]);
});

test("web preview approval never authorizes study assets or native/store publication", () => {
  expect(assetPublicationErrors("public-preview", false, true)).toEqual([]);
  expect(assetPublicationErrors("public-preview", false)).not.toHaveLength(0);
  expect(assetPublicationErrors("public-preview", true)).not.toHaveLength(0);
  expect(assetPublicationErrors("internal-study", true, true)).not.toHaveLength(
    0,
  );
  expect(
    releaseErrors(
      {
        PUBLISH: "true",
        RELEASE_PREVIEW_APPROVED: "true",
        GITHUB_REF: "refs/heads/main",
      },
      "public-preview",
    ),
  ).not.toEqual([]);
});

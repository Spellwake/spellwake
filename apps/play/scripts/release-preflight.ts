import {
  assetPublicationErrors,
  type AssetScope,
  releaseAssetPackage,
} from "./release-asset-source";
/** Check distribution prerequisites without printing any credential values. */
export function releaseErrors(
  environment: Record<string, string | undefined>,
  assetScope: AssetScope = "internal-study",
): string[] {
  const signed = environment.SIGNED_RELEASE === "true";
  const publish = environment.PUBLISH === "true";
  const required = signed
    ? [
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
      ]
    : [];
  if (publish) {
    required.push(
      "GOOGLE_PLAY_SERVICE_ACCOUNT",
      "RENDER_WEB_DEPLOY_HOOK",
      "WEB_RELEASE_URL",
    );
  }
  const errors: string[] = [];
  if ((signed || publish) && environment.GITHUB_REF !== "refs/heads/main") {
    errors.push(
      "Signed distribution runs must use main after dev and staging promotion.",
    );
  }
  if (publish && !signed) errors.push("Publishing requires signed packages.");
  if (publish)
    errors.push(
      ...assetPublicationErrors(
        assetScope,
        environment.RELEASE_ASSETS_APPROVED === "true",
      ),
    );
  for (const name of required) {
    if (!environment[name]?.trim())
      errors.push(`Missing release setting: ${name}`);
  }
  if (publish && environment.RENDER_WEB_DEPLOY_HOOK) {
    try {
      const hook = new URL(environment.RENDER_WEB_DEPLOY_HOOK);
      if (
        hook.protocol !== "https:" ||
        hook.hostname !== "api.render.com" ||
        !hook.pathname.startsWith("/deploy/")
      ) {
        errors.push(
          "RENDER_WEB_DEPLOY_HOOK must be the frontend static site's HTTPS Render hook.",
        );
      }
    } catch {
      errors.push("RENDER_WEB_DEPLOY_HOOK is not a valid URL.");
    }
  }
  if (publish && environment.WEB_RELEASE_URL) {
    try {
      if (new URL(environment.WEB_RELEASE_URL).protocol !== "https:") {
        errors.push("WEB_RELEASE_URL must use HTTPS.");
      }
    } catch {
      errors.push("WEB_RELEASE_URL is not a valid URL.");
    }
  }
  return errors;
}

if (import.meta.main) {
  const errors = releaseErrors(
    process.env,
    (await releaseAssetPackage()).scope,
  );
  if (errors.length > 0) {
    console.error(errors.join("\n"));
    process.exit(1);
  }
  console.info("Release prerequisites passed.");
}

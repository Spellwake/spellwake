import {
  assetPublicationErrors,
  releaseAssetPackage,
} from "./release-asset-source";
const hook = process.env.RENDER_WEB_DEPLOY_HOOK;
const site = process.env.WEB_RELEASE_URL;
const commit = process.env.GITHUB_SHA;
const assetPackage = await releaseAssetPackage();
const assetRevision = assetPackage.revision;
const publicationErrors = assetPublicationErrors(
  assetPackage.scope,
  process.env.RELEASE_ASSETS_APPROVED === "true",
  process.env.RELEASE_PREVIEW_APPROVED === "true",
);
if (publicationErrors.length > 0) {
  console.error(publicationErrors.join("\n"));
  process.exit(1);
}
if (!hook || !site || !commit || !assetRevision) {
  console.error("Web deploy hook, site URL and release revision are required.");
  process.exit(1);
}
const url = new URL(hook);
if (
  url.protocol !== "https:" ||
  url.hostname !== "api.render.com" ||
  !url.pathname.startsWith("/deploy/")
) {
  console.error("Use the frontend static site's HTTPS Render deploy hook.");
  process.exit(1);
}
const status = new URL("release.json", site.endsWith("/") ? site : `${site}/`);
if (status.protocol !== "https:") {
  console.error("The deployed web release must use HTTPS.");
  process.exit(1);
}
url.searchParams.set("ref", commit);
// Never include the hook URL or response body in logs: the URL contains a deploy secret.
try {
  const response = await fetch(url, {
    method: "POST",
    signal: AbortSignal.timeout(30000),
  });
  if (!response.ok) {
    console.error(`Render rejected the web deployment (${response.status}).`);
    process.exit(1);
  }
} catch {
  console.error("Could not reach the Render deployment endpoint.");
  process.exit(1);
}
for (let attempt = 0; attempt < 60; attempt += 1) {
  await Bun.sleep(10000);
  try {
    const check = await fetch(status, {
      cache: "no-store",
      signal: AbortSignal.timeout(10000),
    });
    const data: unknown = check.ok ? await check.json() : null;
    if (
      typeof data === "object" &&
      data !== null &&
      "commit" in data &&
      data.commit === commit &&
      "assetRevision" in data &&
      data.assetRevision === assetRevision
    ) {
      console.info(
        `Web release serves code ${commit} and asset revision ${assetRevision}.`,
      );
      process.exit(0);
    }
  } catch {
    console.info("Web deployment is not reachable yet; retrying verification.");
  }
}
console.error(
  "Web deployment did not serve the requested revision within the verification window.",
);
process.exit(1);
export {};

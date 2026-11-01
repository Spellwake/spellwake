import {
  assetPublicationErrors,
  releaseAssetPackage,
} from "./release-asset-source";

const assets = await releaseAssetPackage();
const errors = assetPublicationErrors(
  assets.scope,
  process.env.RELEASE_ASSETS_APPROVED === "true",
  process.env.RELEASE_PREVIEW_APPROVED === "true",
);
if (errors.length > 0) {
  console.error(errors.join("\n"));
  process.exit(1);
}
console.info("Public web asset prerequisites passed.");

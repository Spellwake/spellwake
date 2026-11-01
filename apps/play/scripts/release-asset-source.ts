import { resolve } from "node:path";

const root = resolve(import.meta.dir, "../../..");

/** Select the declared asset input without changing the local transparent logo. */
export function tableAssetSource(): string {
  return resolve(
    root,
    process.env.SPELLWAKE_RELEASE_ASSETS === "true"
      ? ".release-assets/tabletop"
      : "benchmarks/browser-rendering/public",
  );
}

/** Keep native icon derivation on the same asset package as the table. */
export function logoAssetSource(): string {
  return resolve(
    root,
    process.env.SPELLWAKE_RELEASE_ASSETS === "true"
      ? ".release-assets/logo.svg"
      : "apps/ui-workshop/public/logo.svg",
  );
}

export type AssetScope =
  "internal-study" | "public-preview" | "publication-cleared";

type ReleaseAssetPackage = {
  revision: string | null;
  path:
    | "approved/app-runtime"
    | "studies/assets/app-runtime"
    | "previews/app-runtime";
  scope: AssetScope;
};

/** Bind code, exact asset bytes and publication scope in one checked-in input. */
export async function releaseAssetPackage(): Promise<ReleaseAssetPackage> {
  const data: unknown = await Bun.file(
    resolve(root, "apps/play/release-assets.json"),
  ).json();
  if (
    typeof data === "object" &&
    data !== null &&
    "revision" in data &&
    "path" in data &&
    "scope" in data
  ) {
    const revision = data.revision;
    const path = data.path;
    const scope = data.scope;
    if (
      (revision === null ||
        (typeof revision === "string" && /^[a-f0-9]{40}$/.test(revision))) &&
      ((path === "approved/app-runtime" && scope === "publication-cleared") ||
        (path === "studies/assets/app-runtime" && scope === "internal-study") ||
        (path === "previews/app-runtime" && scope === "public-preview"))
    )
      return { revision, path, scope };
  }
  throw new Error(
    "Invalid immutable runtime asset package revision, path or publication scope.",
  );
}

/** Identify the immutable asset snapshot used by this release. */
export async function releaseAssetRevision(): Promise<string | null> {
  return (await releaseAssetPackage()).revision;
}

/** Public preview approval applies only to direct web deployment, never store publication. */
export function assetPublicationErrors(
  scope: AssetScope,
  approved: boolean,
  webPreviewApproved = false,
): string[] {
  if (scope === "public-preview" && webPreviewApproved) return [];
  const errors: string[] = [];
  if (scope !== "publication-cleared") {
    errors.push(
      "The pinned asset package is not cleared for this distribution scope.",
    );
  }
  if (!approved) {
    errors.push(
      "Set RELEASE_ASSETS_APPROVED only after the packaged artwork is cleared for distribution.",
    );
  }
  return errors;
}

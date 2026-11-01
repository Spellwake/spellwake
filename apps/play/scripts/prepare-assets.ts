import { copyFile, mkdir, rm } from "node:fs/promises";
import { dirname, resolve } from "node:path";
import { tableAssets } from "./table-assets";
import { tableAssetSource } from "./release-asset-source";

const source = tableAssetSource();
const output = resolve(import.meta.dir, "../public");

await rm(output, { recursive: true, force: true });
await Promise.all(
  tableAssets.map(async (asset) => {
    const destination = resolve(output, asset);
    await mkdir(dirname(destination), { recursive: true });
    await copyFile(resolve(source, asset), destination);
  }),
);
console.info(`Prepared ${tableAssets.length} tabletop study assets.`);

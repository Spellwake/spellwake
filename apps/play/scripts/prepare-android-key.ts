import { appendFile, writeFile } from "node:fs/promises";
import { join } from "node:path";

const key = process.env.ANDROID_KEYSTORE_BASE64;
const directory = process.env.RUNNER_TEMP;
const output = process.env.GITHUB_ENV;
if (!key || !directory || !output) {
  console.error("Android keystore and runner environment are required.");
  process.exit(1);
}
const path = join(directory, "spellwake-upload.jks");
await writeFile(path, Buffer.from(key, "base64"), { mode: 0o600 });
await appendFile(output, `ANDROID_KEYSTORE_PATH=${path}\n`);

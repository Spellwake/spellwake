import { mkdir, writeFile, appendFile } from "node:fs/promises";
import { homedir } from "node:os";
import { join } from "node:path";

const key = process.env.APPLE_API_PRIVATE_KEY;
const id = process.env.APPLE_API_KEY;
const output = process.env.GITHUB_ENV;
if (!key || !id || !/^[A-Za-z0-9]+$/.test(id) || !output) {
  console.error(
    "Apple API key, key ID and GitHub environment file are required.",
  );
  process.exit(1);
}
// The standard location also lets Apple's upload tool discover the key.
const directory = join(homedir(), ".appstoreconnect/private_keys");
await mkdir(directory, { recursive: true, mode: 0o700 });
const path = join(directory, `AuthKey_${id}.p8`);
await writeFile(path, key, { mode: 0o600 });
await appendFile(
  output,
  `APPLE_API_KEY_PATH=${path}\nAPI_PRIVATE_KEYS_DIR=${directory}\n`,
);

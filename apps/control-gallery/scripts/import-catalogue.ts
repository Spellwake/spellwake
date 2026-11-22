// Keep the private bulk snapshot on the build host. The import route requests
// only the fields needed to resolve a deck; no card images or rules text.
import { createReadStream } from "node:fs";
import { createInterface } from "node:readline";
import { createGunzip } from "node:zlib";
import { fileURLToPath } from "node:url";
import { pipeline } from "node:stream/promises";
import type { Plugin } from "vite";

export function importCatalogue(): Plugin {
  const module = "virtual:deck-import-catalogue";
  const snapshot = fileURLToPath(
    new URL(
      "../../../work/catalogue/oracle-cards-20261029210158.jsonl.gz",
      import.meta.url,
    ),
  );
  return {
    name: "spellwake-deck-import-catalogue",
    resolveId(id) {
      if (id === module) return "\0" + module;
    },
    async load(id) {
      if (id !== "\0" + module) return;
      this.addWatchFile(snapshot);
      const lines: string[] = [];
      const gzip = createGunzip();
      await pipeline(createReadStream(snapshot), gzip, async (stream) => {
        for await (const line of createInterface({ input: stream })) {
          const c = JSON.parse(line);
          if (
            [
              "token",
              "double_faced_token",
              "emblem",
              "art_series",
              "reversible_card",
            ].includes(c.layout) ||
            !c.oracle_id
          )
            continue;
          lines.push(
            [
              c.name,
              c.oracle_id,
              c.id,
              c.set,
              c.collector_number,
              c.mtgo_id,
              c.mtgo_foil_id,
              c.type_line,
              c.cmc,
            ].join("\t"),
          );
        }
      });
      return "export default " + JSON.stringify(lines.join("\n"));
    },
  };
}

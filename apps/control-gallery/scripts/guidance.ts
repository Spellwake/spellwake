import { execFileSync } from "node:child_process";
import { createRequire } from "node:module";
import { dirname, join } from "node:path";
import { readFileSync } from "node:fs";

const require = createRequire(import.meta.url);
const directory = dirname(require.resolve("modern-web-guidance/package.json"));
const cli = join(directory, "skills/modern-web-guidance/modern-web.mjs");
const version = readFileSync(
  join(dirname(cli), "skill-version.txt"),
  "utf8",
).trim();

// Keep a focused query with its reviewed guide so a dependency update reveals changed matches.
const queries = [
  { query: "open action menu", guide: "menu" },
  { query: "show tooltip", guide: "interest-triggered-tooltips" },
  { query: "switch tab panels", guide: "tabs" },
  { query: "switch tab panels", guide: "anchor-positioning-tab-underline" },
  { query: "mark current navigation", guide: "navbar" },
  { query: "check text contrast", guide: "color" },
  { query: "announce status message", guide: "persistent-toast-notifications" },
  { query: "announce status message", guide: "accessibility" },
  { query: "adjust numeric quantity", guide: "forms" },
  { query: "style native checkboxes", guide: "checkbox" },
  { query: "reduce interaction motion", guide: "motion" },
];
for (const { query, guide } of queries) {
  const result: unknown = JSON.parse(
    execFileSync("node", [cli, "search", query, "--skill-version", version], {
      encoding: "utf8",
    }),
  );
  if (!Array.isArray(result) || !result.some((entry) => entry?.id === guide)) {
    throw new Error(
      `Recheck the guide for "${query}". Expected search result "${guide}".`,
    );
  }
  console.log(`\n${query} → ${guide}\n`);
  execFileSync("node", [cli, "retrieve", guide], {
    stdio: "inherit",
  });
}

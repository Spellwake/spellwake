import { readFileSync } from "node:fs";
import { dirname, resolve, relative } from "node:path";
import { fileURLToPath } from "node:url";
import type { Plugin } from "vite";
import type { ReviewSection, ReviewView } from "../src/review-types.ts";

type SourceAction = {
  label: string;
  outcome?: string;
  to?: string;
  target?: string;
  targets?: string[];
  external?: string;
  href?: string;
};
type SourceView = {
  id: string;
  title: string;
  url: string;
  notes?: string | string[];
  actions?: SourceAction[];
};
type Catalogue = {
  sections: { id: string; title: string; views: SourceView[] }[];
  concepts: SourceView[];
};
const root = fileURLToPath(
  new URL("../../../assets/ui-review/open-play", import.meta.url),
);
const directory = resolve(root, "approved-screens");
const read = (file: string) => JSON.parse(readFileSync(file, "utf8"));
function makeView(view: SourceView): ReviewView {
  const path = resolve(directory, view.url);
  const data = readFileSync(path);
  if (data.readUInt32BE(0) !== 0x89504e47)
    throw new Error("Expected approved PNG: " + path);
  const actions = (view.actions ?? []).flatMap((action) => {
    const targets = action.targets?.length
      ? action.targets
      : [action.target ?? action.to];
    return targets.map((target) => ({
      label: action.label,
      outcome: action.outcome ?? "",
      ...(target ? { target } : {}),
      ...(action.external ? { external: action.external } : {}),
    }));
  });
  return {
    id: view.id,
    title: view.title,
    image: "/" + relative(root, path),
    width: data.readUInt32BE(16),
    height: data.readUInt32BE(20),
    notes: typeof view.notes === "string" ? [view.notes] : (view.notes ?? []),
    actions,
  };
}
export function reviewCatalogue(): Plugin {
  const id = "virtual:approved-screens";
  return {
    name: "spellwake-approved-screens",
    resolveId(source) {
      if (source === id) return "\0" + id;
    },
    load(source) {
      if (source !== "\0" + id) return;
      const catalogueFile = resolve(directory, "catalogue.json");
      this.addWatchFile(catalogueFile);
      const catalogue: Catalogue = read(catalogueFile);
      const sections: ReviewSection[] = catalogue.sections.map((section) => ({
        id: section.id,
        title: section.title,
        views: section.views.map(makeView),
      }));
      const known = new Set(
        sections.flatMap((section) => section.views.map((view) => view.image)),
      );
      for (const section of sections) {
        const pkg = resolve(root, section.id, "package.json");
        if (section.id !== "lobby-details-2026-11-03") continue;
        this.addWatchFile(pkg);
        for (const ref of read(pkg).moduleReferences as (SourceView & {
          file: string;
        })[]) {
          const view = makeView({
            ...ref,
            url: relative(directory, resolve(dirname(pkg), ref.file)),
          });
          if (!known.has(view.image)) section.views.push(view);
        }
      }
      for (const concept of catalogue.concepts)
        sections.push({
          id: concept.id,
          title: concept.title,
          views: [
            makeView({
              ...concept,
              title: concept.title + " · approved concept",
            }),
          ],
        });
      return "export default " + JSON.stringify(sections);
    },
  };
}

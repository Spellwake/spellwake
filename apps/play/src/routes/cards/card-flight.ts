import { flushSync } from "react-dom";

/** The one name the travelling card carries, on its tile before and in the detail pane after. */
const NAME = "card-in-hand";
/** The longest the flight waits for the larger picture before going on without it. */
const PICTURE_WAIT_MS = 320;

const landing = (): HTMLElement | null =>
  document.querySelector<HTMLElement>("[data-card-landing] [data-card-face]");

const wait = (ms: number): Promise<void> =>
  new Promise((done) => window.setTimeout(done, ms));

/**
 * Choosing a card in the gallery: its picture travels from its tile into the
 * detail pane, growing as it goes, so the eye follows the card it chose to
 * where its details are. The tile keeps its place; what travels is the
 * browser's own snapshot of it (the View Transitions API, same-document).
 *
 * Where the API is missing, or the person asks for reduced motion, the choice
 * is simply made. Focus stays on the tile that was pressed.
 */
export function flyToDetail(tile: HTMLElement, choose: () => void): void {
  const from = tile.querySelector<HTMLElement>("[data-card-face]");
  const reduced = window.matchMedia("(prefers-reduced-motion: reduce)").matches;
  if (!("startViewTransition" in document) || !from || reduced) {
    choose();
    return;
  }
  // One element may carry the name at a time: the pane gives it up, the tile takes it.
  const before = landing();
  if (before) before.style.viewTransitionName = "none";
  from.style.viewTransitionName = NAME;
  const root = document.documentElement;
  root.dataset.flight = "card";
  const transition = document.startViewTransition(async () => {
    flushSync(choose);
    from.style.viewTransitionName = "";
    const after = landing();
    if (!after) return;
    after.style.viewTransitionName = NAME;
    // The card lands as its larger picture, so that picture is waited for briefly.
    const picture = after.querySelector("img");
    if (picture && !picture.complete)
      await Promise.race([
        picture.decode().catch(() => undefined),
        wait(PICTURE_WAIT_MS),
      ]);
    await wait(0);
  });
  void transition.finished.finally(() => {
    from.style.viewTransitionName = "";
    const after = landing();
    if (after) after.style.viewTransitionName = "";
    delete root.dataset.flight;
  });
}

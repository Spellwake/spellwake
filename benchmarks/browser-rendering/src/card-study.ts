/** Material showcase shell; play-area.ts is the independent game-view entry. */
import { z } from "zod";
import { mountCardScene } from "./card-scene";
import { createSceneSettings, SCENE_SETTING_IDS } from "./scene-settings";
const settings = createSceneSettings();
const bindings = SCENE_SETTING_IDS.map((id) => {
  const element = z
    .union([
      z.instanceof(HTMLInputElement),
      z.instanceof(HTMLSelectElement),
      z.instanceof(HTMLButtonElement),
      z.instanceof(HTMLOutputElement),
    ])
    .parse(document.getElementById(id));
  const setting = settings[id];
  setting.value = element.value;
  setting.textContent = element.textContent ?? "";
  if (element instanceof HTMLInputElement) setting.checked = element.checked;
  if (element instanceof HTMLSelectElement)
    setting.options = Array.from(element.options).map((option) => ({
      value: option.value,
      textContent: option.textContent ?? "",
      disabled: option.disabled,
    }));
  for (const name of ["input", "change", "click"])
    element.addEventListener(name, () => {
      setting.value = element.value;
      if (element instanceof HTMLInputElement)
        setting.checked = element.checked;
      setting.dispatchEvent(new Event(name));
    });
  return { element, setting };
});
function syncControls(): void {
  for (const { element, setting } of bindings) {
    element.value = setting.value;
    element.hidden = setting.hidden;
    if (!(element instanceof HTMLOutputElement))
      element.disabled = setting.disabled;
    if (element instanceof HTMLInputElement) element.checked = setting.checked;
    if (element instanceof HTMLButtonElement)
      element.textContent = setting.textContent;
    if (element instanceof HTMLSelectElement)
      for (const [index, option] of Array.from(element.options).entries()) {
        const state = setting.options[index];
        if (state) {
          option.textContent = state.textContent;
          option.disabled = state.disabled;
        }
      }
    for (const [name, value] of setting.attributes)
      element.setAttribute(name, value);
  }
}
const element = (id: string) =>
  z.instanceof(HTMLElement).parse(document.getElementById(id));
const status = element("status");
mountCardScene(
  {
    canvas: z.instanceof(HTMLCanvasElement).parse(element("card")),
    status,
    opponentNavigation: z
      .instanceof(HTMLDivElement)
      .parse(element("opponent-navigation")),
    returnBoard: z.instanceof(HTMLButtonElement).parse(element("return-board")),
    stackPanel: z.instanceof(HTMLDivElement).parse(element("stack-preview")),
    stackCount: z.instanceof(HTMLOutputElement).parse(element("stack-count")),
    resolvePreview: z
      .instanceof(HTMLButtonElement)
      .parse(element("resolve-preview")),
    title: element("card-title"),
    hint: element("view-hint"),
    sleeveAttribution: element("sleeve-attribution"),
    onFrame: syncControls,
  },
  settings,
).catch((error: unknown) => {
  console.error("Card showcase failed", error);
  status.textContent = "Unable to render the cards. Reload to retry.";
});

/** Page-independent scene settings; these objects are never DOM controls. */
export class SceneSetting extends EventTarget {
  value = "";
  checked = false;
  disabled = false;
  hidden = false;
  textContent = "";
  options: { value: string; textContent: string; disabled: boolean }[] = [];
  readonly attributes = new Map<string, string>();
  get selectedOptions() {
    return this.options.filter((option) => option.value === this.value);
  }
  setAttribute(name: string, value: string): void {
    this.attributes.set(name, value);
  }
}
export const SCENE_SETTING_IDS = [
  "tilt",
  "turn",
  "light",
  "table-zoom",
  "view",
  "card-back",
  "sleeve-design",
  "ordinary",
  "foil",
  "flip",
  "sleeve",
  "deck-count",
  "deck-mode",
  "deck-performance",
  "deck-count-label",
  "player-count",
  "player-focus",
  "navigation-mode",
  "viewer-seat",
  "table-mode",
  "hand-size",
  "opponent-hand-size",
  "show-hands",
  "hand-summary",
  "inspect-hand",
  "interaction-status",
  "auto-tap",
  "seat-neighbors",
  "reset",
  "compare-deck",
] as const;
export type SceneSettingId = (typeof SCENE_SETTING_IDS)[number];
export type SceneSettings = Record<SceneSettingId, SceneSetting>;

export function createSceneSettings(): SceneSettings {
  return Object.fromEntries(
    SCENE_SETTING_IDS.map((id) => [id, new SceneSetting()]),
  ) as SceneSettings;
}

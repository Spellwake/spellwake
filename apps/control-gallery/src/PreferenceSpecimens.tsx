import { useState } from "react";
import { AudioLines, Droplet, Music, Volume2 } from "lucide-react";
import { RangeSlider, RadioGroup, SettingSwitch } from "@spellwake/ui";
import { type View } from "./references";

const channels = [
  { label: "Master volume", Icon: Volume2 },
  { label: "Music", Icon: Music },
  { label: "Sound effects", Icon: AudioLines },
] as const;
export function TablePreferences({ view }: { view: View }) {
  const changed = view === "CM-07";
  const [volume, setVolume] = useState(changed ? [58, 0, 40] : [80, 50, 80]);
  const [autoTap, setAutoTap] = useState(!changed);
  return (
    <>
      <div className="preference-stage table-stage">
        <div
          data-specimen
          className={"table-preferences" + (view === "CM-10" ? " watcher" : "")}
        >
          {channels.map(({ label, Icon }, i) => (
            <div key={label} className="audio-row">
              <label htmlFor={"audio-" + i}>
                <Icon aria-hidden="true" />
                {label}
              </label>
              <RangeSlider
                id={"audio-" + i}
                label={label}
                min={0}
                max={100}
                step={1}
                value={volume[i]!}
                valueText={volume[i] + "%"}
                autoFocus={changed && i === 1}
                onValueChange={(next) =>
                  setVolume((current) =>
                    current.map((v, j) => (j === i ? next : v)),
                  )
                }
              />
            </div>
          ))}
          <div className="switch-row">
            <label htmlFor="auto-tap">
              <Droplet aria-hidden="true" />
              Auto-tap mana
            </label>
            <SettingSwitch
              id="auto-tap"
              label="Auto-tap mana"
              checked={autoTap}
              onCheckedChange={setAutoTap}
            />
          </div>
        </div>
      </div>
      <div className="readout">
        <strong>Preview values</strong>
        <span>
          {volume.map((n) => n + "%").join(" / ")} · Auto-tap{" "}
          {autoTap ? "on" : "off"}
        </span>
      </div>
    </>
  );
}
export function PictureZoom() {
  const [zoom, setZoom] = useState(145);
  return (
    <>
      <div className="preference-stage account-stage">
        <div className="zoom-specimen" data-specimen>
          <RangeSlider
            label="Zoom"
            min={100}
            max={200}
            step={1}
            value={zoom}
            valueText={zoom + "%"}
            showStepButtons
            onValueChange={setZoom}
          />
        </div>
      </div>
      <div className="readout">
        <strong>Preview zoom</strong>
        <span>{zoom}%</span>
      </div>
    </>
  );
}
const badges = [
  { value: "none", label: "None" },
  { value: "supporter", label: "Supporter", decoration: "✷" },
];
export function BadgeChoice() {
  const [badge, setBadge] = useState("none");
  return (
    <>
      <div className="preference-stage account-stage">
        <div data-specimen className="badge-specimen">
          <RadioGroup
            label="Profile badge"
            options={badges}
            value={badge}
            onValueChange={setBadge}
          />
        </div>
      </div>
      <div className="readout">
        <strong>Badge draft</strong>
        <span>{badge === "none" ? "None" : "Supporter ✷"}</span>
      </div>
    </>
  );
}

export const preferenceUses = [
  {
    name: "Range slider",
    id: "range-slider",
    description: "Audio levels and picture zoom.",
    screens: ["CM-06", "CM-07", "CM-10", "AS-R03"],
  },
  {
    name: "Setting switch",
    id: "switch",
    description: "Auto-tap mana during play and while watching.",
    screens: ["CM-06", "CM-07", "CM-10"],
  },
  {
    name: "Radio group",
    id: "radio-group",
    description: "Draft a profile badge from the eligible choices.",
    screens: ["AS-R04"],
  },
] as const;

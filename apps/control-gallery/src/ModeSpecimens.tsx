import { useState } from "react";
import {
  OriginalCardFace,
  PrintedModePicker,
  type CardImage,
  type PrintedMode,
} from "@spellwake/ui";
import abradeSrc from "../../../assets/ui-review/open-play/in-game/arena-reference/abrade-existing-card.jpg?inline";
import crypticSrc from "../../../assets/ui-review/open-play/in-game/arena-reference/cryptic-command-existing-card.jpg?inline";
import { references, type View } from "./references";

const abrade: CardImage = {
  name: "Abrade",
  src: abradeSrc,
  width: 488,
  height: 680,
};
const cryptic: CardImage = {
  name: "Cryptic Command",
  src: crypticSrc,
  width: 488,
  height: 680,
};
function printed(
  id: string,
  label: string,
  top: number,
  bottom: number,
): PrintedMode {
  return {
    id,
    label,
    region: { x: 37, y: top, width: 414, height: bottom - top },
  };
}
const damage = printed(
  "damage",
  "Abrade deals 3 damage to target creature.",
  449,
  497,
);
const destroy = printed("destroy", "Destroy target artifact.", 497, 526);
const modes = [
  {
    ...printed("counter", "Counter target spell.", 459, 487),
    unavailableReason: "No legal targets",
  },
  printed("return", "Return target permanent to its owner’s hand.", 487, 540),
  printed("tap", "Tap all creatures your opponents control.", 540, 593),
  printed("draw", "Draw a card.", 593, 622),
];
const draftPresets: Partial<Record<View, readonly string[]>> = {
  "MM02:modes": ["draw"],
  "MM03:modes": ["tap", "draw"],
  "MM05:modes": ["return", "draw"],
};
const sources = {
  "MC04:face": {
    image: abrade,
    width: 215,
    modes: [damage],
    tone: "available",
  },
  "MC05:face": {
    image: abrade,
    width: 244,
    modes: [destroy],
    tone: "available",
  },
  "MM06:face": {
    image: cryptic,
    width: 324,
    modes: modes.filter((m) => m.id === "return" || m.id === "draw"),
    tone: "selected",
  },
} as const;
export default function ModeSpecimen({
  view,
  navigate,
}: {
  view: View;
  navigate: (next: View, focusId?: string) => void;
}) {
  const initial = draftPresets[view] ?? [];
  const [selected, setSelected] = useState<readonly string[]>(initial),
    [message, setMessage] = useState("");
  const single = view.startsWith("MC");
  const src = Object.hasOwn(sources, view)
    ? sources[view as keyof typeof sources]
    : undefined;
  const [, , width, height] = references[view].crop;
  const list = single
    ? [
        damage,
        view === "MC02:modes"
          ? { ...destroy, unavailableReason: "No legal targets" }
          : destroy,
      ]
    : modes;
  const pickerView = single
    ? view === "MC04:face" && window.history.state?.from === "MC02:modes"
      ? "MC02:modes"
      : "MC01:modes"
    : "MM05:modes";
  function confirm() {
    if (selected.includes("return") && selected.includes("draw")) {
      navigate("MM06:face", "mode-preview");
      return;
    }
    setMessage(
      "Confirmed modes · " +
        modes
          .filter((m) => selected.includes(m.id))
          .map((m) => m.label)
          .join(" "),
    );
  }
  return (
    <>
      <div className="popup-stage">
        <div
          id="mode-preview"
          tabIndex={-1}
          role="group"
          aria-label={references[view].state}
          className="popup-scene mode-scene"
          data-source={src ? true : undefined}
          style={{ width, height }}
        >
          {src ? (
            <div style={{ width: src.width }}>
              <OriginalCardFace
                image={src.image}
                highlights={src.modes.map((m) => ({
                  id: m.id,
                  region: m.region,
                  tone: src.tone,
                }))}
              />
            </div>
          ) : (
            <PrintedModePicker
              image={single ? abrade : cryptic}
              modes={list}
              faceWidth={single ? 316 : 326}
              gap={single ? 30 : 12}
              choice={
                single
                  ? {
                      kind: "one",
                      onChoose: (id) =>
                        navigate(
                          id === "damage" ? "MC04:face" : "MC05:face",
                          "mode-preview",
                        ),
                    }
                  : {
                      kind: "multiple",
                      count: 2,
                      selected,
                      onChange: (ids) => {
                        setSelected(ids);
                        setMessage("");
                      },
                    }
              }
            />
          )}
        </div>
      </div>
      <div className="readout mode-readout">
        {src ? (
          <button
            type="button"
            onClick={() => navigate(pickerView, "mode-preview")}
          >
            View picker
          </button>
        ) : (
          !single && (
            <>
              <span aria-live="polite">
                {message || `${selected.length} of 2 selected`}
              </span>
              <button
                type="button"
                disabled={selected.length !== 2}
                onClick={confirm}
              >
                Next
              </button>
              <button
                type="button"
                onClick={() => {
                  setSelected(initial);
                  setMessage("");
                }}
              >
                Reset example
              </button>
            </>
          )
        )}
      </div>
    </>
  );
}

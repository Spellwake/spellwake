import { useEffect, useRef } from "react";
import {
  TurnPhaseDisplay,
  MatchActions,
  type MatchActionsProps,
} from "@spellwake/ui";
import { references, type View } from "./references";
import type { ControlUse } from "./UsageMap";

export const hudUses: ControlUse[] = [
  {
    id: "match-phase",
    mapSection: "component",
    approvedCount: 107,
    name: "Turn and phase display",
    description:
      "Five phase names on focus or hover. The active phase follows the game across board views, combat and stack resolution.",
    screens: ["NP-01:hud", "NP-04:hud", "AT-01:hud", "AS05:hud"],
  },
  {
    id: "button",
    name: "Match actions",
    description:
      "The same action controls show Next, the offered combat choices or Resolve. A submitted command can show progress, a result, or yield to the other players.",
    screens: ["NP-01:hud", "AT-04:hud", "BL-01:hud", "AS01:hud", "AS10:hud"],
  },
];

export function MatchHudSpecimen({
  view,
  navigate,
}: {
  view: View;
  navigate: (next: View) => void;
}) {
  const root = useRef<HTMLDivElement>(null);
  const playingSubmission = useRef(false);
  const stack = view.startsWith("AS");
  const blocking = view.startsWith("BL");
  const combat = blocking || view.startsWith("AT");
  const ref = references[view];
  const [, , width, height] = ref.crop;
  useEffect(() => {
    if (view !== "NP-04:hud") return;
    const icon = root.current?.querySelector<HTMLButtonElement>(
      'button[aria-label="Combat"]',
    );
    icon?.focus({ preventScroll: true });
  }, [view]);
  useEffect(() => {
    if (view !== "AS10:hud") {
      playingSubmission.current = false;
      return;
    }
    if (!playingSubmission.current) return;
    const timer = window.setTimeout(() => navigate("AS05:hud"), 1100);
    return () => window.clearTimeout(timer);
  }, [view, navigate]);
  const next = (label: string, target: View) => ({
    label,
    onActivate: () => navigate(target),
  });
  let actions: MatchActionsProps;
  switch (view) {
    case "AS01:hud":
      actions = {
        primary: {
          label: "Resolve",
          onActivate: () => {
            playingSubmission.current = true;
            navigate("AS10:hud");
          },
        },
      };
      break;
    case "AS10:hud":
      actions = {
        primary: { label: "Resolve", pending: true, onActivate: () => {} },
      };
      break;
    case "AS05:hud":
      actions = {};
      break;
    case "AT-01:hud":
    case "AT-04:hud":
      actions = {
        primary: {
          ...next("Attack", "AT-08:hud"),
          disabled: view === "AT-01:hud",
        },
        secondary: next("No Attacks", "AT-09:hud"),
      };
      break;
    case "BL-01:hud":
      actions = {
        primary: { label: "Block", disabled: true, onActivate: () => {} },
        secondary: next("No Blocks", "BL-08:hud"),
      };
      break;
    case "BL-08:hud":
      actions = { status: "No blocks declared." };
      break;
    case "AT-08:hud":
      actions = { status: "Attackers declared" };
      break;
    case "AT-09:hud":
      actions = { status: "No attackers declared" };
      break;
    default:
      actions = { primary: next("Next", "AT-01:hud") };
  }
  return (
    <div className="popup-stage">
      <div
        ref={root}
        className={
          "popup-scene hud-scene" +
          (stack ? " resolving" : blocking ? " blocking" : "")
        }
        style={{ width, height }}
        data-specimen
      >
        <div
          className="match-hud"
          style={{ top: (stack ? 81 : 84) - ref.crop[1] }}
        >
          <TurnPhaseDisplay
            turnLabel={stack || blocking ? "Alex’s turn" : "Your turn"}
            phase={combat ? "combat" : "first-main"}
            phaseLabel={combat ? "Combat" : stack ? "Main" : "Main phase"}
          />
          <div className="match-offer" style={{ width: stack ? 255 : 274 }}>
            <MatchActions {...actions} />
          </div>
        </div>
      </div>
    </div>
  );
}

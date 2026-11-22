import type { ReactElement } from "react";
import { CircleUserRound, LogOut, MessageCircle, Settings } from "lucide-react";
import {
  designPixels,
  MatchActions,
  PlayerIdentity,
  TurnPhaseDisplay,
} from "@spellwake/ui";
import { ImmersiveFrame } from "../../components/shell/ImmersiveFrame";
import { useOrientation, useShellLayout } from "../../lib/display-scale";
import * as frame from "../../components/shell/frames.css";
import * as p from "./probes.css";

const SEATS = ["You", "Alex", "Morgan", "Sam", "Riley", "Jordan"] as const;
const noop = (): void => undefined;
const icon = designPixels(28);

/**
 * Frame probe, development only. It fills the immersive frame with the real
 * match controls from the shared library over a stand-in table and hand, at a
 * six-seat table, so the floating regions can be stressed before the live
 * tabletop is connected.
 */
export function MatchProbe(): ReactElement {
  // A phone held sideways is short, so its hand takes a shallower tray.
  const isShort =
    useShellLayout() === "compact" && useOrientation() === "landscape";
  return (
    <ImmersiveFrame
      playersLabel="Players"
      trayHeight={isShort ? 124 : 196}
      leading={
        <>
          <button
            type="button"
            className={frame.hudButton}
            aria-label="Table settings"
          >
            <Settings size={icon} aria-hidden="true" />
          </button>
          <button
            type="button"
            className={frame.hudButton}
            aria-label="Table chat"
          >
            <MessageCircle size={icon} aria-hidden="true" />
          </button>
        </>
      }
      players={SEATS.map((name, index) => (
        <PlayerIdentity
          key={name}
          presentation="seat"
          name={name}
          life={40}
          actionLabel={`View ${name}`}
          onActivate={noop}
          takingTurn={index === 0}
          portrait={<CircleUserRound aria-hidden="true" />}
        />
      ))}
      trailing={
        <button
          type="button"
          className={frame.hudButton}
          aria-label="Leave table"
        >
          <LogOut size={icon} aria-hidden="true" />
        </button>
      }
      side={
        <>
          <TurnPhaseDisplay
            turnLabel="Your turn"
            phase="combat"
            phaseLabel="Combat"
          />
          <MatchActions
            primary={{ label: "Attack", onActivate: noop, disabled: true }}
            secondary={{ label: "No Attacks", onActivate: noop }}
          />
        </>
      }
      tray={
        <div className={p.hand} aria-hidden="true">
          {Array.from({ length: 7 }, (_, index) => (
            <span key={index} className={p.handCard} />
          ))}
        </div>
      }
    >
      <div className={p.table} aria-hidden="true">
        <div className={p.battlefield}>
          {Array.from({ length: 3 }, (_, index) => (
            <span key={index} className={p.card} />
          ))}
        </div>
      </div>
    </ImmersiveFrame>
  );
}

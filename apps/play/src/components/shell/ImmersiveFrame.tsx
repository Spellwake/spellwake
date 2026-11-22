import type { ReactElement, ReactNode } from "react";
import { assignInlineVars } from "@vanilla-extract/dynamic";
import { designPixels } from "@spellwake/ui";
import * as s from "./frames.css";

/**
 * Frame for a screen that owns the whole viewport, such as a match. The stage
 * fills the screen and controls float in fixed regions that keep clear of each
 * other, of notches, and of the tray along the bottom edge, on every device.
 */
export function ImmersiveFrame({
  children,
  leading,
  players,
  playersLabel,
  trailing,
  side,
  tray,
  trayHeight,
}: {
  /** The stage. It fills the screen behind every control. */
  children: ReactNode;
  /** Top-left controls. */
  leading?: ReactNode;
  /** Seats, in turn order. */
  players?: ReactNode;
  playersLabel: string;
  /** Top-right controls. */
  trailing?: ReactNode;
  /** Turn state and offered actions. */
  side?: ReactNode;
  /** Content along the bottom edge, such as a hand. */
  tray?: ReactNode;
  /** Height reserved for the tray, in design pixels. */
  trayHeight: number;
}): ReactElement {
  return (
    <div
      className={s.immersive}
      style={assignInlineVars({ [s.trayHeight]: designPixels(trayHeight) })}
    >
      <div className={s.immersiveStage}>{children}</div>
      {leading && (
        <div className={s.hudLeading} data-frame="hud-leading">
          {leading}
        </div>
      )}
      {players && (
        <div
          className={s.hudPlayers}
          data-frame="hud-players"
          role="region"
          aria-label={playersLabel}
          tabIndex={0}
        >
          {players}
        </div>
      )}
      {trailing && (
        <div className={s.hudTrailing} data-frame="hud-trailing">
          {trailing}
        </div>
      )}
      {side && (
        <div className={s.hudSide} data-frame="hud-side">
          {side}
        </div>
      )}
      {tray && (
        <div className={s.hudTray} data-frame="hud-tray">
          {tray}
        </div>
      )}
    </div>
  );
}

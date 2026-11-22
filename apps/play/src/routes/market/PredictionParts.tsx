import type { ReactElement } from "react";
import { CalendarDays, Check } from "lucide-react";
import { PredictionsIcon } from "@spellwake/ui";
import {
  longDay,
  type Outcome,
  type PrintingLabel,
} from "../../market/prediction-sources";
import * as s from "./predictions.css";

/** The printing's artwork, cropped square; a quiet glyph when there is no image. */
export function PredictionArt({
  card,
  small = false,
}: {
  card: PrintingLabel;
  small?: boolean;
}): ReactElement {
  return (
    <span
      className={small ? `${s.art} ${s.artSmall}` : s.art}
      aria-hidden="true"
    >
      {card.faceUrl ? (
        <span
          className={s.artImage}
          style={{ backgroundImage: `url("${card.faceUrl}")` }}
        />
      ) : (
        <PredictionsIcon className={s.artGlyph} />
      )}
    </span>
  );
}

export function DateChip({ day }: { day: string }): ReactElement {
  return (
    <span className={s.dateChip}>
      <CalendarDays aria-hidden="true" />
      {longDay(day)}
    </span>
  );
}

const OUTCOME_LABEL: Record<Outcome | "pending", string> = {
  yes: "Yes",
  no: "No",
  void: "Void",
  pending: "Pending",
};
/** A public outcome, or Pending before one exists. Informational, never a control. */
export function OutcomePill({
  outcome,
  prefix,
  checked = false,
}: {
  outcome: Outcome | null;
  prefix?: string;
  checked?: boolean;
}): ReactElement {
  const key = outcome ?? "pending";
  return (
    <span className={s.pill} data-tone={key === "void" ? undefined : key}>
      {checked && <Check aria-hidden="true" />}
      {prefix ? `${prefix}: ${OUTCOME_LABEL[key]}` : OUTCOME_LABEL[key]}
    </span>
  );
}

/** Yes or No as the side a bet took. */
export function SidePill({
  side,
  small = false,
}: {
  side: "yes" | "no";
  small?: boolean;
}): ReactElement {
  return (
    <span className={small ? s.sideChip : s.pill} data-tone={side}>
      {side === "yes" ? "Yes" : "No"}
    </span>
  );
}

/** A trail of sparks after the page title, drawn rather than painted. */
export function TitleTrail(): ReactElement {
  return (
    <svg className={s.trail} viewBox="0 0 190 44" aria-hidden="true">
      <defs>
        <linearGradient id="prediction-trail" x1="0" x2="1">
          <stop offset="0" stopColor="#d9b878" stopOpacity="0" />
          <stop offset="0.55" stopColor="#e7cf9f" stopOpacity="0.75" />
          <stop offset="1" stopColor="#f3b45a" stopOpacity="0.9" />
        </linearGradient>
      </defs>
      <path
        d="M2 40 C 52 34, 84 12, 120 9 S 170 12, 186 18"
        fill="none"
        stroke="url(#prediction-trail)"
        strokeWidth="1.4"
        strokeLinecap="round"
      />
      {[
        [126, 9, 1.4],
        [140, 8.6, 1.1],
        [152, 9.4, 1.6],
        [163, 11, 1.1],
        [174, 13.4, 1.3],
        [186, 18, 1.8],
      ].map(([cx, cy, r]) => (
        <circle key={cx} cx={cx} cy={cy} r={r} fill="#f6d79c" />
      ))}
    </svg>
  );
}

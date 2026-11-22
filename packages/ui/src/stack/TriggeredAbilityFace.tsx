import { useLayoutEffect, useRef, useState, type ReactNode } from "react";
import frame from "./extended-art-frame.svg?inline";
import * as s from "./stack.css";

// The bands share an original-coordinate viewBox so the printed bevels stay the
// same thickness when the rules field takes over the art window.
function FrameBand({ top, bottom }: { top: number; bottom: number }) {
  return (
    <svg
      aria-hidden
      className={s.frameBand}
      viewBox={`0 ${top} 1062 ${bottom - top}`}
      preserveAspectRatio="none"
    >
      <image href={frame} width={1062} height={1482} />
    </svg>
  );
}

export interface TriggeredAbilityContent {
  name: string;
  art?: ReactNode;
  typeLabel: string;
  effect: string;
}

export function TriggeredAbilityFace({
  name,
  art,
  typeLabel,
  effect,
}: TriggeredAbilityContent) {
  const rules = useRef<HTMLSpanElement>(null);
  const [fitsArtPanel, setFitsArtPanel] = useState(false);
  useLayoutEffect(() => {
    const text = rules.current;
    if (!text) return;
    // At stack size the illustrated frame has 35px for rules, including padding.
    const measure = () => setFitsArtPanel(text.offsetHeight <= 35);
    measure();
    const observer = new ResizeObserver(measure);
    observer.observe(text);
    return () => observer.disconnect();
  }, [effect]);
  const illustrated = Boolean(art) && fitsArtPanel;
  return (
    <div className={s.ability} data-illustrated={illustrated}>
      {illustrated && <div className={s.abilityArt}>{art}</div>}
      <div className={s.abilityName}>
        <FrameBand top={0} bottom={202} />
        <strong>{name}</strong>
      </div>
      <div className={s.abilityType}>
        <FrameBand top={illustrated ? 970 : 1003} bottom={1120} />
        <b>{typeLabel}</b>
      </div>
      <div className={s.abilityRules}>
        <FrameBand top={1120} bottom={1350} />
        <span ref={rules}>{effect}</span>
      </div>
      <div className={s.abilityFoot}>
        <FrameBand top={1350} bottom={1482} />
      </div>
    </div>
  );
}

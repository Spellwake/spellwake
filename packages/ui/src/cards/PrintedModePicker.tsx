import { useId, type CSSProperties } from "react";
import { AnimatePresence, motion } from "motion/react";
import { Check } from "lucide-react";
import { controlMotion } from "../control-motion";
import { useAllowsControlMotion } from "../useAllowsControlMotion";
import {
  OriginalCardFace,
  regionStyle,
  type CardImage,
  type PrintedRegion,
} from "./OriginalCardFace";
import * as s from "./cards.css";
import { designPixels } from "../design-pixels";

export interface PrintedMode {
  id: string;
  label: string;
  region: PrintedRegion;
  unavailableReason?: string;
}
type ModeChoice =
  | { kind: "one"; onChoose: (id: string) => void }
  | {
      kind: "multiple";
      count: number;
      selected: readonly string[];
      onChange: (ids: string[]) => void;
    };
export interface PrintedModePickerProps {
  image: CardImage;
  modes: readonly PrintedMode[];
  choice: ModeChoice;
  faceWidth: number;
  gap: number;
}
/** Each copy of the face chooses the mode whose paragraph it outlines. The host owns legality, selection and progression. */
export function PrintedModePicker({
  image,
  modes,
  choice,
  faceWidth,
  gap,
}: PrintedModePickerProps) {
  const prefix = useId();
  const animate = useAllowsControlMotion();
  const selected =
    choice.kind === "multiple" ? new Set(choice.selected) : new Set<string>();
  const full = choice.kind === "multiple" && selected.size >= choice.count;
  function choose(id: string) {
    if (choice.kind === "one") {
      choice.onChoose(id);
      return;
    }
    const next = new Set(selected);
    if (next.has(id)) next.delete(id);
    else if (!full) next.add(id);
    choice.onChange(modes.filter((m) => next.has(m.id)).map((m) => m.id));
  }
  return (
    <div
      className={s.choices}
      role="group"
      aria-label={
        choice.kind === "one"
          ? "Choose one mode"
          : `Choose ${choice.count} modes`
      }
      style={{ "--face-width": designPixels(faceWidth), gap } as CSSProperties}
    >
      {modes.map((mode) => {
        const active = selected.has(mode.id),
          disabled = Boolean(mode.unavailableReason) || (full && !active);
        const reasonId = prefix + mode.id;
        return (
          <div key={mode.id} className={s.option}>
            <OriginalCardFace image={image}>
              {/* The whole copy of the face chooses its mode, so the target is
                  never smaller than a card; the outline stays on the mode's
                  own printed paragraph. */}
              <button
                type="button"
                className={s.modeHit}
                aria-label={mode.label}
                aria-pressed={choice.kind === "multiple" ? active : undefined}
                disabled={disabled}
                aria-describedby={mode.unavailableReason ? reasonId : undefined}
                onClick={() => choose(mode.id)}
              >
                <span
                  className={s.mark}
                  style={regionStyle(mode.region, image)}
                  data-tone={
                    active ? "selected" : disabled ? "muted" : "available"
                  }
                >
                  <AnimatePresence>
                    {active && (
                      <motion.span
                        aria-hidden
                        className={s.tick}
                        initial={animate ? { opacity: 0, scale: 0.8 } : false}
                        animate={{ opacity: 1, scale: 1 }}
                        exit={{ opacity: 0 }}
                        transition={{
                          duration: animate ? controlMotion.feedback : 0,
                          ease: controlMotion.ease,
                        }}
                      >
                        <Check size="100%" strokeWidth={3} />
                      </motion.span>
                    )}
                  </AnimatePresence>
                </span>
              </button>
            </OriginalCardFace>
            {mode.unavailableReason && (
              <span id={reasonId} className={s.reason}>
                {mode.unavailableReason}
              </span>
            )}
          </div>
        );
      })}
    </div>
  );
}

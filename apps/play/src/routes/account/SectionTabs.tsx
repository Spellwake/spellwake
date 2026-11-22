import type { KeyboardEvent, ReactElement } from "react";
import { motion } from "motion/react";
import { motionLanguage as m } from "../../lib/motion";
import * as s from "./account.css";

/**
 * The column of sections beside a settings-style dialog's content, with one
 * plate that travels to the chosen section. Arrow keys, Home and End move
 * between the sections that are not disabled.
 */
export function SectionTabs<Section extends string>({
  id,
  label,
  sections,
  labels,
  current,
  isDisabled = () => false,
  onChange,
}: {
  /** Prefix for the tab and panel ids; the panel is `${id}-panel`. */
  id: string;
  /** The accessible name of the tab list. */
  label: string;
  sections: readonly Section[];
  labels: Record<Section, string>;
  current: Section;
  isDisabled?: (section: Section) => boolean;
  onChange: (section: Section) => void;
}): ReactElement {
  const available = sections.filter((section) => !isDisabled(section));
  const onKeyDown = (event: KeyboardEvent<HTMLDivElement>): void => {
    const index = available.indexOf(current);
    const step =
      event.key === "ArrowDown" || event.key === "ArrowRight"
        ? 1
        : event.key === "ArrowUp" || event.key === "ArrowLeft"
          ? -1
          : 0;
    const next =
      event.key === "Home"
        ? available[0]
        : event.key === "End"
          ? available.at(-1)
          : step !== 0
            ? available[(index + step + available.length) % available.length]
            : undefined;
    if (!next) return;
    event.preventDefault();
    onChange(next);
    event.currentTarget
      .querySelector<HTMLElement>(`[data-section="${next}"]`)
      ?.focus();
  };
  return (
    <div
      role="tablist"
      aria-label={label}
      className={s.sections}
      onKeyDown={onKeyDown}
    >
      {sections.map((section) => {
        const isSelected = section === current;
        const disabled = isDisabled(section);
        return (
          <button
            key={section}
            type="button"
            role="tab"
            id={`${id}-tab-${section}`}
            data-section={section}
            className={s.section}
            aria-selected={isSelected}
            aria-controls={`${id}-panel`}
            aria-disabled={disabled || undefined}
            tabIndex={isSelected ? 0 : -1}
            onClick={() => {
              if (!disabled) onChange(section);
            }}
          >
            {isSelected && (
              <motion.span
                layoutId={`${id}-plate`}
                className={s.sectionPlate}
                transition={{
                  duration: m.duration.marker,
                  ease: m.ease.travel,
                }}
              />
            )}
            <span className={s.sectionLabel}>{labels[section]}</span>
          </button>
        );
      })}
    </div>
  );
}

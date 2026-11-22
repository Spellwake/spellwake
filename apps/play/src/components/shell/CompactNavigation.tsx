import type { ReactElement } from "react";
import { Link, useLocation } from "react-router-dom";
import { motion } from "motion/react";
import { destinations, type Destination } from "../../routes/navigation";
import { motionLanguage as m } from "../../lib/motion";
import * as s from "./compact-navigation.css";

const travel = { duration: m.duration.marker, ease: m.ease.arrive } as const;

/**
 * Sections of the current destination on small layouts: a strip under the
 * header, or inline within the header when a phone is on its side and has no
 * height to give a second bar.
 */
export function CompactSections({
  current,
  placement,
}: {
  current: Destination | undefined;
  placement: "below" | "inline";
}): ReactElement | null {
  const { pathname } = useLocation();
  if (!current?.sections.length) return null;
  return (
    <nav
      className={placement === "inline" ? s.stripInline : s.strip}
      aria-label={current.label}
      data-frame={placement === "inline" ? undefined : "strip"}
    >
      {current.sections.map((section) => {
        const isCurrent = section.path === pathname;
        return (
          <Link
            key={section.path}
            to={section.path}
            className={s.stripLink}
            aria-current={isCurrent ? "page" : undefined}
          >
            {isCurrent && (
              <motion.span
                layoutId="compact-section-mark"
                className={s.stripMark}
                transition={travel}
                aria-hidden="true"
              />
            )}
            {section.label}
          </Link>
        );
      })}
    </nav>
  );
}

/**
 * Primary destinations for small layouts: a dock floating over the content.
 * Only the current destination spells out its name; the fill travels to it.
 */
export function CompactNavigation({
  current,
}: {
  current: Destination | undefined;
}): ReactElement {
  return (
    <nav className={s.dock} aria-label="Application" data-frame="dock">
      {destinations.map((destination) => {
        const isCurrent = destination.id === current?.id;
        return (
          <Link
            key={destination.id}
            to={destination.path}
            className={s.dockItem}
            aria-current={isCurrent ? "page" : undefined}
            aria-label={destination.label}
          >
            {isCurrent && (
              <motion.span
                layoutId="compact-dock-fill"
                className={s.dockFill}
                transition={travel}
                aria-hidden="true"
              />
            )}
            <span className={s.glyph} aria-hidden="true">
              {destination.icon}
            </span>
            {isCurrent && (
              <span className={s.dockLabel}>{destination.label}</span>
            )}
          </Link>
        );
      })}
    </nav>
  );
}

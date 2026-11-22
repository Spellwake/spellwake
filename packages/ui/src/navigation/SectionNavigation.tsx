import { useId, type MouseEvent, type ReactNode } from "react";
import { motion } from "motion/react";
import { controlMotion } from "../control-motion";
import { useAllowsControlMotion } from "../useAllowsControlMotion";
import * as s from "./navigation.css";
export interface SectionLink {
  href: string;
  label: string;
  icon: ReactNode;
}
export interface SectionNavigationProps {
  label: string;
  links: readonly SectionLink[];
  currentHref: string;
  /** "clear" removes the rail's own panel so the host's surface shows through. */
  surface?: "panel" | "clear";
  onNavigate?: (
    event: MouseEvent<HTMLAnchorElement>,
    link: SectionLink,
  ) => void;
}
/** Native links keep browser navigation, including modified clicks and opening a new tab. */
export function SectionNavigation({
  label,
  links,
  currentHref,
  surface = "panel",
  onNavigate,
}: SectionNavigationProps) {
  const id = useId(),
    animate = useAllowsControlMotion();
  return (
    <nav
      aria-label={label}
      className={[s.theme, s.rail, surface === "clear" ? s.clearRail : ""].join(
        " ",
      )}
    >
      {links.map((link) => (
        <a
          key={link.href}
          href={link.href}
          className={s.link}
          aria-current={link.href === currentHref ? "page" : undefined}
          onClick={(event) => {
            if (
              event.defaultPrevented ||
              event.button !== 0 ||
              event.metaKey ||
              event.ctrlKey ||
              event.altKey ||
              event.shiftKey
            )
              return;
            onNavigate?.(event, link);
          }}
        >
          {link.href === currentHref && (
            <motion.span
              aria-hidden
              className={s.railMark}
              initial={false}
              {...(animate ? { layoutId: id } : {})}
              transition={{
                duration: animate ? controlMotion.settle : 0,
                ease: controlMotion.ease,
              }}
            />
          )}
          <span aria-hidden className={s.railIcon}>
            {link.icon}
          </span>
          {link.label}
        </a>
      ))}
    </nav>
  );
}

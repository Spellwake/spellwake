import type { CSSProperties, ReactNode } from "react";
import { motion } from "motion/react";
import { controlMotion } from "../control-motion";
import { useAllowsControlMotion } from "../useAllowsControlMotion";
import * as s from "./identity.css";
import { designPixels } from "../design-pixels";

export interface PlayerAvatarProps {
  children: ReactNode;
  diameter: number;
  label?: string;
  ring?: "subtle" | "gold" | "selected";
}
/** The caller supplies portrait artwork. Omit label when the adjacent identity already names the player. */
export function PlayerAvatar({
  children,
  diameter,
  label,
  ring = "subtle",
}: PlayerAvatarProps) {
  const animate = useAllowsControlMotion();
  return (
    <span
      className={s.avatar}
      data-ring={ring}
      style={{ "--sw-avatar-size": designPixels(diameter) } as CSSProperties}
      role={label ? "img" : undefined}
      aria-label={label}
      aria-hidden={label ? undefined : true}
    >
      <motion.span
        aria-hidden
        className={s.halo}
        initial={false}
        animate={{ opacity: ring === "selected" ? 1 : 0 }}
        transition={{
          duration: animate ? controlMotion.settle : 0,
          ease: controlMotion.ease,
        }}
      />
      <span aria-hidden className={s.art}>
        {children}
      </span>
    </span>
  );
}

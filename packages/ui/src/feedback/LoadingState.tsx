import type { CSSProperties, ReactNode } from "react";
import { motion } from "motion/react";
import { controlMotion } from "../control-motion";
import { useAllowsControlMotion } from "../useAllowsControlMotion";
import * as s from "./loading.css";

export interface LoadingStateProps {
  label: string;
  children: ReactNode;
}
/** One accessible status for a group of decorative placeholders. The caller owns read completion. */
export function LoadingState({ label, children }: LoadingStateProps) {
  const animate = useAllowsControlMotion();
  return (
    <div role="status" className={s.status}>
      <span className={s.srOnly}>{label}</span>
      <motion.div
        aria-hidden
        initial={animate ? { opacity: 0.35 } : false}
        animate={{ opacity: 1 }}
        transition={{
          duration: animate ? controlMotion.settle : 0,
          ease: controlMotion.ease,
        }}
      >
        {children}
      </motion.div>
    </div>
  );
}
export interface LoadingShapeProps {
  shape?: "line" | "emblem" | "portrait";
  width: CSSProperties["width"];
  height: CSSProperties["height"];
  tone?: "muted" | "bright";
}
export function LoadingShape({
  shape = "line",
  width,
  height,
  tone = "muted",
}: LoadingShapeProps) {
  return (
    <span
      aria-hidden
      className={s.shape}
      data-shape={shape}
      data-tone={tone}
      style={{ width, height }}
    />
  );
}

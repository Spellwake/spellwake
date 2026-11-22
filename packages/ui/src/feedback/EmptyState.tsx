import type { ReactNode } from "react";
import { motion } from "motion/react";
import { controlMotion } from "../control-motion";
import { useAllowsControlMotion } from "../useAllowsControlMotion";
import * as s from "./feedback.css";

export type EmptyStateProps =
  | {
      treatment: "quiet" | "contained";
      title: string;
      description?: never;
      action?: never;
    }
  | {
      treatment?: "plain" | "panel";
      title: string;
      description?: string;
      action?: ReactNode;
    };

/** Successful empty list or group. The owner supplies the scoped message and optional action. */
export function EmptyState({
  title,
  description,
  action,
  treatment = "plain",
}: EmptyStateProps) {
  const animate = useAllowsControlMotion();
  return (
    <motion.div
      className={[s.theme, s.empty, treatment === "panel" ? s.panel : ""].join(
        " ",
      )}
      initial={animate ? { opacity: 0.6, y: 3 } : false}
      animate={{ opacity: 1, y: 0 }}
      transition={{
        duration: animate ? controlMotion.settle : 0,
        ease: controlMotion.ease,
      }}
    >
      <p
        className={
          treatment === "contained"
            ? s.contained
            : treatment === "quiet"
              ? s.quiet
              : s.title
        }
      >
        {title}
      </p>
      {description && <p className={s.description}>{description}</p>}
      {action && <div className={s.emptyAction}>{action}</div>}
    </motion.div>
  );
}

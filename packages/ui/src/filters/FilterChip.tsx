import { useAllowsControlMotion } from "../useAllowsControlMotion";
import { motion, useIsPresent } from "motion/react";
import { X } from "lucide-react";
import { controlMotion } from "../control-motion";
import * as s from "./filters.css";
export interface FilterChipProps {
  label: string;
  onRemove: () => void;
  treatment?: "pill" | "compact" | "outlined";
}
/** Removes a single active restriction. AnimatePresence in the hosting list enables exit motion. */
export function FilterChip({
  label,
  onRemove,
  treatment = "pill",
}: FilterChipProps) {
  const present = useIsPresent();
  const reduced = !useAllowsControlMotion();
  return (
    <motion.button
      type="button"
      layout={reduced ? false : "position"}
      initial={false}
      animate={{ opacity: 1, scale: 1 }}
      exit={{ opacity: 0, scale: reduced ? 1 : 0.92 }}
      transition={{
        duration: reduced ? 0 : controlMotion.feedback,
        ease: controlMotion.ease,
      }}
      className={[
        s.chip,
        treatment === "compact" && s.compact,
        treatment === "outlined" && s.outlinedChip,
      ]
        .filter(Boolean)
        .join(" ")}
      aria-label={`Remove ${label} filter`}
      aria-hidden={!present || undefined}
      disabled={!present}
      onClick={onRemove}
    >
      <span className={s.chipEntry}>{label}</span>
      <X aria-hidden="true" />
    </motion.button>
  );
}

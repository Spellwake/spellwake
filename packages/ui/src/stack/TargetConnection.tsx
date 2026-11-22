import { useId } from "react";
import { motion } from "motion/react";
import { controlMotion } from "../control-motion";
import { useAllowsControlMotion } from "../useAllowsControlMotion";
import * as s from "./stack.css";

export interface AnchorPoint {
  x: number;
  y: number;
}
/** Endpoints are in CSS pixels relative to the overlay's positioned parent. The arrowhead marks the destination. */
export function TargetConnection({
  from,
  to,
  bend = 0,
}: {
  from: AnchorPoint;
  to: AnchorPoint;
  bend?: number;
}) {
  const allowsMotion = useAllowsControlMotion();
  const head = useId().replace(/:/g, "");
  const dx = to.x - from.x,
    dy = to.y - from.y,
    length = Math.hypot(dx, dy);
  if (!length) return null;
  const cx = (from.x + to.x) / 2 - (dy / length) * bend,
    cy = (from.y + to.y) / 2 + (dx / length) * bend;
  const path = `M ${from.x} ${from.y} Q ${cx} ${cy} ${to.x} ${to.y}`;
  const local = length < 120;
  return (
    <svg className={s.connection} aria-hidden="true" focusable="false">
      <defs>
        <marker
          id={head}
          markerUnits="userSpaceOnUse"
          markerWidth={local ? 12 : 10}
          markerHeight={local ? 12 : 10}
          refX="9"
          refY="5"
          viewBox="0 0 10 10"
          orient="auto"
        >
          <path
            d="M2 1.5 L9 5 L2 8.5"
            fill="none"
            stroke="#fff7e4"
            strokeWidth={local ? 1.75 : 1.7}
            strokeLinecap="round"
            strokeLinejoin="round"
          />
        </marker>
      </defs>
      <motion.path
        initial={allowsMotion ? { opacity: 0, d: path } : false}
        animate={{ opacity: 1, d: path }}
        transition={{
          duration: allowsMotion ? controlMotion.feedback : 0,
          opacity: {
            duration: allowsMotion ? controlMotion.feedback : 0,
            delay: allowsMotion ? controlMotion.settle : 0,
          },
          ease: controlMotion.ease,
        }}
        fill="none"
        stroke="#fff7e4"
        strokeLinecap="butt"
        strokeWidth={local ? 2 : 1.65}
        markerEnd={`url(#${head})`}
      />
    </svg>
  );
}

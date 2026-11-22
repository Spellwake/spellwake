import { motion } from "motion/react";
import { controlMotion } from "../control-motion";
import { useAllowsControlMotion } from "../useAllowsControlMotion";
import * as s from "./castingCost.css";

export interface ManaSymbol {
  /** Accessible name for this mana symbol, such as "one red mana". */
  name: string;
  src: string;
}
export interface ManaCostProps {
  label: string;
  symbols: readonly ManaSymbol[];
  size?: "preview" | "payment";
}

/** Renders the host's ordered, already-priced mana symbols. Repeated symbols remain distinct. */
export function ManaCost({ label, symbols, size = "preview" }: ManaCostProps) {
  const animate = useAllowsControlMotion();
  const occurrences = new Map<string, number>();
  return (
    <div role="status" aria-atomic="true" className={s.cost} data-size={size}>
      <span>{label}</span>
      <span className={s.symbols}>
        {symbols.map((symbol) => {
          const occurrence = occurrences.get(symbol.src) ?? 0;
          occurrences.set(symbol.src, occurrence + 1);
          return (
            <motion.img
              key={symbol.src + ":" + occurrence}
              alt={symbol.name}
              src={symbol.src}
              initial={animate ? { opacity: 0.6, y: 2 } : false}
              animate={{ opacity: 1, y: 0 }}
              transition={{
                duration: animate ? controlMotion.feedback : 0,
                ease: controlMotion.ease,
              }}
            />
          );
        })}
      </span>
    </div>
  );
}

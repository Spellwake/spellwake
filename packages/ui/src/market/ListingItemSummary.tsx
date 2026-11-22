import { motion, useAnimationControls } from "motion/react";
import { useLayoutEffect, useRef, type ReactNode } from "react";
import { CoinAmount, type CoinAmountProps } from "../amounts/CoinAmount";
import { useAllowsControlMotion } from "../useAllowsControlMotion";
import { controlMotion } from "../control-motion";
import * as s from "./listing.css";

export type ListingItem = {
  name: string;
  quantity: number;
  artwork: ReactNode;
} & ({ kind: "card"; setCode: string; finish: string } | { kind: "pack" });

export interface ListingItemSummaryProps {
  item: ListingItem;
  seller?: string;
  layout?: "inline" | "detail" | "inspector";
  /** The total for this entire listing. */
  price?: CoinAmountProps;
}

/** The artwork is decorative here because the adjacent text identifies the item and printing. */
export function ListingItemSummary({
  item,
  seller,
  layout = "inline",
  price,
}: ListingItemSummaryProps) {
  const quantity = `${item.quantity} ${item.kind === "pack" ? (item.quantity === 1 ? "pack" : "packs") : item.quantity === 1 ? "copy" : "copies"}`;
  const printing = item.kind === "card" ? [item.setCode, item.finish] : [];
  const detail = layout !== "inline";
  const metadata = [...printing, quantity, ...(seller ? [seller] : [])].join(
    "  ·  ",
  );
  const revision = JSON.stringify([item.kind, item.name, metadata]);
  const previous = useRef(revision),
    animation = useAnimationControls(),
    animate = useAllowsControlMotion();
  useLayoutEffect(() => {
    if (previous.current !== revision && animate) {
      void animation.start({
        opacity: [0.65, 1],
        transition: {
          duration: controlMotion.settle,
          ease: controlMotion.ease,
        },
      });
    } else animation.set({ opacity: 1 });
    previous.current = revision;
    return () => animation.stop();
  }, [animation, animate, revision]);

  return (
    <span className={s.summary} data-layout={layout} data-kind={item.kind}>
      <span aria-hidden="true" className={s.art}>
        {item.artwork}
      </span>
      <motion.span initial={false} animate={animation} className={s.copy}>
        <span className={s.title}>{item.name}</span>
        {detail ? (
          <>
            {printing.length > 0 && <span>{printing.join("  ·  ")}</span>}
            <span>{quantity}</span>
            {seller && <span>Seller: {seller}</span>}
          </>
        ) : (
          <span>{metadata}</span>
        )}
      </motion.span>
      {price && (
        <span className={s.price}>
          <CoinAmount {...price} />
        </span>
      )}
    </span>
  );
}

import { motion } from "motion/react";
import { ListingItemSummary, type ListingItem } from "./ListingItemSummary";
import type { CoinAmountProps } from "../amounts/CoinAmount";
import { controlMotion } from "../control-motion";
import { useAllowsControlMotion } from "../useAllowsControlMotion";
import * as s from "./listing.css";

export interface ListingRowProps {
  item: ListingItem;
  seller: string;
  price: CoinAmountProps;
  selected: boolean;
  onSelect: () => void;
}

/** Selects a listing for inspection. The owning view handles purchase or cancellation separately. */
export function ListingRow({
  item,
  seller,
  price,
  selected,
  onSelect,
}: ListingRowProps) {
  const animate = useAllowsControlMotion();
  return (
    <button
      type="button"
      className={s.row}
      data-kind={item.kind}
      aria-pressed={selected}
      onClick={onSelect}
    >
      <motion.span
        className={s.selection}
        aria-hidden="true"
        initial={false}
        animate={{ opacity: selected ? 1 : 0 }}
        transition={{
          duration: animate ? controlMotion.settle : 0,
          ease: controlMotion.ease,
        }}
      />
      <ListingItemSummary item={item} seller={seller} price={price} />
    </button>
  );
}

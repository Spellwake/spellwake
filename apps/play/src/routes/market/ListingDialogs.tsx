import { useRef, type ReactElement, type RefObject } from "react";
import {
  CoinAmount,
  ConfirmationDialog,
  ListingItemSummary,
  type ListingItem as SummaryItem,
} from "@spellwake/ui";
import {
  listingName,
  type Listing,
  type ListingItem,
  type Operation,
} from "../../market/listing-sources";
import { CardFace } from "../cards/CardFace";
import { PackWrapper } from "../packs/PackWrapper";
import * as s from "./market.css";

export const COIN = `${import.meta.env.BASE_URL}coin.svg`;

/** A listed item's picture: the printed card face, or the drawn pack wrapper. */
export function ListingArt({ item }: { item: ListingItem }): ReactElement {
  return item.kind === "card" ? (
    <CardFace url={item.faceUrl} name={item.name} />
  ) : (
    <span className={s.packArt}>
      <PackWrapper setName={item.setName} product={item.product} />
    </span>
  );
}
/** The shape the library's listing summary and row take. */
export function summaryItem(item: ListingItem, quantity: number): SummaryItem {
  const base = {
    name: listingName(item),
    quantity,
    artwork: <ListingArt item={item} />,
  };
  return item.kind === "card"
    ? {
        ...base,
        kind: "card",
        setCode: item.setCode ?? item.setName,
        finish: item.finish,
      }
    : { ...base, kind: "pack" };
}

/** The line under a held-open command: why it cannot go on, or that its outcome is unknown. */
function StageLine({
  operation,
  unconfirmed,
  blocked,
}: {
  operation: Operation | null;
  unconfirmed: string;
  blocked: string | null;
}): ReactElement | null {
  const stage = operation?.stage;
  const text =
    stage?.kind === "unconfirmed"
      ? unconfirmed
      : stage?.kind === "rejected"
        ? stage.message
        : blocked;
  if (!text) return null;
  return (
    <p
      className={s.stageLine}
      data-tone={stage?.kind === "unconfirmed" ? "unknown" : "known"}
      role="status"
    >
      {text}
    </p>
  );
}

/**
 * Buy listing: the whole lot at its total, confirmed once. It stays open
 * while the purchase is in flight or unconfirmed, then only Check status can
 * settle it; a known reason, such as too few coins, is said in place of
 * buying.
 */
export function BuyDialog({
  open,
  listing,
  operation,
  balance,
  canTrade,
  onBuy,
  onCheck,
  onClose,
  returnFocusRef,
}: {
  open: boolean;
  listing: Listing | null;
  operation: Operation | null;
  balance: number | null;
  canTrade: boolean;
  onBuy: () => void;
  onCheck: () => void;
  onClose: () => void;
  returnFocusRef: RefObject<HTMLElement | null>;
}): ReactElement | null {
  // The last listing shown stays while the dialog animates closed.
  const last = useRef<Listing | null>(null);
  const current = operation?.kind === "buy" ? operation.listing : listing;
  if (current) last.current = current;
  const shown = current ?? last.current;
  if (!shown) return null;
  const stage = operation?.kind === "buy" ? operation.stage : null;
  const isUnconfirmed = stage?.kind === "unconfirmed";
  const shortOfCoins =
    balance !== null && shown.total !== null && shown.total > balance;
  return (
    <ConfirmationDialog
      open={open}
      appearance="market"
      title="Buy listing"
      cancelLabel="Cancel"
      confirmLabel={isUnconfirmed ? "Check status" : "Buy"}
      divider
      dismissOnBackdrop
      returnFocusRef={returnFocusRef}
      pending={stage?.kind === "pending" || (isUnconfirmed && stage.isChecking)}
      locked={isUnconfirmed}
      confirmDisabled={
        !isUnconfirmed &&
        (shortOfCoins ||
          !canTrade ||
          shown.total === null ||
          stage?.kind === "rejected")
      }
      onCancel={onClose}
      onConfirm={isUnconfirmed ? onCheck : onBuy}
    >
      <div className={s.dialogItem}>
        <ListingItemSummary
          item={summaryItem(shown.item, shown.quantity)}
          seller={shown.seller.name}
          layout="detail"
        />
      </div>
      <div className={s.total}>
        Total
        <CoinAmount amount={shown.total} iconSrc={COIN} />
      </div>
      <StageLine
        operation={operation?.kind === "buy" ? operation : null}
        unconfirmed="Purchase not confirmed."
        blocked={
          shortOfCoins
            ? "Not enough available coins."
            : !canTrade
              ? "Buying is unavailable."
              : null
        }
      />
    </ConfirmationDialog>
  );
}

/** Cancel listing?: asks first, then holds open like Buy until the outcome is known. */
export function CancelDialog({
  open,
  listing,
  operation,
  onCancelListing,
  onCheck,
  onClose,
  returnFocusRef,
}: {
  open: boolean;
  listing: Listing | null;
  operation: Operation | null;
  onCancelListing: () => void;
  onCheck: () => void;
  onClose: () => void;
  returnFocusRef: RefObject<HTMLElement | null>;
}): ReactElement | null {
  const last = useRef<Listing | null>(null);
  const current = operation?.kind === "cancel" ? operation.listing : listing;
  if (current) last.current = current;
  const shown = current ?? last.current;
  if (!shown) return null;
  const stage = operation?.kind === "cancel" ? operation.stage : null;
  const isUnconfirmed = stage?.kind === "unconfirmed";
  return (
    <ConfirmationDialog
      open={open}
      appearance="market"
      title="Cancel listing?"
      cancelLabel="Keep listing"
      confirmLabel={isUnconfirmed ? "Check status" : "Cancel listing"}
      tone={isUnconfirmed ? "gold" : "danger"}
      divider
      dismissOnBackdrop
      returnFocusRef={returnFocusRef}
      pending={stage?.kind === "pending" || (isUnconfirmed && stage.isChecking)}
      locked={isUnconfirmed}
      confirmDisabled={stage?.kind === "rejected"}
      onCancel={onClose}
      onConfirm={isUnconfirmed ? onCheck : onCancelListing}
    >
      <div className={s.dialogItem}>
        <ListingItemSummary
          item={summaryItem(shown.item, shown.quantity)}
          price={{ amount: shown.total, iconSrc: COIN }}
        />
      </div>
      <p className={s.dialogText}>
        The items will return to your available collection.
      </p>
      <StageLine
        operation={operation?.kind === "cancel" ? operation : null}
        unconfirmed="Cancellation not confirmed."
        blocked={null}
      />
    </ConfirmationDialog>
  );
}

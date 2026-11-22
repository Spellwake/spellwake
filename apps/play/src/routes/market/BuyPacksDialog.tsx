import {
  useEffect,
  useRef,
  useState,
  type ReactElement,
  type RefObject,
} from "react";
import { CoinAmount, ConfirmationDialog, QuantityControl } from "@spellwake/ui";
import {
  MAX_QUANTITY,
  quantityProblem,
  type QuoteTerms,
  type ReadySupply,
  type SupplyTile,
} from "../../market/supply-sources";
import { PackWrapper } from "../packs/PackWrapper";
import { COIN } from "./ListingDialogs";
import * as s from "./supply.css";

type Product = Extract<SupplyTile, { kind: "product" }>;

/**
 * Buy packs, as approved: a quantity from 1 to 1000 against a fresh quote of
 * its exact total. Changed terms are shown and need a new press of Buy; too
 * few remaining, too few coins or an unread price keep Buy off with the
 * reason. Buy holds the dialog with its terms until the outcome is known.
 */
export function BuyPacksDialog({
  open,
  tile,
  source,
  onClose,
  returnFocusRef,
}: {
  open: boolean;
  tile: SupplyTile | null;
  source: ReadySupply;
  onClose: () => void;
  returnFocusRef: RefObject<HTMLElement | null>;
}): ReactElement | null {
  const last = useRef<Product | null>(null);
  if (tile?.kind === "product") last.current = tile;
  const product = last.current;
  const purchase =
    source.purchase && source.purchase.tileId === product?.id
      ? source.purchase
      : null;
  const [text, setText] = useState("1");
  const problem = quantityProblem(text);
  const quantity = problem ? null : Number(text);

  // Each opening starts at one pack, or at a held purchase's quantity.
  const tileId = open ? (tile?.id ?? null) : null;
  const heldQuantity = useRef<number | null>(null);
  heldQuantity.current = purchase?.quantity ?? null;
  useEffect(() => {
    if (tileId) setText(String(heldQuantity.current ?? 1));
  }, [tileId]);

  // The quote follows the product and a valid quantity; closing stops it.
  const { quoteFor } = source;
  useEffect(() => {
    quoteFor(tileId, quantity ?? 1);
  }, [tileId, quantity, quoteFor]);

  const quote =
    source.quote && source.quote.tileId === product?.id ? source.quote : null;
  const terms: QuoteTerms | null = purchase
    ? purchase.terms
    : quote?.state === "ready" && quote.quantity === quantity
      ? quote.terms
      : null;
  // The last terms shown for this quantity, so a change of price is said and needs a new Buy.
  const shown = useRef<QuoteTerms | null>(null);
  const [changedFrom, setChangedFrom] = useState<number | null>(null);
  const [lastKnown, setLastKnown] = useState<QuoteTerms | null>(null);
  const termsKey = terms
    ? `${terms.quantity}:${terms.total}:${terms.revision}`
    : null;
  const latest = useRef(terms);
  latest.current = terms;
  useEffect(() => {
    const terms = latest.current;
    if (!termsKey || !terms) return;
    const before = shown.current;
    setChangedFrom(
      before &&
        before.quantity === terms.quantity &&
        before.total !== null &&
        terms.total !== null &&
        before.total !== terms.total
        ? before.total
        : null,
    );
    shown.current = terms;
    setLastKnown(terms);
  }, [termsKey]);
  useEffect(() => {
    shown.current = null;
    setChangedFrom(null);
    setLastKnown(null);
  }, [tileId]);

  if (!product) return null;
  const remaining = terms?.remaining ?? product.quote?.remaining ?? null;
  const ceiling = Math.min(MAX_QUANTITY, remaining ?? MAX_QUANTITY);
  const stage = purchase?.stage ?? null;
  const isUnconfirmed = stage?.kind === "unconfirmed";
  const isFrozen = stage?.kind === "pending" || isUnconfirmed;
  const soldOut = remaining === 0;
  const tooMany =
    quantity !== null &&
    remaining !== null &&
    remaining > 0 &&
    quantity > remaining;
  const isLoading = !purchase && quote?.state === "loading";
  const failed = !purchase && quote?.state === "failed";
  const shortOfCoins =
    terms?.total != null &&
    source.balance !== null &&
    terms.total > source.balance;
  const reason = isUnconfirmed
    ? "Purchase not confirmed."
    : stage?.kind === "rejected"
      ? stage.message
      : failed
        ? "Couldn’t load price."
        : shortOfCoins
          ? "Not enough available coins."
          : !source.canBuy
            ? "Buying is unavailable."
            : null;
  const step = quantity ?? (Number.isFinite(Number(text)) ? Number(text) : 1);

  return (
    <ConfirmationDialog
      open={open}
      appearance="market"
      title="Buy packs"
      cancelLabel="Cancel"
      confirmLabel={
        isUnconfirmed ? "Check status" : soldOut ? "Sold out" : "Buy"
      }
      divider
      dismissOnBackdrop
      returnFocusRef={returnFocusRef}
      pending={stage?.kind === "pending" || (isUnconfirmed && stage.isChecking)}
      locked={isUnconfirmed}
      confirmDisabled={
        !isUnconfirmed &&
        (soldOut ||
          tooMany ||
          problem !== null ||
          isLoading ||
          failed ||
          !terms ||
          terms.total === null ||
          shortOfCoins ||
          !source.canBuy ||
          stage?.kind === "rejected")
      }
      onCancel={onClose}
      onConfirm={() => {
        if (isUnconfirmed) source.checkPurchase();
        else if (terms && quantity !== null)
          source.buy(product.id, quantity, terms);
      }}
    >
      <div className={s.buyItem}>
        <span className={s.buyArt} aria-hidden="true">
          <PackWrapper setName={product.setName} product={product.fullName} />
        </span>
        <div className={s.buyCopy}>
          <p className={s.buyName}>{product.setName}</p>
          <p>{product.product}</p>
          <p>
            {remaining === null
              ? "Checking availability…"
              : `${remaining} available`}
          </p>
        </div>
      </div>
      <div className={s.row}>
        <span className={s.rowLabel}>Quantity</span>
        <div className={s.rowControl}>
          <QuantityControl
            label="Quantity"
            value={step}
            treatment="joined"
            disabled={isFrozen}
            canDecrease={step > 1}
            canIncrease={step < ceiling}
            onDecrease={() => setText(String(Math.max(1, step - 1)))}
            onIncrease={() => setText(String(Math.min(ceiling, step + 1)))}
            edit={{
              draft: text,
              onChange: setText,
              ...(problem ? { error: problem } : {}),
            }}
          />
        </div>
      </div>
      {problem && (
        <p className={s.rowError} role="alert">
          {problem}
        </p>
      )}
      {changedFrom !== null && !purchase && (
        <p className={s.changed} role="status">
          Price updated.
        </p>
      )}
      <div className={s.row}>
        <span className={s.rowLabel}>Total</span>
        <span className={s.total} aria-live="polite">
          {isLoading ? (
            <>
              <span className={s.totalNote}>Updating total…</span>
              <span className={s.smallSpinner} aria-hidden="true" />
            </>
          ) : failed && lastKnown?.total != null ? (
            <>
              <span className={s.lastKnown}>{lastKnown.total}</span>
              <span className={s.totalNote}>Last known</span>
            </>
          ) : terms?.total != null && !tooMany && !problem ? (
            <>
              {changedFrom !== null && !purchase && (
                <del className={s.struck}>{changedFrom}</del>
              )}
              <CoinAmount amount={terms.total} iconSrc={COIN} />
            </>
          ) : (
            "—"
          )}
        </span>
      </div>
      {tooMany && remaining !== null && (
        <p className={s.rowError} role="alert">
          Only {remaining} available.
        </p>
      )}
      {reason && (
        <p
          className={s.stageLine}
          data-tone={isUnconfirmed ? "unknown" : "known"}
          role="status"
        >
          {reason}
        </p>
      )}
    </ConfirmationDialog>
  );
}

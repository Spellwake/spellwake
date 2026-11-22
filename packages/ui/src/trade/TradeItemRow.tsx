import type { ReactNode } from "react";
import { X } from "lucide-react";
import { ActionButton } from "../button/ActionButton";
import { AmountDigits } from "../amounts/AmountDigits";
import { QuantityControl } from "../amounts/QuantityControl";
import * as s from "./itemRow.css";
import { designPixels } from "../design-pixels";

export interface TradeItemRowProps {
  kind: "card" | "pack" | "coin";
  name: string;
  detail?: string;
  artwork: ReactNode;
  quantity: number;
  /** Supply the quantity editor only for a participant's editable offer. */
  editor?: ReactNode;
}

/** A trade entry shared by editable offers, locked trays and receipts. */
export function TradeItemRow({
  kind,
  name,
  detail,
  artwork,
  quantity,
  editor,
}: TradeItemRowProps) {
  return (
    <div className={s.container}>
      <div
        className={s.row}
        data-kind={kind}
        data-editable={editor != null || undefined}
      >
        <span aria-hidden="true" className={s.art}>
          {artwork}
        </span>
        <span className={s.identity}>
          <span>{name}</span>
          {detail && <small className={s.detail}>{detail}</small>}
        </span>
        <div className={s.trailing}>
          {editor ?? (
            <span className={s.count}>
              × <AmountDigits value={quantity} />
            </span>
          )}
        </div>
      </div>
    </div>
  );
}

export interface TradeQuantityEditorProps {
  label: string;
  quantity: number;
  available: number;
  draft: string;
  onDraftChange: (text: string) => void;
  /** Commit a whole amount. Zero removes the holding from the offer. */
  onCommit: (quantity: number) => void;
  clearLabel: string;
}
/** A blank draft allows replacement. Committing it restores the last quantity. */
export function TradeQuantityEditor({
  label,
  quantity,
  available,
  draft,
  onDraftChange,
  onCommit,
  clearLabel,
}: TradeQuantityEditorProps) {
  const cap = (amount: number) => Math.max(0, Math.min(available, amount));
  const parsed = /^\d+$/.test(draft) ? Number(draft) : quantity;
  const amount = cap(parsed);
  return (
    <div className={s.editor}>
      <QuantityControl
        label={label}
        value={quantity}
        treatment="trade"
        edit={{
          draft: draft === "" ? "" : String(amount),
          onChange: (text) => {
            if (text === "") onDraftChange("");
            else if (/^\d+$/.test(text))
              onDraftChange(String(cap(Number(text))));
          },
          onCommit: () => onCommit(amount),
          onCancel: () => onDraftChange(String(cap(quantity))),
        }}
        canDecrease={amount > 0}
        canIncrease={amount < available}
        onDecrease={() => onCommit(cap(amount - 1))}
        onIncrease={() => onCommit(cap(amount + 1))}
      />
      <ActionButton
        className={s.remove}
        aria-label={clearLabel}
        onClick={() => onCommit(0)}
      >
        <X size={designPixels(19)} aria-hidden="true" />
      </ActionButton>
    </div>
  );
}

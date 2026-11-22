import { Minus, Plus, Trash2 } from "lucide-react";
import { useEffect, useRef } from "react";
import { AmountDigits } from "./AmountDigits";
import * as s from "./amounts.css";

interface QuantityEdit {
  draft: string;
  onChange: (text: string) => void;
  onCommit?: () => void;
  onCancel?: () => void;
  error?: string;
  descriptionId?: string;
}
export interface QuantityControlProps {
  label: string;
  value: number | string;
  onDecrease: () => void;
  onIncrease: () => void;
  canDecrease: boolean;
  canIncrease: boolean;
  disabled?: boolean;
  removeOnDecrease?: boolean;
  treatment?: "separate" | "joined" | "pill" | "ivory-pill" | "trade";
  edit?: QuantityEdit;
}
/** The owner sets the legal actions, including damage transfers and removing a last copy. */
export function QuantityControl({
  label,
  value,
  onDecrease,
  onIncrease,
  canDecrease,
  canIncrease,
  disabled = false,
  removeOnDecrease = false,
  treatment = "separate",
  edit,
}: QuantityControlProps) {
  const ivory = treatment === "ivory-pill";
  const pill = treatment === "pill" || ivory,
    joined = treatment === "joined" || treatment === "trade";
  const cell = [s.cell, pill ? s.pillCell : joined ? s.joinedCell : ""].join(
    " ",
  );
  const input = useRef<HTMLInputElement>(null);
  useEffect(() => {
    input.current?.setCustomValidity(edit?.error ?? "");
  }, [edit?.error]);
  const DecreaseIcon = removeOnDecrease ? Trash2 : Minus;
  const valueClass = [cell, s.center, joined ? s.joinedValue : ""].join(" ");
  return (
    <div
      role="group"
      aria-label={label}
      className={[
        s.group,
        pill ? s.pill : joined ? s.joined : "",
        ivory ? s.ivory : "",
        treatment === "trade" ? s.trade : "",
      ].join(" ")}
    >
      <button
        className={[cell, s.button].join(" ")}
        type="button"
        aria-label={`${removeOnDecrease ? "Remove" : "Decrease"} ${label}`}
        disabled={disabled || !canDecrease}
        onClick={onDecrease}
      >
        <DecreaseIcon aria-hidden className={s.glyph} strokeWidth={1.6} />
      </button>
      {edit ? (
        <input
          ref={input}
          className={[valueClass, s.input].join(" ")}
          aria-label={label}
          aria-invalid={edit.error ? true : undefined}
          aria-describedby={edit.descriptionId}
          disabled={disabled}
          type="text"
          inputMode="numeric"
          value={edit.draft}
          onChange={(e) => edit.onChange(e.currentTarget.value)}
          onBlur={edit.onCommit}
          onKeyDown={(e) => {
            if (e.key === "Escape" && edit.onCancel) {
              e.preventDefault();
              edit.onCancel();
              return;
            }
            if (e.key === "Enter" && !e.nativeEvent.isComposing) {
              e.preventDefault();
              e.currentTarget.blur();
            }
          }}
        />
      ) : (
        <span className={valueClass}>
          <AmountDigits value={value} />
        </span>
      )}
      <button
        className={[
          cell,
          s.button,
          treatment === "separate" ? s.increment : "",
        ].join(" ")}
        type="button"
        aria-label={`Increase ${label}`}
        disabled={disabled || !canIncrease}
        onClick={onIncrease}
      >
        <Plus
          aria-hidden
          className={[
            s.glyph,
            treatment === "separate" ? s.incrementGlyph : "",
          ].join(" ")}
          strokeWidth={1.6}
        />
      </button>
    </div>
  );
}

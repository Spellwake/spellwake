import { useRef, useState, type ReactElement } from "react";
import { Minus, Plus, Trash2 } from "lucide-react";
import * as s from "../controls.css";
import { IconButton } from "../icon-button/IconButton";

/** Quantity reserves its width during editing; deleting the final copy is explicit. */
export function Quantity({
  value,
  onValueChange,
  onRemove,
  max = 999,
  label = "Copies in deck",
}: {
  value: number;
  onValueChange: (value: number) => void;
  onRemove: () => void;
  max?: number;
  label?: string;
}): ReactElement {
  const [draft, setDraft] = useState(String(value));
  const [editing, setEditing] = useState(false);
  const skipBlur = useRef(false);
  function commit(): void {
    const parsed = Number(draft);
    if (Number.isInteger(parsed) && parsed >= 1 && parsed <= max)
      onValueChange(parsed);
    setEditing(false);
  }
  return (
    <div className={s.stepper} role="group" aria-label={label}>
      <IconButton
        label={value <= 1 ? "Remove from deck" : "Remove one copy"}
        variant="ghost"
        onClick={() => (value <= 1 ? onRemove() : onValueChange(value - 1))}
      >
        {value <= 1 ? <Trash2 size={18} /> : <Minus size={18} />}
      </IconButton>
      <input
        className={s.quantity}
        type="text"
        inputMode="numeric"
        pattern="[0-9]*"
        aria-label="Set quantity"
        value={editing ? draft : value}
        onFocus={() => {
          setDraft(String(value));
          setEditing(true);
        }}
        onChange={(event) => setDraft(event.target.value)}
        onBlur={() => {
          if (skipBlur.current) {
            skipBlur.current = false;
            return;
          }
          commit();
        }}
        onKeyDown={(event) => {
          if (event.key === "Enter") {
            commit();
            skipBlur.current = true;
            event.currentTarget.blur();
          }
          if (event.key === "Escape") {
            setDraft(String(value));
            setEditing(false);
            skipBlur.current = true;
            event.currentTarget.blur();
          }
        }}
      />
      <IconButton
        label="Add one copy"
        variant="ghost"
        disabled={value >= max}
        onClick={() => onValueChange(value + 1)}
      >
        <Plus size={18} />
      </IconButton>
    </div>
  );
}

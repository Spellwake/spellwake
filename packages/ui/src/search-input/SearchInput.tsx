import { useRef, type ReactElement } from "react";
import { Search, X } from "lucide-react";
import * as s from "../controls.css";
import { IconButton } from "../icon-button/IconButton";

/** Clear remains transparent in every pointer state and returns focus to search. */
export function SearchInput({
  value,
  onValueChange,
  label = "Search decks",
  placeholder = "Search decks…",
}: {
  value: string;
  onValueChange: (value: string) => void;
  label?: string;
  placeholder?: string;
}): ReactElement {
  const ref = useRef<HTMLInputElement>(null);
  return (
    <div className={s.inputShell}>
      <Search size={21} aria-hidden />
      <input
        ref={ref}
        className={s.input}
        type="search"
        aria-label={label}
        value={value}
        placeholder={placeholder}
        onChange={(event) => onValueChange(event.target.value)}
      />
      {value && (
        <IconButton
          variant="transparent"
          label="Clear search"
          onClick={() => {
            onValueChange("");
            ref.current?.focus();
          }}
        >
          <X size={18} />
        </IconButton>
      )}
    </div>
  );
}

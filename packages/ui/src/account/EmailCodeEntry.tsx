import { useId, useRef, useState, type Ref } from "react";
import { motion } from "motion/react";
import { ActionButton } from "../button/ActionButton";
import { controlMotion } from "../control-motion";
import { useAllowsControlMotion } from "../useAllowsControlMotion";
import * as s from "./account.css";

const length = 6;
export interface EmailCodeEntryProps {
  value: string;
  onChange: (value: string) => void;
  onVerify: (code: string) => void;
  onResend: () => void;
  /** Seconds remaining in the host's resend window. */
  resendIn: number;
  pending?: boolean;
  error?: string;
  inputRef?: Ref<HTMLInputElement>;
  visibleLabel?: boolean;
}
/** One native input preserves paste, selection, leading zeroes and autofill across six visual slots. */
export function EmailCodeEntry({
  value,
  onChange,
  onVerify,
  onResend,
  resendIn,
  pending = false,
  error,
  inputRef,
  visibleLabel = false,
}: EmailCodeEntryProps) {
  const id = useId(),
    errorId = useId();
  const [focused, setFocused] = useState(false);
  const [selection, setSelection] = useState({ start: 0, end: 0 });
  const slots = useRef<HTMLDivElement>(null);
  const animate = useAllowsControlMotion();
  const wait = Math.max(0, Math.ceil(resendIn));
  function syncSelection(input: HTMLInputElement) {
    setSelection({
      start: input.selectionStart ?? 0,
      end: input.selectionEnd ?? 0,
    });
  }
  return (
    <form
      className={s.codeForm}
      onSubmit={(event) => {
        event.preventDefault();
        if (!pending && value.length === length) onVerify(value);
      }}
    >
      <label htmlFor={id} className={visibleLabel ? s.label : s.hidden}>
        Verification code
      </label>
      <div className={s.inputWrap}>
        <div className={s.slots} ref={slots} aria-hidden="true">
          {Array.from({ length }, (_, i) => {
            const selected =
              focused && i >= selection.start && i < selection.end;
            const active =
              focused && i === Math.min(selection.start, length - 1);
            return (
              <span
                key={i}
                className={s.slot}
                data-active={active}
                data-selected={selected}
                data-invalid={Boolean(error)}
              >
                <motion.span
                  key={value[i] ?? ""}
                  initial={animate && value[i] ? { y: 2, opacity: 0.5 } : false}
                  animate={{ y: 0, opacity: 1 }}
                  transition={{
                    duration: animate ? controlMotion.feedback : 0,
                    ease: controlMotion.ease,
                  }}
                >
                  {value[i]}
                </motion.span>
                {active && error && !value[i] && <span className={s.caret} />}
              </span>
            );
          })}
        </div>
        <input
          id={id}
          ref={inputRef}
          className={s.native}
          type="text"
          inputMode="numeric"
          autoComplete="one-time-code"
          pattern="[0-9]{6}"
          spellCheck={false}
          autoCapitalize="none"
          value={value}
          disabled={pending}
          aria-invalid={Boolean(error)}
          aria-describedby={error ? errorId : undefined}
          onChange={(event) => {
            const digits = event.currentTarget.value
              .replace(/[^0-9]/g, "")
              .slice(0, length);
            if (digits === value) return;
            onChange(digits);
            if (digits.length === length) onVerify(digits);
          }}
          onFocus={(event) => {
            setFocused(true);
            syncSelection(event.currentTarget);
          }}
          onBlur={() => setFocused(false)}
          onSelect={(event) => syncSelection(event.currentTarget)}
          onClick={(event) => {
            if (event.detail !== 1 || !slots.current) return;
            const input = event.currentTarget;
            if (input.selectionStart !== input.selectionEnd) return;
            const cells = Array.from(slots.current.children);
            const index = cells.findIndex(
              (cell) => event.clientX < cell.getBoundingClientRect().right,
            );
            const digit = Math.min(index === -1 ? length : index, value.length);
            input.setSelectionRange(digit, Math.min(digit + 1, value.length));
            syncSelection(input);
          }}
        />
      </div>
      {error && (
        <p className={s.error} id={errorId} role="alert">
          {error}
        </p>
      )}
      <ActionButton
        type="submit"
        className={s.verify}
        disabled={pending || value.length !== length}
        aria-busy={pending}
        data-pending={pending}
      >
        Verify
      </ActionButton>
      <div className={s.resend}>
        Didn’t get it?{" "}
        <button type="button" disabled={pending || wait > 0} onClick={onResend}>
          {wait > 0 ? `Resend in ${wait}s` : "Resend"}
        </button>
      </div>
    </form>
  );
}

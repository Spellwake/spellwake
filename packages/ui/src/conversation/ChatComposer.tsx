import { Ban, Send } from "lucide-react";
import { motion } from "motion/react";
import { useId, type Ref } from "react";
import { controlMotion } from "../control-motion";
import { useAllowsControlMotion } from "../useAllowsControlMotion";
import * as s from "./conversation.css";
import { designPixels } from "../design-pixels";

type Draft = {
  surface: "table" | "direct";
  value: string;
  placeholder: string;
  inputRef?: Ref<HTMLTextAreaElement>;
};
export type ChatComposerProps =
  | { mode: "watching" }
  | (Draft & { mode: "blocked" })
  | (Draft & {
      mode: "editable";
      pending?: boolean;
      maxCharacters: number;
      onChange: (draft: string) => void;
      onSend: (draft: string) => void;
    });

/** The owner keeps per-conversation drafts and clears the matching draft on acceptance. */
export function ChatComposer(props: ChatComposerProps) {
  const animate = useAllowsControlMotion(),
    hint = useId();
  if (props.mode === "watching")
    return <p className={s.readOnly}>Chat is read-only.</p>;
  const editable = props.mode === "editable";
  const overLimit =
    editable && Array.from(props.value).length > props.maxCharacters;
  const canSend =
    editable && !props.pending && !overLimit && props.value.trim().length > 0;
  const table = props.surface === "table";
  return (
    <div className={s.composer} data-surface={props.surface}>
      {!editable && (
        <p className={s.blocked}>
          <Ban size={designPixels(17)} aria-hidden />
          Player blocked.
        </p>
      )}
      <form
        className={s.field}
        data-blocked={!editable}
        onSubmit={(e) => {
          e.preventDefault();
          if (canSend) props.onSend(props.value);
        }}
      >
        <textarea
          rows={1}
          aria-label={props.placeholder.replace(/(?:…|\.{3})$/, "")}
          placeholder={props.placeholder}
          ref={props.inputRef}
          value={props.value}
          readOnly={!editable}
          aria-invalid={overLimit || undefined}
          aria-describedby={overLimit ? hint : undefined}
          onChange={(e) => {
            if (editable) props.onChange(e.currentTarget.value);
          }}
          onKeyDown={(e) => {
            if (e.key !== "Enter" || e.shiftKey) return;
            if (e.nativeEvent.isComposing || e.nativeEvent.keyCode === 229)
              return;
            e.preventDefault();
            if (canSend && !e.repeat) e.currentTarget.form?.requestSubmit();
          }}
        />
        <button
          type="submit"
          aria-label="Send message"
          disabled={!canSend}
          aria-busy={(editable && props.pending) || undefined}
        >
          <motion.span
            initial={false}
            animate={{ opacity: canSend ? 1 : 0.48 }}
            transition={{ duration: animate ? controlMotion.feedback : 0 }}
          >
            {table ? (
              <span className={s.tableArrow} aria-hidden="true" />
            ) : (
              <Send aria-hidden size={designPixels(24)} strokeWidth={1.7} />
            )}
          </motion.span>
        </button>
      </form>
      {overLimit && (
        <p id={hint} role="status" className={s.limit}>
          Message exceeds {props.maxCharacters.toLocaleString()} characters.
        </p>
      )}
    </div>
  );
}

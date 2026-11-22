import {
  useEffect,
  useId,
  useLayoutEffect,
  useRef,
  useState,
  type CSSProperties,
  type RefObject,
} from "react";
import { AnimatePresence, motion, useIsPresent } from "motion/react";
import { CircleCheck, CircleAlert, X } from "lucide-react";
import { controlMotion } from "../control-motion";
import { useAllowsControlMotion } from "../useAllowsControlMotion";
import * as s from "./feedback.css";

export interface FeedbackItem {
  id: string;
  text: string;
  tone?: "neutral" | "success" | "warning";
  onDismiss?: () => void;
}
export interface FeedbackMessagesProps {
  label: string;
  items: readonly FeedbackItem[];
  presentation?: "toast" | "inline";
  returnFocusRef?: RefObject<HTMLElement | null>;
}
function Message({
  item,
  presentation,
  dismiss,
}: {
  item: FeedbackItem;
  presentation: NonNullable<FeedbackMessagesProps["presentation"]>;
  dismiss: (button: HTMLButtonElement) => void;
}) {
  const animate = useAllowsControlMotion(),
    present = useIsPresent(),
    toast = presentation === "toast";
  const Icon =
    item.tone === "success"
      ? CircleCheck
      : item.tone === "warning"
        ? CircleAlert
        : null;
  const classes = toast
    ? [
        s.toast,
        item.tone === "success"
          ? s.success
          : item.tone === "warning"
            ? s.warning
            : "",
        item.onDismiss ? s.dismissible : "",
      ]
    : [s.quiet, s.inline];
  return (
    <motion.div
      className={[s.message, ...classes].join(" ")}
      aria-hidden={!present || undefined}
      inert={!present}
      initial={animate ? { opacity: 0, y: toast ? 4 : 0 } : false}
      animate={{ opacity: 1, y: 0 }}
      exit={{ opacity: 0 }}
      transition={{
        duration: animate ? controlMotion.feedback : 0,
        ease: controlMotion.ease,
      }}
    >
      {toast && Icon && <Icon className={s.icon} aria-hidden />}
      <span>{item.text}</span>
      {item.onDismiss && (
        <button
          className={s.close}
          aria-label={`Dismiss ${item.text}`}
          type="button"
          disabled={!present}
          onClick={(e) => dismiss(e.currentTarget)}
        >
          <X size="1em" strokeWidth={1.4} aria-hidden />
        </button>
      )}
    </motion.div>
  );
}
/** Controlled notices. The caller owns each notice's identity, lifetime, and dismissal. */
export function FeedbackMessages({
  label,
  items,
  presentation = "toast",
  returnFocusRef,
}: FeedbackMessagesProps) {
  const host = useRef<HTMLDivElement>(null),
    region = useRef<HTMLDivElement>(null);
  const anchor = "--feedback-" + useId().replace(/[^a-zA-Z0-9_-]/g, "");
  const toast = presentation === "toast";
  const [announcement, setAnnouncement] = useState("");
  const announced = useRef(new Map<string, string>());
  useLayoutEffect(() => {
    const el = region.current;
    if (
      !toast ||
      !el ||
      !("showPopover" in el) ||
      !CSS.supports("anchor-name", "--test") ||
      !CSS.supports("width", "anchor-size(width)")
    )
      return;
    el.popover = "manual";
    el.showPopover();
    return () => {
      el.hidePopover();
      el.removeAttribute("popover");
    };
  }, [toast]);
  useEffect(() => {
    const fresh = items.filter(
      (item) => announced.current.get(item.id) !== item.text,
    );
    announced.current = new Map(items.map((item) => [item.id, item.text]));
    const text = fresh.map((item) => item.text).join(". ");
    if (!text) {
      if (items.length === 0) setAnnouncement("");
      return;
    }
    // A dialog opened after this host sits above it and would hide a new notice
    // behind its backdrop. Showing the host again lifts it back to the top.
    // Focus inside a notice is left alone, since hiding would move it.
    const el = region.current;
    if (
      toast &&
      el?.matches(":popover-open") &&
      document.querySelector("dialog:modal") &&
      !el.contains(document.activeElement)
    ) {
      el.hidePopover();
      el.showPopover();
    }
    const doc = document as Document & { ariaNotify?: (text: string) => void };
    if (doc.ariaNotify) doc.ariaNotify(text);
    else setAnnouncement(text);
  }, [items, toast]);
  function dismiss(item: FeedbackItem, button: HTMLButtonElement) {
    if (button === document.activeElement) {
      const buttons = Array.from(
        region.current?.querySelectorAll<HTMLButtonElement>("button:enabled") ??
          [],
      );
      const index = buttons.indexOf(button);
      (
        buttons[index + 1] ??
        buttons[index - 1] ??
        returnFocusRef?.current ??
        host.current
      )?.focus();
    }
    item.onDismiss?.();
  }
  return (
    <div
      className={[s.host, s.theme].join(" ")}
      ref={host}
      tabIndex={-1}
      style={{ anchorName: anchor } as CSSProperties}
    >
      <div
        role="region"
        aria-label={label}
        ref={region}
        className={[s.group, toast ? s.floating : ""].join(" ")}
        style={{ positionAnchor: anchor } as CSSProperties}
      >
        <AnimatePresence>
          {items.map((item) => (
            <Message
              key={item.id}
              item={item}
              presentation={presentation}
              dismiss={(button) => dismiss(item, button)}
            />
          ))}
        </AnimatePresence>
      </div>
      <span role="status" aria-atomic="true" className={s.srOnly}>
        {announcement}
      </span>
    </div>
  );
}

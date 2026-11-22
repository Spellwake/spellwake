import { useEffect, useId, useRef, type ReactElement } from "react";
import { X } from "lucide-react";
import { ActionButton } from "@spellwake/ui";
import * as s from "./cards.css";

/**
 * The filter panel over the inspector. It applies as it changes, so it has no
 * Apply or Cancel: X, Escape or a press outside it closes it and keeps them.
 */
export function FilterPanel({
  children,
  onClose,
  onReset,
}: {
  children: ReactElement;
  onClose: () => void;
  onReset: () => void;
}): ReactElement {
  const panel = useRef<HTMLDivElement>(null);
  const titleId = useId();
  const close = useRef(onClose);
  close.current = onClose;
  // Focus starts on the panel's heading once, when it opens.
  useEffect(() => {
    panel.current?.querySelector<HTMLElement>("h2")?.focus();
  }, []);
  useEffect(() => {
    const onClose = () => close.current();
    const onKey = (event: KeyboardEvent) => {
      if (event.key === "Escape") onClose();
    };
    const onPress = (event: PointerEvent) => {
      const target = event.target as Element | null;
      // Menus open from the panel render elsewhere; pressing in them is not outside.
      if (
        target &&
        !panel.current?.contains(target) &&
        !target.closest(
          "[data-radix-popper-content-wrapper], [role=listbox]",
        ) &&
        !target.closest("[aria-haspopup=dialog]")
      )
        onClose();
    };
    document.addEventListener("keydown", onKey);
    document.addEventListener("pointerdown", onPress);
    return () => {
      document.removeEventListener("keydown", onKey);
      document.removeEventListener("pointerdown", onPress);
    };
  }, []);
  return (
    <div
      ref={panel}
      className={s.panel}
      role="dialog"
      aria-modal="false"
      aria-labelledby={titleId}
    >
      <div className={s.panelHead}>
        <h2 className={s.panelTitle} id={titleId} tabIndex={-1}>
          Filters
        </h2>
        <button
          type="button"
          className={s.close}
          aria-label="Close filters"
          onClick={onClose}
        >
          <X aria-hidden="true" />
        </button>
      </div>
      <div className={s.panelScroll}>{children}</div>
      <div className={s.panelFoot}>
        <ActionButton treatment="compact-form" onClick={onReset}>
          Reset filters
        </ActionButton>
      </div>
    </div>
  );
}

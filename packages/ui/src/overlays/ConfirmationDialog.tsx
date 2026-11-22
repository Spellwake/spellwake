import type { ReactNode, RefObject } from "react";
import { X } from "lucide-react";
import { motion, useAnimationControls } from "motion/react";
import { useId, useLayoutEffect, useRef, type PointerEvent } from "react";
import { ActionButton } from "../button/ActionButton";
import { controlMotion } from "../control-motion";
import { useAllowsControlMotion } from "../useAllowsControlMotion";
import { Dialog, useOverlayPresentation } from "./Dialog";
import * as s from "./confirmation.css";

export interface ConfirmationActionsProps {
  cancelLabel: string;
  confirmLabel: string;
  tone?: "gold" | "danger";
  pending?: boolean;
  /**
   * Holds the dialog open with its terms while an outcome is unknown: the
   * secondary action, close, Escape and a press outside do nothing. The
   * primary action stays available, as Check status.
   */
  locked?: boolean;
  /** Disables the primary action, as when a known reason prevents it. */
  confirmDisabled?: boolean;
  onCancel: () => void;
  onConfirm: () => void;
}

/** Secondary action first in both the visual and keyboard order. */
export function ConfirmationActions({
  cancelLabel,
  confirmLabel,
  tone = "gold",
  pending = false,
  locked = false,
  confirmDisabled = false,
  onCancel,
  onConfirm,
}: ConfirmationActionsProps) {
  return (
    <footer className={s.actions} data-tone={tone}>
      <ActionButton
        data-cancel=""
        disabled={pending || locked}
        onClick={onCancel}
      >
        {cancelLabel}
      </ActionButton>
      <ActionButton
        pending={pending}
        disabled={confirmDisabled}
        onClick={onConfirm}
      >
        {confirmLabel}
      </ActionButton>
    </footer>
  );
}

export interface ConfirmationDialogProps extends ConfirmationActionsProps {
  open: boolean;
  title: string;
  description?: string;
  children?: ReactNode;
  appearance: "social" | "lobby" | "trade" | "match" | "market";
  compact?: boolean;
  /** Wide trade decision with the dividing rule above its actions. */
  wide?: boolean;
  divider?: boolean;
  dismissOnBackdrop?: boolean;
  /** Explicit return target for menu-launched dialogs and pointer invokers in WebKit. */
  returnFocusRef?: RefObject<HTMLElement | null>;
}

function outside(event: PointerEvent<HTMLDialogElement>) {
  const box = event.currentTarget.getBoundingClientRect();
  return (
    event.clientX < box.left ||
    event.clientX > box.right ||
    event.clientY < box.top ||
    event.clientY > box.bottom
  );
}

/** Mount across open and close transitions. The owner runs the confirmed command and supplies its pending state. */
export function ConfirmationDialog({
  open,
  title,
  description,
  children,
  appearance,
  compact = false,
  wide = false,
  divider = false,
  dismissOnBackdrop = false,
  returnFocusRef,
  ...actions
}: ConfirmationDialogProps) {
  const ref = useRef<HTMLDialogElement>(null),
    titleId = useId(),
    detailId = useId();
  const pointer = useRef<number | null>(null);
  const presentation = useOverlayPresentation();
  const allowMotion = useAllowsControlMotion(),
    animation = useAnimationControls();
  useLayoutEffect(() => {
    const node = ref.current;
    if (!node) return;
    let current = true;
    if (open) {
      if (!node.open) {
        node.showModal();
        node
          .querySelector<HTMLButtonElement>("[data-cancel]:enabled")
          ?.focus({ preventScroll: true });
        if (allowMotion) animation.set({ opacity: 0, scale: 0.985 });
      }
      void animation.start({
        opacity: 1,
        scale: 1,
        transition: {
          duration: allowMotion ? controlMotion.settle : 0,
          ease: controlMotion.ease,
        },
      });
    } else if (node.open) {
      pointer.current = null;
      const finishClose = () => {
        if (!current || !node.open) return;
        node.close();
        if (returnFocusRef?.current?.isConnected)
          returnFocusRef.current.focus({ preventScroll: true });
      };
      if (!allowMotion) finishClose();
      else
        void animation
          .start({
            opacity: 0,
            scale: 0.985,
            transition: {
              duration: controlMotion.feedback,
              ease: controlMotion.ease,
            },
          })
          .then(finishClose);
    }
    return () => {
      current = false;
      animation.stop();
    };
  }, [open, allowMotion, animation, returnFocusRef]);

  function dismiss() {
    if (open && !actions.pending && !actions.locked) actions.onCancel();
  }
  // On a phone a confirmation is a drawer like every other dialog.
  if (presentation === "drawer")
    return (
      <Dialog
        open={open}
        title={title}
        size="compact"
        busy={(actions.pending ?? false) || (actions.locked ?? false)}
        onClose={dismiss}
        {...(returnFocusRef ? { returnFocusRef } : {})}
      >
        {description && <p className={s.description}>{description}</p>}
        {children}
        <ConfirmationActions {...actions} />
      </Dialog>
    );
  return (
    <motion.dialog
      ref={ref}
      className={s.dialog}
      data-appearance={appearance}
      data-compact={compact}
      data-wide={wide || undefined}
      data-divider={divider}
      aria-labelledby={titleId}
      aria-describedby={description ? detailId : undefined}
      aria-busy={actions.pending || undefined}
      initial={false}
      animate={animation}
      onCancel={(event) => {
        event.preventDefault();
        dismiss();
      }}
      onPointerDown={(event) => {
        pointer.current =
          dismissOnBackdrop &&
          event.isPrimary &&
          event.button === 0 &&
          outside(event)
            ? event.pointerId
            : null;
      }}
      onPointerCancel={() => {
        pointer.current = null;
      }}
      onPointerUp={(event) => {
        const shouldDismiss =
          pointer.current === event.pointerId && outside(event);
        pointer.current = null;
        if (shouldDismiss) dismiss();
      }}
    >
      <div className={s.body} inert={!open}>
        <h2 className={s.title} id={titleId}>
          {title}
        </h2>
        {description && (
          <p className={s.description} id={detailId}>
            {description}
          </p>
        )}
        {children}
        <ConfirmationActions {...actions} />
        <button
          type="button"
          aria-label="Close"
          className={s.close}
          disabled={actions.pending || actions.locked || !open}
          onClick={dismiss}
        >
          <X aria-hidden="true" strokeWidth={1.5} />
        </button>
      </div>
    </motion.dialog>
  );
}

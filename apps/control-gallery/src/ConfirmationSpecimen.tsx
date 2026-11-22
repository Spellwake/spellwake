import { useEffect, useRef, useState } from "react";
import {
  ActionButton,
  ConfirmationDialog,
  type ConfirmationDialogProps,
} from "@spellwake/ui";
import { references } from "./references";
import type { ControlUse } from "./UsageMap";

export const editTradeDecision = {
  appearance: "trade",
  wide: true,
  title: "Edit items?",
  description: "Both players become Not ready and release reserved items.",
  cancelLabel: "Keep ready",
  confirmLabel: "Edit items",
} as const satisfies Omit<
  ConfirmationDialogProps,
  "open" | "onCancel" | "onConfirm"
>;
export const cancelTradeDecision = {
  appearance: "trade",
  title: "Cancel trade?",
  description: "Trade will close for both players.",
  cancelLabel: "Keep trading",
  confirmLabel: "Cancel trade",
  tone: "danger",
  divider: true,
} as const satisfies Omit<
  ConfirmationDialogProps,
  "open" | "onCancel" | "onConfirm"
>;
const presets = {
  "TR10:edit-confirm": { ...editTradeDecision, result: "Items are editable." },
  "MSG07:confirm": {
    appearance: "social",
    title: "Remove Mira from friends?",
    cancelLabel: "Cancel",
    confirmLabel: "Remove friend",
    result: "Mira removed from friends.",
    dismissOnBackdrop: true,
  },
  "MSG08:confirm": {
    appearance: "social",
    compact: true,
    divider: true,
    tone: "danger",
    title: "Block Mira?",
    cancelLabel: "Cancel",
    confirmLabel: "Block",
    result: "Mira blocked.",
    dismissOnBackdrop: true,
  },
  "TR07:confirm": { ...cancelTradeDecision, result: "Trade canceled." },
  "LO03:confirm": {
    appearance: "lobby",
    compact: true,
    title: "Leave lobby?",
    description: "Only you will leave. The lobby stays open.",
    cancelLabel: "Stay",
    confirmLabel: "Leave",
    result: "You left the lobby.",
  },
  "LO02:confirm": {
    appearance: "lobby",
    title: "Close lobby?",
    description: "This lobby will close for everyone.",
    cancelLabel: "Cancel",
    confirmLabel: "Close lobby",
    result: "Lobby closed.",
  },
  "CM08:confirm": {
    appearance: "match",
    title: "Concede?",
    dismissOnBackdrop: true,
    description: "You can stay and watch.",
    cancelLabel: "Cancel",
    confirmLabel: "Concede",
    tone: "danger",
    result: "Watching the game.",
  },
} as const satisfies Record<
  string,
  Omit<ConfirmationDialogProps, "open" | "onCancel" | "onConfirm"> & {
    result: string;
  }
>;
type ConfirmView = keyof typeof presets;
export function isConfirmationView(view: string | null): view is ConfirmView {
  return view !== null && Object.hasOwn(presets, view);
}
const screens = Object.keys(presets) as ConfirmView[];
export const confirmationUses: ControlUse[] = [
  {
    id: "dialog",
    mapSection: "component",
    name: "Confirmation dialog",
    description:
      "The same titled modal holds the decision in messaging, trade, the lobby and the match.",
    screens: [
      ...screens,
      "TR04:locked-tray",
      "TR05:locked-tray",
      "TR06:locked-tray",
    ],
  },
  {
    id: "button",
    mapSection: "component",
    name: "Confirmation actions",
    description:
      "The safe action comes first. Both decisions reuse the shared action button and lock during submission.",
    screens,
  },
];

/** A separate document keeps the real modal and its backdrop inside the comparison viewport. */
export function ConfirmationSpecimen({ view }: { view: ConfirmView }) {
  const [, , width, height] = references[view].crop;
  return (
    <div className="popup-stage">
      <iframe
        key={view}
        className="confirm-preview"
        data-specimen
        style={{ width, height }}
        title={references[view].state}
        src={`?confirmation=${encodeURIComponent(view)}`}
      />
    </div>
  );
}

export function ConfirmationExample({ view }: { view: ConfirmView }) {
  const { result, ...preset } = presets[view];
  const [open, setOpen] = useState(false),
    [pending, setPending] = useState(false),
    [status, setStatus] = useState("");
  const opener = useRef<HTMLButtonElement>(null),
    timer = useRef<number | undefined>(undefined);
  useEffect(() => {
    opener.current?.focus({ preventScroll: true });
    setOpen(true);
    return () => window.clearTimeout(timer.current);
  }, []);
  function confirm() {
    if (timer.current !== undefined) return;
    setPending(true);
    timer.current = window.setTimeout(() => {
      timer.current = undefined;
      setStatus(result);
      setPending(false);
      setOpen(false);
    }, 480);
  }
  return (
    <div className="confirmation-example">
      <p role="status">{status}</p>
      <ActionButton
        ref={opener}
        onClick={() => {
          setStatus("");
          setOpen(true);
        }}
      >
        Open confirmation
      </ActionButton>
      <ConfirmationDialog
        {...preset}
        returnFocusRef={opener}
        open={open}
        pending={pending}
        onCancel={() => {
          setStatus(
            view === "TR10:edit-confirm"
              ? "Offer kept ready."
              : view === "CM08:confirm"
                ? "Returned to settings."
                : "Canceled.",
          );
          setOpen(false);
        }}
        onConfirm={confirm}
      />
    </div>
  );
}

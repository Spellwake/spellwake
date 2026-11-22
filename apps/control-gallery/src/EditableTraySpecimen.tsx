import { useEffect, useRef, useState, type ReactNode } from "react";
import {
  ConfirmationDialog,
  TradeReadinessBadge,
  type TradeParticipantStatus,
  TradeItemPicker,
  TradeQuantityEditor,
  TradeTray,
} from "@spellwake/ui";
import { ReferenceArtwork } from "./ReferenceArtwork";
import { TradeActionPanel, type ActionView } from "./TradeActionPanel";
import { editTradeDecision } from "./ConfirmationSpecimen";
import {
  editableViews,
  lockedViews,
  exampleHoldings,
  type EditableView,
  type Holding,
} from "./trade-offer-fixtures";
export { isEditableTrayView } from "./trade-offer-fixtures";
import { references } from "./references";
import type { ControlUse } from "./UsageMap";

export const editableTrayUses: ControlUse[] = [
  {
    id: "trade-trays",
    name: "Editable trade tray",
    mapSection: "component",
    description:
      "Edit item quantities in a scrollable list, with coins in the same list and Add items beneath it.",
    screens: [...editableViews],
  },
  {
    id: "dialog",
    mapSection: "component",
    name: "Edit items confirmation",
    description: "Keep a locked offer or reopen it for editing.",
    screens: [...lockedViews, "TR10:edit-confirm"],
  },
];
export function EditableTraySpecimen({
  view,
  actionsView,
  room,
  onComplete,
  onCanceled,
}: {
  onCanceled: () => void;
  view: EditableView;
  actionsView?: ActionView;
  room?: {
    initialOverlay?: "card" | "pack" | "cancel";
    compose: (
      ownTray: ReactNode,
      actions: ReactNode,
      partner: TradeParticipantStatus,
    ) => ReactNode;
  };
  onComplete: (
    source: EditableView,
    quantities: Record<string, number>,
  ) => void;
}) {
  const dense = view === "TR11:own-tray",
    [, , width, height] = references[view].crop;
  const [holdings, setHoldings] = useState(() => exampleHoldings(view));
  const lockedExample = lockedViews.some((id) => id === view);
  const initialParticipants: {
    own: TradeParticipantStatus;
    partner: TradeParticipantStatus;
  } = {
    own:
      view === "TR06:locked-tray"
        ? "confirmed"
        : lockedExample
          ? "ready"
          : "editing",
    partner:
      view === "TR04:locked-tray" || view === "TR06:locked-tray"
        ? "ready"
        : "editing",
  };
  const [participants, setParticipants] = useState(initialParticipants);
  const [confirmEdit, setConfirmEdit] = useState(false);
  const limited = view === "TR09:availability";
  const portrait = (
    <ReferenceArtwork
      source={view}
      region={
        lockedExample
          ? [107, 238, 70, 69]
          : limited
            ? [819, 531, 64, 64]
            : view === "TR09:empty-tray"
              ? [75, 97, 66, 66]
              : [105, 234, 69, 68]
      }
    />
  );
  const addButton = useRef<HTMLButtonElement>(null);
  const [pickerOpen, setPickerOpen] = useState(
    room?.initialOverlay === "card" || room?.initialOverlay === "pack",
  );
  const [confirmCancel, setConfirmCancel] = useState(
    room?.initialOverlay === "cancel",
  );
  const [simulatePartner, setSimulatePartner] = useState(false);
  const completion = useRef<() => void>(() => {});
  useEffect(() => {
    if (!room || !simulatePartner || confirmEdit || confirmCancel) return;
    if (participants.own === "ready" && participants.partner === "editing") {
      const timer = window.setTimeout(
        () => setParticipants({ own: "ready", partner: "ready" }),
        1800,
      );
      return () => window.clearTimeout(timer);
    }
    if (participants.own === "confirmed" && participants.partner === "ready") {
      const timer = window.setTimeout(() => completion.current(), 1800);
      return () => window.clearTimeout(timer);
    }
  }, [!!room, simulatePartner, participants, confirmEdit, confirmCancel]);
  function edit(id: string, text: string) {
    setHoldings((old) =>
      old.map((row) => (row.id === id ? { ...row, draft: text } : row)),
    );
  }
  function change(id: string, next: number | "add") {
    setHoldings((old) =>
      old.map((row) => {
        if (row.id !== id) return row;
        const n =
          next === "add" ? Math.min(row.maximum, row.quantity + 1) : next;
        return {
          ...row,
          quantity: n,
          draft: String(n),
          available: row.maximum - n,
        };
      }),
    );
    if (next === 0) requestAnimationFrame(() => addButton.current?.focus());
  }
  const offered = holdings.filter((row) => row.quantity > 0);
  const amount = (row: Holding) =>
    Math.min(row.maximum, row.draft === "" ? row.quantity : Number(row.draft));
  const items = offered.map((row) => ({
    ...row,
    editor: (
      <TradeQuantityEditor
        label={`${row.name} ${row.detail ?? ""} quantity`.replace(/ +/g, " ")}
        clearLabel={`Remove ${row.name}`}
        available={row.maximum}
        quantity={row.quantity}
        draft={row.draft}
        onDraftChange={(text) => edit(row.id, text)}
        onCommit={(n) => change(row.id, n)}
      />
    ),
  }));
  function markReady() {
    setSimulatePartner(true);
    setParticipants((prev) =>
      prev.own === "editing" ? { ...prev, own: "ready" } : prev,
    );
    if (!actionsView) requestAnimationFrame(() => addButton.current?.focus());
  }
  function confirm(side: "own" | "partner") {
    setSimulatePartner(true);
    const other = side === "own" ? "partner" : "own";
    if (participants[side] !== "ready" || participants[other] === "editing")
      return;
    if (participants[other] === "confirmed") {
      onComplete(
        view,
        Object.fromEntries(offered.map((row) => [row.id, amount(row)])),
      );
    } else {
      setParticipants({ ...participants, [side]: "confirmed" });
    }
  }
  useEffect(() => {
    completion.current = () => confirm("partner");
  });
  const ownTray =
    participants.own !== "editing" ? (
      <TradeTray
        kind="locked"
        status={participants.own}
        portrait={portrait}
        items={offered.map((row) => ({
          ...row,
          quantity: amount(row),
        }))}
        editButtonRef={addButton}
        onEditItems={() => setConfirmEdit(true)}
      />
    ) : (
      <TradeTray
        kind="editable"
        compact={dense}
        portrait={portrait}
        addButtonRef={addButton}
        items={items}
        onAddItems={() => setPickerOpen(true)}
        {...(limited
          ? {
              ready: {
                disabled: !offered.some((row) => amount(row) > 0),
                onReady: markReady,
              },
            }
          : {})}
      />
    );
  const actions = (
    <TradeActionPanel
      {...(room
        ? {
            room: true,
            initialCancel: room.initialOverlay === "cancel",
            onCancelOpenChange: setConfirmCancel,
          }
        : {})}
      own={participants.own}
      partner={participants.partner}
      canReady={
        view !== "TR09:empty-tray" || offered.some((row) => amount(row) > 0)
      }
      {...(actionsView ? { view: actionsView } : {})}
      onReady={markReady}
      onTrade={() => confirm("own")}
      onCancelTrade={onCanceled}
      onPartnerAction={() => {
        if (participants.partner === "ready") confirm("partner");
        else if (participants.partner === "editing")
          setParticipants({ ...participants, partner: "ready" });
      }}
    />
  );
  return (
    <>
      {room ? (
        room.compose(ownTray, actions, participants.partner)
      ) : (
        <>
          {!actionsView && (
            <div className="popup-stage">
              <div
                tabIndex={-1}
                id="trade-offer"
                className="popup-scene tray-example"
                data-specimen
                style={{ width, height }}
              >
                {ownTray}
              </div>
            </div>
          )}
          {!limited && actions}
        </>
      )}
      {lockedExample && !room && !actionsView && (
        <div className="readout">
          <span>Mira</span>
          <TradeReadinessBadge status={participants.partner} />
        </div>
      )}
      <ConfirmationDialog
        {...editTradeDecision}
        open={confirmEdit}
        returnFocusRef={addButton}
        onCancel={() => setConfirmEdit(false)}
        onConfirm={() => {
          setConfirmEdit(false);
          setParticipants({ own: "editing", partner: "editing" });
        }}
      />
      {pickerOpen && (
        <TradeItemPicker
          initialTab={room?.initialOverlay === "pack" ? "pack" : "card"}
          returnFocusRef={addButton}
          items={holdings.map((row) => ({ ...row, added: row.quantity }))}
          onAdd={(id) => change(id, "add")}
          onClose={() => setPickerOpen(false)}
        />
      )}
    </>
  );
}

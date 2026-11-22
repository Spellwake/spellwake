import { useRef, useState } from "react";
import {
  ConfirmationDialog,
  TradeRoomActions,
  TradeStatusLine,
  tradeRoomStatus,
  type TradeParticipantStatus,
} from "@spellwake/ui";
import { cancelTradeDecision } from "./ConfirmationSpecimen";
import { references, originalUrl, type View } from "./references";
import type { ControlUse } from "./UsageMap";

export const actionScenes = {
  "TR01:actions": "TR01:own-tray",
  "TR11:actions": "TR11:own-tray",
  "TR04:actions": "TR04:locked-tray",
  "TR05:actions": "TR05:locked-tray",
  "TR06:actions": "TR06:locked-tray",
} as const;
export type ActionView = keyof typeof actionScenes;
export function isTradeActionView(view: View): view is ActionView {
  return Object.hasOwn(actionScenes, view);
}
export const tradeActionUses: ControlUse[] = [
  {
    name: "Trade room actions",
    id: "trade-consent",
    mapSection: "component",
    description:
      "The same controls mark an offer ready, confirm its terms, or open the cancel decision.",
    screens: [...(Object.keys(actionScenes) as ActionView[]), "TR07:confirm"],
  },
];

export function TradeActionPanel({
  own,
  partner,
  canReady,
  view,
  onReady,
  onTrade,
  onCancelTrade,
  onPartnerAction,
  room = false,
  initialCancel = false,
  onCancelOpenChange,
}: {
  own: TradeParticipantStatus;
  partner: TradeParticipantStatus;
  canReady: boolean;
  view?: ActionView;
  onReady: () => void;
  onTrade: () => void;
  onCancelTrade: () => void;
  onPartnerAction: () => void;
  room?: boolean;
  initialCancel?: boolean;
  onCancelOpenChange?: (open: boolean) => void;
}) {
  const [cancelOpen, setCancelOpen] = useState(initialCancel),
    cancel = useRef<HTMLButtonElement>(null);
  function toggleCancel(open: boolean) {
    setCancelOpen(open);
    onCancelOpenChange?.(open);
  }
  const status = tradeRoomStatus(own, partner);
  const actions = (
    <TradeRoomActions
      status={status}
      canReady={canReady}
      onReady={onReady}
      onTrade={onTrade}
      onCancel={() => toggleCancel(true)}
      cancelButtonRef={cancel}
    />
  );
  const line = <TradeStatusLine status={status} />;
  // Sample an adjacent strip of scenery so the controls themselves remain live.
  const [x, y] = references[view ?? "TR01:actions"].crop;
  const width = own === "editing" ? 500 : 565,
    height = own === "editing" ? 70 : 74;
  return (
    <>
      {view ? (
        <div className="popup-stage">
          <div
            data-specimen
            className="popup-scene trade-action-scene"
            style={{
              width,
              height,
              backgroundImage: `url("${originalUrl(view)}")`,
              backgroundPosition: `-${x}px -${y + 80}px`,
            }}
          >
            {actions}
          </div>
        </div>
      ) : (
        <div className="trade-room-footer">
          {line}
          {actions}
        </div>
      )}
      {view && <div className="action-example-status">{line}</div>}
      {!room &&
        (partner === "editing" ||
          (partner === "ready" && own !== "editing")) && (
          <div className="readout">
            <button
              type="button"
              disabled={!canReady}
              onClick={onPartnerAction}
            >
              {partner === "editing"
                ? "Simulate partner ready"
                : "Simulate partner confirmation"}
            </button>
          </div>
        )}
      <ConfirmationDialog
        {...cancelTradeDecision}
        open={cancelOpen}
        returnFocusRef={cancel}
        onCancel={() => toggleCancel(false)}
        onConfirm={onCancelTrade}
      />
    </>
  );
}

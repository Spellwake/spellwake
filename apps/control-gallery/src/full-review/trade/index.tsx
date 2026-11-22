import { useRef, useState } from "react";
import {
  ActionButton,
  TradeReceipt,
  TradeClosedNotice,
  TradeTray,
  FeedbackMessages,
} from "@spellwake/ui";
import { ChevronLeft, LoaderCircle } from "lucide-react";
import { ReviewHeader } from "../ReviewHeader";
import type { ScreenProps } from "../../review-types";
import { EditableTraySpecimen } from "../../EditableTraySpecimen";
import { tradeCoinSample } from "../../OwnedPickerSpecimen";
import { traySample } from "../../TradeTraySpecimen";
import type { EditableView } from "../../trade-offer-fixtures";
import {
  receiptContributions,
  receiptFromHistory,
} from "../../TradeReceiptSpecimen";
import "./trade.css";

const editing: Record<string, EditableView> = {
  "TR-D04": "TR04:locked-tray",
  "TR-D05": "TR05:locked-tray",
  "TR-D06": "TR06:locked-tray",
  "TR-D11": "TR11:own-tray",
};
function AvailabilityBoard({ go }: Pick<ScreenProps, "go">) {
  return (
    <section className="sw-trade-board">
      <h1>Trade · items and availability</h1>
      <div className="sw-trade-availability">
        <EditableTraySpecimen
          view="TR09:empty-tray"
          onComplete={() => go("TR-D08")}
          onCanceled={() => go("TR-D10")}
        />
        <iframe title="Search with no matches" src="?picker=TR09:no-matches" />
        <iframe
          title="Empty owned category"
          src="?picker=TR09:empty-category"
        />
        <EditableTraySpecimen
          view="TR09:availability"
          onComplete={() => go("TR-D08")}
          onCanceled={() => go("TR-D10")}
        />
      </div>
    </section>
  );
}
function FeedbackBoard({ go }: Pick<ScreenProps, "go">) {
  const [dismissed, setDismissed] = useState(false),
    [checking, setChecking] = useState(false);
  return (
    <section className="sw-trade-board sw-trade-feedback">
      <h1>Trade · confirmations and recovery</h1>
      <div className="sw-feedback-grid">
        <iframe
          title="Edit items confirmation"
          src="?confirmation=TR10:edit-confirm"
        />
        <article>
          <h2>
            <LoaderCircle /> Checking trade
          </h2>
          <p>We couldn’t confirm the result.</p>
          <ActionButton
            onClick={() => {
              setChecking(true);
              go("TR-D08");
            }}
            disabled={checking}
          >
            Check status
          </ActionButton>
          <hr />
          <ActionButton disabled>Trade</ActionButton>
        </article>
        <div className="sw-trade-closed-board">
          <TradeClosedNotice reason="canceled" onBack={() => go("MSG-D01")} />
          <p>Trade unavailable.</p>
        </div>
        <article>
          <h2>
            <LoaderCircle /> Reserving items…
          </h2>
          <hr />
          <ActionButton disabled>Ready</ActionButton>
          <hr />
          <FeedbackMessages
            label="Trade notifications"
            items={
              dismissed
                ? []
                : [
                    {
                      id: "rejected",
                      tone: "warning",
                      text: "Couldn’t update trade. No changes were made.",
                      onDismiss: () => setDismissed(true),
                    },
                  ]
            }
          />
        </article>
      </div>
    </section>
  );
}
export default function FullTradeScreen({ view, go, data }: ScreenProps) {
  const id = view.id,
    [canceled, setCanceled] = useState(false);
  const closed = useRef<HTMLDivElement>(null);
  if (id === "TR-D09") return <AvailabilityBoard go={go} />;
  if (id === "TR-D10") return <FeedbackBoard go={go} />;
  const sample = editing[id] ?? "TR01:own-tray";
  const partner = traySample(
    sample.endsWith("locked-tray") ? "TR04:partner-tray" : "TR01:partner-tray",
  );
  const overlay =
    id === "TR-D02"
      ? "card"
      : id === "TR-D03"
        ? "pack"
        : id === "TR-D07"
          ? "cancel"
          : undefined;
  const back = () => go("MSG-D01");
  const receipt =
    id === "TR-D08"
      ? receiptContributions(receiptFromHistory(data))
      : undefined;
  const coinsPaid =
    receipt?.gave.items.find((item) => item.kind === "coin")?.quantity ?? 0;
  const coinsReceived =
    receipt?.received.items.find((item) => item.kind === "coin")?.quantity ?? 0;
  return (
    <div className="sw-full-trade">
      <ReviewHeader
        active="Collection"
        go={go}
        coins={tradeCoinSample.available - coinsPaid + coinsReceived}
      />
      {canceled ? (
        <div className="sw-trade-closed" ref={closed} tabIndex={-1}>
          <TradeClosedNotice reason="canceled" onBack={back} />
        </div>
      ) : receipt ? (
        <div className="sw-trade-receipt">
          <TradeReceipt {...receipt} onDone={back} />
        </div>
      ) : (
        <div className="sw-trade-room">
          <div className="sw-trade-heading">
            <ActionButton icon={<ChevronLeft />} onClick={back}>
              Back
            </ActionButton>
            <h1>Trade with Mira</h1>
          </div>
          <EditableTraySpecimen
            key={id}
            view={sample}
            onCanceled={() => {
              setCanceled(true);
              requestAnimationFrame(() => closed.current?.focus());
            }}
            onComplete={(source, quantities) =>
              go("TR-D08", { source, quantities })
            }
            room={{
              ...(overlay ? { initialOverlay: overlay } : {}),
              compose: (ownTray, actions, status) => (
                <>
                  <div
                    className="sw-trade-columns"
                    data-dense={sample === "TR11:own-tray"}
                  >
                    {ownTray}
                    <TradeTray
                      kind="offer"
                      participant="Mira"
                      status={status}
                      {...partner}
                    />
                  </div>
                  {actions}
                </>
              ),
            }}
          />
        </div>
      )}
    </div>
  );
}

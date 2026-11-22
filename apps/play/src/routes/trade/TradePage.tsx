import {
  useEffect,
  useRef,
  useState,
  type ReactElement,
  type ReactNode,
} from "react";
import { ChevronLeft, CircleUserRound } from "lucide-react";
import {
  ActionButton,
  ConfirmationDialog,
  FeedbackMessages,
  TradeClosedNotice,
  TradeItemPicker,
  TradeQuantityEditor,
  TradeReceipt,
  TradeRoomActions,
  TradeStatusLine,
  TradeTray,
  tradeRoomStatus,
  type TradeTrayItem,
} from "@spellwake/ui";
import { CroppedArt } from "../../account/Portrait";
import type { Portrait } from "../../account/account-sources";
import { useDemoIdentity } from "../../account/demo-account";
import { useAccount } from "../../auth/AccountProvider";
import { CONNECTED_IDENTITY } from "../../account/connected-account";
import { PackWrapper } from "../packs/PackWrapper";
import {
  shortfalls,
  statusWords,
  canMarkReady,
  type OfferLine,
  type ReadyTrade,
  type TradeAsset,
  type TradeSource,
} from "../../trade/trade-sources";
import * as s from "./trade.css";

const COIN = `${import.meta.env.BASE_URL}coin.svg`;
const RECEIPT_HEADING = "trade-receipt-heading";
const CLOSED_HEADING = "trade-closed-heading";

const editDecision = {
  appearance: "trade",
  wide: true,
  title: "Edit items?",
  description: "Both players become Not ready and release reserved items.",
  cancelLabel: "Keep ready",
  confirmLabel: "Edit items",
} as const;
const cancelDecision = {
  appearance: "trade",
  title: "Cancel trade?",
  description: "Trade will close for both players.",
  cancelLabel: "Keep trading",
  confirmLabel: "Cancel trade",
  tone: "danger",
  divider: true,
} as const;
const PENDING_WORDS = {
  ready: "Reserving items…",
  trade: "Confirming trade…",
  edit: "Releasing items…",
  cancel: "Canceling trade…",
} as const;

/** An offered thing's picture: card artwork, a drawn pack wrapper, or the shared coin. */
function Artwork({ asset }: { asset: TradeAsset }): ReactElement {
  if (asset.kind === "coin")
    return <img className={s.coin} src={COIN} alt="" draggable={false} />;
  if (asset.kind === "pack")
    return (
      <span className={s.pack}>
        <PackWrapper
          setName={asset.setName ?? asset.name}
          product={asset.name}
        />
      </span>
    );
  return (
    <span className={s.card}>
      {asset.artUrl && (
        <img
          className={s.cardArt}
          src={asset.artUrl}
          alt=""
          loading="lazy"
          decoding="async"
          draggable={false}
        />
      )}
    </span>
  );
}
const trayItem = (line: OfferLine): TradeTrayItem => ({
  id: line.id,
  kind: line.kind,
  name: line.name,
  ...(line.detail ? { detail: line.detail } : {}),
  artwork: <Artwork asset={line} />,
  quantity: line.quantity,
});
function Face({ portrait }: { portrait: Portrait | null }): ReactNode {
  return portrait ? (
    <span className={s.face}>
      <CroppedArt {...portrait} />
    </span>
  ) : (
    <CircleUserRound className={s.blank} strokeWidth={1.1} />
  );
}
function useOwnPortrait(): Portrait | null {
  const preview = useDemoIdentity();
  return useAccount().state.status === "unconfigured"
    ? preview.portrait
    : CONNECTED_IDENTITY.portrait;
}

/**
 * The trade room with one person, as approved: your offer on the left, theirs
 * on the right, the shared status and actions beneath. Only your side is
 * editable, and only until you are Ready. The source owns the terms,
 * readiness, reservations and settlement; the screen changes nothing itself.
 */
export function TradePage({
  source,
  onBack,
  onFinish,
}: {
  source: TradeSource;
  /** Leaves the room open and returns to the surface that opened it. */
  onBack: () => void;
  /** Leaves a completed or closed room. */
  onFinish: () => void;
}): ReactElement {
  const ownPortrait = useOwnPortrait();
  if (source.status !== "ready")
    return (
      <section className={s.page} role="region" aria-label="Trade">
        {source.status === "loading" ? (
          <p className={s.opening} role="status">
            <span className={s.spinner} aria-hidden="true" />
            Opening trade…
          </p>
        ) : (
          <div className={s.closed}>
            <TradeClosedNotice
              reason="unavailable"
              onBack={onBack}
              headingId={CLOSED_HEADING}
            />
          </div>
        )}
      </section>
    );
  return (
    <Room
      source={source}
      ownPortrait={ownPortrait}
      onBack={onBack}
      onFinish={onFinish}
    />
  );
}

function Room({
  source,
  ownPortrait,
  onBack,
  onFinish,
}: {
  source: ReadyTrade;
  ownPortrait: Portrait | null;
  onBack: () => void;
  onFinish: () => void;
}): ReactElement {
  const [drafts, setDrafts] = useState<Record<string, string>>({});
  const [isPicking, setIsPicking] = useState(false);
  const [isConfirmingEdit, setIsConfirmingEdit] = useState(false);
  const [isConfirmingCancel, setIsConfirmingCancel] = useState(false);
  const addButton = useRef<HTMLButtonElement>(null);
  const editButton = useRef<HTMLButtonElement>(null);
  const cancelButton = useRef<HTMLButtonElement>(null);
  const { stage, own, other, partner, pending } = source;

  // A room that settles or closes while open hands focus to its result, when
  // focus was in the room. A room that opens already finished leaves focus alone.
  const ended = stage.kind === "complete" || stage.kind === "closed";
  const page = useRef<HTMLElement>(null);
  const wasOpen = useRef(!ended);
  useEffect(() => {
    if (!ended) {
      wasOpen.current = true;
      return;
    }
    if (!wasOpen.current) return;
    wasOpen.current = false;
    const active = document.activeElement;
    if (active && active !== document.body && !page.current?.contains(active))
      return;
    document
      .getElementById(
        stage.kind === "complete" ? RECEIPT_HEADING : CLOSED_HEADING,
      )
      ?.focus({ preventScroll: true });
  }, [ended, stage.kind]);

  // Returning to editing hands focus to Add items, unless it is elsewhere on purpose.
  const wasEditing = useRef(own.status === "editing");
  useEffect(() => {
    const isEditing = own.status === "editing";
    if (isEditing && !wasEditing.current) {
      const active = document.activeElement;
      if (!active || active === document.body || page.current?.contains(active))
        addButton.current?.focus({ preventScroll: true });
    }
    wasEditing.current = isEditing;
  }, [own.status]);

  const ownFace = <Face portrait={ownPortrait} />;
  const otherFace = <Face portrait={partner.portrait} />;

  if (stage.kind === "complete")
    return (
      <section
        ref={page}
        className={s.page}
        role="region"
        aria-label={`Trade with ${partner.name}`}
      >
        <div className={s.receipt}>
          <TradeReceipt
            headingId={RECEIPT_HEADING}
            gave={{ portrait: ownFace, items: stage.gave.map(trayItem) }}
            received={{
              portrait: otherFace,
              items: stage.received.map(trayItem),
            }}
            onDone={onFinish}
          />
        </div>
      </section>
    );
  if (stage.kind === "closed")
    return (
      <section
        ref={page}
        className={s.page}
        role="region"
        aria-label={`Trade with ${partner.name}`}
      >
        <div className={s.closed}>
          <TradeClosedNotice
            reason={stage.reason}
            onBack={onFinish}
            headingId={CLOSED_HEADING}
          />
        </div>
      </section>
    );

  const isUnconfirmed = stage.kind === "unconfirmed";
  const isEditing = own.status === "editing";
  const missing = shortfalls(own.lines, source.holdings);
  const owned = (id: string) =>
    source.holdings.find((holding) => holding.id === id)?.owned ?? 0;
  const draftOf = (line: OfferLine) => drafts[line.id] ?? String(line.quantity);
  const forget = (id: string) =>
    setDrafts((current) =>
      Object.fromEntries(Object.entries(current).filter(([key]) => key !== id)),
    );

  const ownTray = isEditing ? (
    <TradeTray
      kind="editable"
      compact={own.lines.length > 3}
      portrait={ownFace}
      addButtonRef={addButton}
      onAddItems={() => setIsPicking(true)}
      items={own.lines.map((line) => ({
        ...trayItem(line),
        editor: (
          <TradeQuantityEditor
            label={`${line.name}${line.detail ? ` ${line.detail}` : ""} quantity`}
            clearLabel={`Remove ${line.name}`}
            quantity={line.quantity}
            available={Math.max(owned(line.id), line.quantity)}
            draft={draftOf(line)}
            onDraftChange={(text) =>
              setDrafts((current) => ({ ...current, [line.id]: text }))
            }
            onCommit={(quantity) => {
              forget(line.id);
              source.setQuantity(line.id, quantity);
              // A removed row hands focus to Add items, which stays put.
              if (quantity === 0)
                requestAnimationFrame(() => addButton.current?.focus());
            }}
          />
        ),
      }))}
    />
  ) : (
    <TradeTray
      kind="locked"
      status={own.status === "confirmed" ? "confirmed" : "ready"}
      portrait={ownFace}
      items={own.lines.map(trayItem)}
      editButtonRef={editButton}
      onEditItems={() => {
        if (!pending && !isUnconfirmed) setIsConfirmingEdit(true);
      }}
    />
  );

  const roomStatus = tradeRoomStatus(own.status, other.status);
  const words = isUnconfirmed
    ? null
    : pending
      ? PENDING_WORDS[pending]
      : isEditing && missing.length > 0
        ? missing.map((entry) => entry.reason).join(" ")
        : statusWords(own.status, other.status, partner.name);

  return (
    <section
      ref={page}
      className={s.page}
      role="region"
      aria-label={`Trade with ${partner.name}`}
    >
      <div className={s.head}>
        <ActionButton
          className={s.back}
          icon={<ChevronLeft aria-hidden="true" />}
          onClick={onBack}
        >
          Back
        </ActionButton>
        <h1 className={s.title}>Trade with {partner.name}</h1>
      </div>
      <div className={s.columns} data-dense={own.lines.length > 3 || undefined}>
        <div className={s.slot} inert={isUnconfirmed || undefined}>
          {ownTray}
        </div>
        <div className={s.slot}>
          <TradeTray
            kind="offer"
            participant={partner.name}
            status={other.status}
            portrait={otherFace}
            items={other.lines.map(trayItem)}
          />
        </div>
      </div>
      <div className={s.footer}>
        {isUnconfirmed ? (
          <div className={s.checking} role="status">
            <span className={s.spinner} aria-hidden="true" />
            <div className={s.checkingCopy}>
              <h2 className={s.checkingTitle}>Checking trade</h2>
              <p className={s.checkingText}>We couldn’t confirm the result.</p>
            </div>
            <ActionButton
              className={s.check}
              pending={stage.isChecking}
              disabled={stage.isChecking}
              onClick={source.checkStatus}
            >
              Check status
            </ActionButton>
          </div>
        ) : (
          <div className={s.status}>
            <TradeStatusLine status={roomStatus} text={words} />
          </div>
        )}
        <TradeRoomActions
          status={roomStatus}
          canReady={
            !isUnconfirmed &&
            canMarkReady(own.lines, other.lines, source.holdings)
          }
          pending={pending !== null || isUnconfirmed}
          onReady={source.ready}
          onTrade={source.trade}
          onCancel={() => setIsConfirmingCancel(true)}
          cancelButtonRef={cancelButton}
        />
      </div>
      <div className={s.notices}>
        <FeedbackMessages
          label="Trade messages"
          presentation="toast"
          items={
            source.notice
              ? [
                  {
                    id: "notice",
                    tone: "warning",
                    text: source.notice,
                    onDismiss: source.dismissNotice,
                  },
                ]
              : []
          }
        />
      </div>
      {isPicking && (
        <TradeItemPicker
          returnFocusRef={addButton}
          items={source.holdings.map((holding) => {
            const added =
              own.lines.find((line) => line.id === holding.id)?.quantity ?? 0;
            return {
              id: holding.id,
              kind: holding.kind,
              name: holding.name,
              ...(holding.detail ? { detail: holding.detail } : {}),
              artwork: <Artwork asset={holding} />,
              available: Math.max(0, holding.owned - added),
              added,
            };
          })}
          onAdd={source.add}
          onClose={() => setIsPicking(false)}
        />
      )}
      <ConfirmationDialog
        {...editDecision}
        open={isConfirmingEdit}
        returnFocusRef={editButton}
        onCancel={() => setIsConfirmingEdit(false)}
        onConfirm={() => {
          setIsConfirmingEdit(false);
          source.editItems();
        }}
      />
      <ConfirmationDialog
        {...cancelDecision}
        open={isConfirmingCancel}
        returnFocusRef={cancelButton}
        onCancel={() => setIsConfirmingCancel(false)}
        onConfirm={() => {
          setIsConfirmingCancel(false);
          source.cancel();
        }}
      />
    </section>
  );
}

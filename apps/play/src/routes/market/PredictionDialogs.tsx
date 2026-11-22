import {
  useEffect,
  useId,
  useState,
  type ReactElement,
  type RefObject,
} from "react";
import { ArrowRight } from "lucide-react";
import {
  ActionButton,
  CoinAmount,
  Dialog,
  FeedbackMessages,
} from "@spellwake/ui";
import {
  amountProblem,
  BET_BLOCK_COPY,
  longDay,
  phaseOf,
  printingLine,
  questionOf,
  returnedOf,
  shortDay,
  usd,
  type Prediction,
  type ReadyPredictions,
  type Side,
  type SideQuote,
} from "../../market/prediction-sources";
import { COIN } from "./ListingDialogs";
import { OutcomePill, PredictionArt, SidePill } from "./PredictionParts";
import * as s from "./predictions.css";

const SIDE_NAME: Record<Side, string> = { yes: "Yes", no: "No" };
const DEFAULT_AMOUNT = "100";

/** The exact question, printing, finish and end date at the top of every dialog. */
function Item({
  prediction,
  side,
}: {
  prediction: Prediction;
  side?: Side | null;
}): ReactElement {
  return (
    <div className={s.dialogItem}>
      <PredictionArt card={prediction.card} />
      <div>
        <p className={s.dialogQuestion}>{questionOf(prediction)}</p>
        <p className={s.dialogMeta}>
          <span>{printingLine(prediction.card)}</span>
          <span aria-hidden="true">·</span>
          <span>{longDay(prediction.deadline)}</span>
          {side && (
            <>
              <span aria-hidden="true">·</span>
              <SidePill side={side} small />
            </>
          )}
        </p>
      </div>
    </div>
  );
}

/**
 * Make a prediction, as approved (BF-01 to BF-09): a whole-coin amount, the
 * side's If you win, and Yes and No. A first press on a side fetches fresh
 * terms and turns that button into Confirm; only a second press bets, and
 * only on the terms shown. Any change to the amount, or new terms, disarms
 * it. X, Escape or a press outside discards the unfinished bet.
 */
export function BetDialog({
  open,
  prediction,
  source,
  onClose,
  returnFocusRef,
}: {
  open: boolean;
  prediction: Prediction | null;
  source: ReadyPredictions;
  onClose: () => void;
  returnFocusRef: RefObject<HTMLElement | null>;
}): ReactElement | null {
  const fieldId = useId();
  const [text, setText] = useState(DEFAULT_AMOUNT);
  const [touched, setTouched] = useState(false);
  const [choosing, setChoosing] = useState<Side | null>(null);
  const [armed, setArmed] = useState<{
    side: Side;
    revision: string;
  } | null>(null);
  const [changed, setChanged] = useState(false);
  const [fresh, setFresh] = useState(0);
  const [isReadNoticeDismissed, setIsReadNoticeDismissed] = useState(false);
  const id = prediction?.id ?? null;
  // Each opening starts a fresh draft.
  useEffect(() => {
    if (!open) return;
    setText(DEFAULT_AMOUNT);
    setTouched(false);
    setChoosing(null);
    setArmed(null);
    setChanged(false);
    setIsReadNoticeDismissed(false);
  }, [open, id]);

  const block =
    source.betBlock ??
    (prediction && phaseOf(prediction) !== "betting" ? "closed" : null);
  const problem = amountProblem(text, source.balance);
  const amount = problem === null ? Number(text.trim()) : null;
  const { quoteFor } = source;
  useEffect(() => {
    if (!open || !id || block || amount === null) {
      quoteFor(null, 0);
      return;
    }
    // A new side choice asks again, so its Confirm rests on fresh terms.
    quoteFor(null, 0);
    quoteFor(id, amount);
  }, [open, id, block, amount, fresh, quoteFor]);
  useEffect(() => () => quoteFor(null, 0), [quoteFor]);

  const quote =
    source.quote &&
    source.quote.predictionId === id &&
    source.quote.amount === amount
      ? source.quote
      : null;
  const sideQuote = (side: Side): SideQuote =>
    quote ? quote[side] : { kind: "checking" };
  // Choosing a side arms it once its fresh terms arrive.
  const choosingQuote = choosing ? sideQuote(choosing) : null;
  useEffect(() => {
    if (!choosing || !choosingQuote) return;
    if (choosingQuote.kind === "ready") {
      setArmed({ side: choosing, revision: choosingQuote.revision });
      setChoosing(null);
    } else if (choosingQuote.kind !== "checking") setChoosing(null);
  }, [choosing, choosingQuote]);
  // New terms for the armed side disarm it.
  const armedQuote = armed ? sideQuote(armed.side) : null;
  useEffect(() => {
    if (!armed || !armedQuote || armedQuote.kind === "checking") return;
    if (armedQuote.kind !== "ready" || armedQuote.revision !== armed.revision) {
      setArmed(null);
      setChanged(armedQuote.kind === "ready");
    }
  }, [armed, armedQuote]);

  if (!prediction) return null;
  const yes = sideQuote("yes");
  const no = sideQuote("no");
  const shownSide: Side =
    armed?.side ??
    choosing ??
    (yes.kind === "ready" || no.kind !== "ready" ? "yes" : "no");
  const shown = sideQuote(shownSide);
  const bothUnavailable =
    yes.kind === "unavailable" && no.kind === "unavailable";
  const tooMuch = yes.kind === "too-much" || no.kind === "too-much";
  const readFailed = yes.kind === "failed" || no.kind === "failed";
  const fieldProblem =
    problem ??
    (tooMuch && shown.kind === "too-much"
      ? "That amount is unavailable. Try a smaller bet."
      : null);
  const showProblem = fieldProblem !== null && (touched || problem === null);
  const closedSide: Side | null =
    yes.kind === "unavailable"
      ? "yes"
      : no.kind === "unavailable"
        ? "no"
        : null;
  const note = bothUnavailable
    ? "No bets available right now."
    : shown.kind === "expired"
      ? "Winnings expired."
      : shown.kind === "unavailable"
        ? `${SIDE_NAME[shownSide]} bets unavailable.`
        : changed
          ? "Winnings changed. Choose again."
          : closedSide
            ? `${SIDE_NAME[closedSide]} bets unavailable.`
            : null;
  const winnings =
    problem !== null ? (
      <span className={s.winningsOff}>Unavailable</span>
    ) : shown.kind === "ready" ? (
      <CoinAmount amount={shown.win} iconSrc={COIN} />
    ) : shown.kind === "checking" || shown.kind === "expired" ? (
      <span className={s.winningsQuiet}>
        <span className={s.smallSpinner} aria-hidden="true" /> Updating…
      </span>
    ) : (
      <span className={s.winningsOff}>Unavailable</span>
    );

  const press = (side: Side) => {
    setTouched(true);
    if (amount === null || source.betPending) return;
    if (armed?.side === side) {
      source.placeBet(prediction.id, side, amount, armed.revision);
      return;
    }
    setArmed(null);
    setChanged(false);
    setChoosing(side);
    setFresh((count) => count + 1);
  };
  const sideButton = (side: Side) => {
    const isArmed = armed?.side === side;
    const usable = sideQuote(side).kind === "ready" || choosing === side;
    return (
      <ActionButton
        className={s.sideButton}
        data-armed={isArmed}
        aria-label={isArmed ? `Confirm ${SIDE_NAME[side]}` : SIDE_NAME[side]}
        pending={(isArmed && source.betPending) || choosing === side}
        disabled={
          problem !== null || !usable || (source.betPending && !isArmed)
        }
        onClick={() => press(side)}
      >
        {isArmed ? "Confirm" : SIDE_NAME[side]}
      </ActionButton>
    );
  };

  return (
    <Dialog
      open={open}
      title="Make a prediction"
      size="standard"
      busy={source.betPending}
      returnFocusRef={returnFocusRef}
      onClose={onClose}
    >
      <Item prediction={prediction} side={armed?.side ?? choosing} />
      {block ? (
        <p className={s.blockNote} role="status">
          {BET_BLOCK_COPY[block]}
        </p>
      ) : (
        <>
          <div className={s.betRow}>
            <div>
              <label className={s.exchangeLabel} htmlFor={fieldId}>
                Your bet
              </label>
              <div className={s.coinField} data-invalid={showProblem}>
                <img className={s.coinIcon} src={COIN} alt="" />
                <input
                  id={fieldId}
                  className={s.coinInput}
                  inputMode="numeric"
                  autoComplete="off"
                  data-autofocus=""
                  value={text}
                  aria-invalid={showProblem}
                  aria-describedby={
                    showProblem ? `${fieldId}-problem` : undefined
                  }
                  disabled={source.betPending}
                  onChange={(event) => {
                    setText(event.target.value);
                    setTouched(true);
                    setArmed(null);
                    setChoosing(null);
                    setChanged(false);
                  }}
                />
              </div>
            </div>
            <ArrowRight className={s.exchangeArrow} aria-hidden="true" />
            <div>
              <span className={s.exchangeLabel}>If you win</span>
              <span className={s.winnings} aria-live="polite">
                {winnings}
              </span>
            </div>
          </div>
          {showProblem && (
            <p id={`${fieldId}-problem`} className={s.fieldError} role="alert">
              {fieldProblem}
            </p>
          )}
          {!showProblem && note && (
            <p className={s.fieldNote} role="status">
              {note}
            </p>
          )}
          {readFailed && !isReadNoticeDismissed && (
            <div className={s.notice}>
              <FeedbackMessages
                label="Bet messages"
                presentation="inline"
                items={[
                  {
                    id: "read",
                    text: "Could not update winnings",
                    onDismiss: () => setIsReadNoticeDismissed(true),
                  },
                ]}
              />
            </div>
          )}
          {source.commandFailure && (
            <div className={s.notice}>
              <FeedbackMessages
                label="Bet outcome"
                presentation="inline"
                items={[
                  {
                    id: "command",
                    tone: "warning",
                    text: source.commandFailure,
                    onDismiss: source.dismissCommandFailure,
                  },
                ]}
              />
            </div>
          )}
          <div className={s.sides}>
            {sideButton("yes")}
            {sideButton("no")}
          </div>
        </>
      )}
    </Dialog>
  );
}

function Exchange({
  bet,
  side,
  right,
}: {
  bet: number;
  side?: Side;
  right: { label: string; amount: number } | null;
}): ReactElement {
  return (
    <div className={s.exchange}>
      <div>
        <span className={s.exchangeLabel}>
          Your bet{side ? ` · ${SIDE_NAME[side]}` : ""}
        </span>
        <span className={s.exchangeValue}>
          <CoinAmount amount={bet} iconSrc={COIN} />
        </span>
      </div>
      {right && (
        <>
          <ArrowRight className={s.exchangeArrow} aria-hidden="true" />
          <div>
            <span className={s.exchangeLabel}>{right.label}</span>
            <span className={s.exchangeValue}>
              <CoinAmount amount={right.amount} iconSrc={COIN} />
            </span>
          </div>
        </>
      )}
    </div>
  );
}

/** "Jan 29 price: $3.75", or Price unavailable when it cannot be read. */
function Evidence({ prediction }: { prediction: Prediction }): ReactElement {
  return prediction.evidence ? (
    <p className={s.verdictNote}>
      {shortDay(prediction.evidence.date)} price: {usd(prediction.evidence.usd)}
    </p>
  ) : (
    <p className={s.verdictNote} data-tone="quiet">
      Price unavailable.
    </p>
  );
}

/**
 * A prediction read-only, as approved (RS-01 to RS-08): an already placed
 * bet while betting is open, a closed one awaiting its result, or the
 * result with the viewer's own return. A viewer with no bet sees only the
 * public result. The only control is close.
 */
export function PredictionView({
  open,
  prediction,
  onClose,
  returnFocusRef,
}: {
  open: boolean;
  prediction: Prediction | null;
  onClose: () => void;
  returnFocusRef: RefObject<HTMLElement | null>;
}): ReactElement | null {
  if (!prediction) return null;
  const phase = phaseOf(prediction);
  const mine = prediction.mine;
  const active = phase === "betting" && mine.kind === "bet";
  let content: ReactElement;
  if (active) {
    content = (
      <>
        <div className={s.resultRow}>
          <span className={s.pill}>Betting open</span>
        </div>
        <Exchange
          bet={mine.cost}
          side={mine.side}
          right={{ label: "If you win", amount: mine.win }}
        />
        <div className={s.verdict}>
          <p className={s.verdictTitle}>Bet placed</p>
          <p className={s.verdictNote} data-tone="quiet">
            You can bet once per prediction.
          </p>
        </div>
      </>
    );
  } else {
    const outcome = phase === "resolved" ? prediction.outcome : null;
    const resultRow = (
      <div className={s.resultRow}>
        <span className={s.resultLabel}>Result</span>
        <OutcomePill outcome={outcome} />
      </div>
    );
    if (mine.kind === "bet") {
      content = (
        <>
          {resultRow}
          <Exchange
            bet={mine.cost}
            right={
              outcome
                ? { label: "Returned", amount: returnedOf(mine, outcome) }
                : null
            }
          />
          <div className={s.verdict}>
            {!outcome ? (
              <>
                <p className={s.verdictTitle}>Awaiting result</p>
                <p className={s.verdictNote} data-tone="quiet">
                  Betting is closed.
                </p>
              </>
            ) : outcome === "void" ? (
              <>
                <p className={s.verdictTitle}>Bet returned</p>
                {prediction.voidReason && (
                  <p className={s.verdictNote} data-tone="quiet">
                    {prediction.voidReason}
                  </p>
                )}
              </>
            ) : (
              <>
                <p className={s.verdictTitle}>
                  {outcome === mine.side ? "You won" : "You lost"}
                </p>
                <Evidence prediction={prediction} />
              </>
            )}
          </div>
        </>
      );
    } else if (mine.kind === "none") {
      content = (
        <>
          {resultRow}
          <div className={s.publicNote}>
            {!outcome ? (
              <>
                <p className={s.verdictNote}>Awaiting result</p>
                <p className={s.verdictNote} data-tone="quiet">
                  Betting is closed.
                </p>
              </>
            ) : outcome === "void" ? (
              <p className={s.verdictNote} data-tone="quiet">
                {prediction.voidReason ?? "Bets returned."}
              </p>
            ) : (
              <Evidence prediction={prediction} />
            )}
          </div>
        </>
      );
    } else {
      content = (
        <>
          {resultRow}
          <div className={s.verdict}>
            <p className={s.verdictTitle}>Your result unavailable.</p>
          </div>
        </>
      );
    }
  }
  return (
    <Dialog
      open={open}
      title={active ? "Prediction" : "Prediction result"}
      size="standard"
      returnFocusRef={returnFocusRef}
      onClose={onClose}
    >
      <Item prediction={prediction} />
      {content}
    </Dialog>
  );
}

import {
  useEffect,
  useMemo,
  useRef,
  useState,
  type ReactElement,
  type ReactNode,
} from "react";
import { parseAsString, parseAsStringLiteral, useQueryStates } from "nuqs";
import { ArrowRight, Box, Plus } from "lucide-react";
import {
  ActionButton,
  CoinAmount,
  EmptyState,
  FeedbackMessages,
  TextField,
} from "@spellwake/ui";
import {
  betStatus,
  entryOf,
  matchPredictions,
  phaseOf,
  printingLine,
  questionOf,
  questionStatus,
  returnedOf,
  type Group,
  type Prediction,
  type PredictionsSource,
  type ReadyPredictions,
} from "../../market/prediction-sources";
import { COIN } from "./ListingDialogs";
import { CreatePredictionDialog } from "./CreatePredictionDialog";
import { BetDialog, PredictionView } from "./PredictionDialogs";
import {
  DateChip,
  OutcomePill,
  PredictionArt,
  SidePill,
  TitleTrail,
} from "./PredictionParts";
import * as s from "./predictions.css";

const TABS = ["explore", "mine"] as const;
type Tab = (typeof TABS)[number];
const queryParsers = {
  tab: parseAsStringLiteral(TABS).withDefault("explore"),
  q: parseAsString.withDefault(""),
  mq: parseAsString.withDefault(""),
};
const GAP_COPY = {
  "signed-out": {
    title: "Sign in to make predictions",
    description: "Bets and predictions belong to your account.",
  },
  unregistered: {
    title: "Finish creating your account",
    description: "Choose a username to start predicting.",
  },
} as const;

type Open = (prediction: Prediction, button: HTMLElement) => void;

/** One question on the Explore board; the action covers the card. */
function QuestionCard({
  prediction,
  onOpen,
}: {
  prediction: Prediction;
  onOpen: Open;
}): ReactElement {
  const entry = entryOf(prediction);
  const question = questionOf(prediction);
  return (
    <li className={s.card}>
      <div className={s.identity}>
        <PredictionArt card={prediction.card} />
        <div style={{ minWidth: 0 }}>
          <p className={s.name}>{prediction.card.name}</p>
          <p className={s.meta}>{printingLine(prediction.card)}</p>
        </div>
      </div>
      <h3 className={s.question}>{question}</h3>
      <DateChip day={prediction.deadline} />
      {phaseOf(prediction) === "resolved" && (
        <div className={s.statusRow}>
          <OutcomePill outcome={prediction.outcome} prefix="Result" checked />
        </div>
      )}
      <span className={s.cardSpacer} />
      <div className={s.cardFoot}>
        <button
          type="button"
          className={s.cardAction}
          aria-haspopup="dialog"
          aria-label={`${entry === "bet" ? "Make a prediction" : "View prediction"}: ${question}`}
          onClick={(event) => onOpen(prediction, event.currentTarget)}
        >
          {entry === "bet" ? "Make a prediction" : "View prediction"}
          <ArrowRight aria-hidden="true" />
        </button>
      </div>
    </li>
  );
}

function BetCard({
  prediction,
  onOpen,
}: {
  prediction: Prediction;
  onOpen: Open;
}): ReactElement | null {
  const bet = prediction.mine;
  const question = questionOf(prediction);
  const status =
    bet.kind === "bet" ? betStatus(prediction, bet) : "Result unavailable";
  const phase = phaseOf(prediction);
  return (
    <li className={s.betCard}>
      <div className={s.betHead}>
        <PredictionArt card={prediction.card} small />
        <div style={{ minWidth: 0 }}>
          <p className={s.name}>{prediction.card.name}</p>
          <p className={s.meta}>{printingLine(prediction.card)}</p>
        </div>
        <div className={s.betTags}>
          {bet.kind === "bet" && <SidePill side={bet.side} />}
          <span
            className={s.status}
            data-tone={status === "Lost" ? "lost" : undefined}
          >
            {status}
          </span>
        </div>
      </div>
      <p className={s.betQuestion}>{question}</p>
      <DateChip day={prediction.deadline} />
      <div className={s.cardFoot} style={{ marginTop: 14 }}>
        {bet.kind === "bet" ? (
          <div className={s.amounts}>
            <span>
              <span className={s.amountLabel}>Your bet</span>
              <CoinAmount amount={bet.cost} iconSrc={COIN} />
            </span>
            {phase !== "awaiting" && (
              <>
                <ArrowRight aria-hidden="true" />
                <span>
                  <span className={s.amountLabel}>
                    {phase === "betting" ? "If you win" : "Returned"}
                  </span>
                  <CoinAmount
                    amount={
                      phase === "betting" || !prediction.outcome
                        ? bet.win
                        : returnedOf(bet, prediction.outcome)
                    }
                    iconSrc={COIN}
                  />
                </span>
              </>
            )}
          </div>
        ) : (
          <span className={s.amounts}>Your result unavailable.</span>
        )}
        <button
          type="button"
          className={s.rowAction}
          aria-haspopup="dialog"
          aria-label={`View: ${question}`}
          onClick={(event) => onOpen(prediction, event.currentTarget)}
        >
          View <ArrowRight aria-hidden="true" />
        </button>
      </div>
    </li>
  );
}

function CreatedRow({
  prediction,
  onOpen,
}: {
  prediction: Prediction;
  onOpen: Open;
}): ReactElement {
  const status = questionStatus(prediction);
  const question = questionOf(prediction);
  return (
    <li className={s.createdRow}>
      <div className={s.identity} style={{ gap: 16 }}>
        <PredictionArt card={prediction.card} small />
        <div style={{ minWidth: 0 }}>
          <p className={s.name}>{prediction.card.name}</p>
          <p className={s.meta}>{printingLine(prediction.card)}</p>
        </div>
      </div>
      <div className={s.createdQuestion}>
        <p>{question}</p>
        <DateChip day={prediction.deadline} />
      </div>
      <span
        className={s.pill}
        data-tone={
          status === "Betting open"
            ? "open"
            : status === "Yes"
              ? "yes"
              : status === "No"
                ? "no"
                : status === "Awaiting result"
                  ? "pending"
                  : undefined
        }
      >
        {status}
      </span>
      <button
        type="button"
        className={s.rowAction}
        aria-haspopup="dialog"
        aria-label={`View: ${question}`}
        onClick={(event) => onOpen(prediction, event.currentTarget)}
      >
        View <ArrowRight aria-hidden="true" />
      </button>
    </li>
  );
}

function SkeletonCards({ count }: { count: number }): ReactElement {
  return (
    <ul className={s.board} aria-hidden="true">
      {Array.from({ length: count }, (_, at) => (
        <li key={at} className={s.card} style={{ minHeight: 280 }}>
          <div className={s.identity}>
            <span className={`${s.skeleton} ${s.art}`} />
            <span>
              <span
                className={s.skeleton}
                style={{ width: "70%", height: 16 }}
              />
              <span
                className={s.skeleton}
                style={{ width: "45%", height: 12, marginTop: 10 }}
              />
            </span>
          </div>
          <span
            className={s.skeleton}
            style={{ width: "90%", height: 16, marginTop: 28 }}
          />
          <span
            className={s.skeleton}
            style={{ width: "60%", height: 16, marginTop: 10 }}
          />
          <span
            className={s.skeleton}
            style={{
              width: "40%",
              height: 22,
              marginTop: 22,
              borderRadius: 999,
            }}
          />
        </li>
      ))}
    </ul>
  );
}
function SkeletonRows({ count }: { count: number }): ReactElement {
  return (
    <ul className={s.createdRows} aria-hidden="true">
      {Array.from({ length: count }, (_, at) => (
        <li key={at} className={s.quietRow}>
          <span
            className={`${s.skeleton} ${s.art} ${s.artSmall}`}
            style={{ width: 46 }}
          />
          <span style={{ flex: 1 }}>
            <span className={s.skeleton} style={{ width: "50%", height: 14 }} />
            <span
              className={s.skeleton}
              style={{ width: "30%", height: 10, marginTop: 8 }}
            />
          </span>
        </li>
      ))}
    </ul>
  );
}
function QuietRow({ children }: { children: ReactNode }): ReactElement {
  return (
    <p className={s.quietRow} role="status">
      <Box aria-hidden="true" />
      {children}
    </p>
  );
}

/** The scroll body asks for more as its end comes into view. */
function useContinuation(
  body: React.RefObject<HTMLElement | null>,
  onEnd: () => void,
  active: boolean,
) {
  const end = useRef<HTMLDivElement>(null);
  const latest = useRef(onEnd);
  latest.current = onEnd;
  useEffect(() => {
    const node = end.current;
    if (!active || !node || typeof IntersectionObserver === "undefined") return;
    const observer = new IntersectionObserver(
      (entries) => {
        if (entries.some((entry) => entry.isIntersecting)) latest.current();
      },
      { root: body.current, rootMargin: "0px 0px 320px 0px" },
    );
    observer.observe(node);
    return () => observer.disconnect();
  }, [active, body]);
  return end;
}

function Explore({
  source,
  query,
  onOpen,
  body,
}: {
  source: ReadyPredictions;
  query: string;
  onOpen: Open;
  body: React.RefObject<HTMLElement | null>;
}): ReactElement {
  const { explore } = source;
  const shown = useMemo(
    () => matchPredictions(explore.items, query),
    [explore.items, query],
  );
  // A search covers every prediction, so the rest are read while one is active.
  useEffect(() => {
    if (query.trim() && explore.hasMore && !explore.isAppending)
      explore.loadMore();
  }, [query, explore]);
  const end = useContinuation(body, explore.loadMore, explore.hasMore);
  if (explore.state === "loading") return <SkeletonCards count={3} />;
  if (explore.state === "failed" && explore.items.length === 0)
    return <p className={s.quiet}>Predictions unavailable</p>;
  if (shown.length === 0)
    return explore.isAppending || (query.trim() && explore.hasMore) ? (
      <div className={s.appending} role="status">
        <span className={s.spinner} aria-hidden="true" />
        <span className={s.srOnly}>Loading predictions…</span>
      </div>
    ) : (
      <p className={s.quiet} role="status">
        {query.trim() ? "No matching predictions" : "No predictions yet"}
      </p>
    );
  return (
    <>
      <ul className={s.board} aria-label="Predictions">
        {shown.map((prediction) => (
          <QuestionCard
            key={prediction.id}
            prediction={prediction}
            onOpen={onOpen}
          />
        ))}
      </ul>
      <div ref={end} />
      {explore.isAppending && (
        <div className={s.appending} role="status">
          <span className={s.spinner} aria-hidden="true" />
          <span className={s.srOnly}>Loading more predictions…</span>
        </div>
      )}
    </>
  );
}

function Mine({
  source,
  query,
  onOpen,
  onExplore,
  onCreate,
}: {
  source: ReadyPredictions;
  query: string;
  onOpen: Open;
  onExplore: () => void;
  onCreate: (button: HTMLElement) => void;
}): ReactElement {
  const { bets, created } = source.mine;
  const searching = query.trim().length > 0;
  const content = (
    group: Group,
    render: (items: Prediction[]) => ReactElement,
    copy: {
      failed: string;
      noMatch: string;
      empty: string;
      action: ReactElement;
    },
  ) => {
    if (group.state === "loading") return <SkeletonRows count={2} />;
    if (group.state === "failed") return <QuietRow>{copy.failed}</QuietRow>;
    if (group.state === "unavailable")
      return <QuietRow>Created predictions unavailable</QuietRow>;
    const items = matchPredictions(group.items, query);
    if (items.length > 0) return render(items);
    if (searching)
      return (
        <p className={s.quietRow} role="status">
          {copy.noMatch}
        </p>
      );
    return (
      <div className={s.emptyPanel}>
        <p className={s.emptyTitle}>{copy.empty}</p>
        {copy.action}
      </div>
    );
  };
  return (
    <>
      <section className={s.group} aria-labelledby="prediction-bets">
        <h2 id="prediction-bets" className={s.groupTitle}>
          Your bets
        </h2>
        {content(
          bets,
          (items) => (
            <ul className={s.bets}>
              {items.map((prediction) => (
                <BetCard
                  key={prediction.id}
                  prediction={prediction}
                  onOpen={onOpen}
                />
              ))}
            </ul>
          ),
          {
            failed: "Could not load predictions",
            noMatch: "No matching bets",
            empty: "No bets yet",
            action: (
              <button
                type="button"
                className={s.emptyAction}
                onClick={onExplore}
              >
                Explore predictions <ArrowRight aria-hidden="true" />
              </button>
            ),
          },
        )}
      </section>
      <section className={s.group} aria-labelledby="prediction-created">
        <h2 id="prediction-created" className={s.groupTitle}>
          Created by you
        </h2>
        {content(
          created,
          (items) => (
            <ul className={s.createdRows}>
              {items.map((prediction) => (
                <CreatedRow
                  key={prediction.id}
                  prediction={prediction}
                  onOpen={onOpen}
                />
              ))}
            </ul>
          ),
          {
            failed: "Could not load created predictions",
            noMatch: "No matching predictions",
            empty: "No predictions created",
            action: (
              <button
                type="button"
                className={s.emptyAction}
                aria-haspopup="dialog"
                onClick={(event) => onCreate(event.currentTarget)}
              >
                Create prediction <ArrowRight aria-hidden="true" />
              </button>
            ),
          },
        )}
      </section>
    </>
  );
}

/**
 * Market Predictions, as approved (market-predictions-desktop-approved-v1):
 * the title with search and Create prediction, then Explore, every question
 * as a card, and My predictions, the viewer's bets and the questions they
 * created. Each tab keeps its own search. A card opens a new bet while one
 * can still be placed, or the prediction read-only. Dialogs return to the
 * same tab, search and scroll.
 */
export function PredictionsPage({
  source,
}: {
  source: PredictionsSource;
}): ReactElement {
  const [params, setParams] = useQueryStates(queryParsers);
  const [chosenId, setChosenId] = useState<string | null>(null);
  const [isCreating, setIsCreating] = useState(false);
  const trigger = useRef<HTMLElement | null>(null);
  const createButton = useRef<HTMLButtonElement>(null);
  const body = useRef<HTMLDivElement>(null);
  const ready = source.status === "ready" ? source : null;
  const tab: Tab = params.tab;
  const query = tab === "explore" ? params.q : params.mq;

  const everything = useMemo(() => {
    if (!ready) return [];
    const seen = new Map<string, Prediction>();
    for (const list of [
      ready.explore.items,
      ready.mine.bets.items,
      ready.mine.created.items,
    ])
      for (const prediction of list)
        if (!seen.has(prediction.id)) seen.set(prediction.id, prediction);
    return [...seen.values()];
  }, [ready]);
  const chosen = everything.find((entry) => entry.id === chosenId) ?? null;
  // The dialog a card opened stays the one it opened, even once a bet lands.
  const [mode, setMode] = useState<"bet" | "view">("view");

  const completed = ready?.completed ?? null;
  useEffect(() => {
    if (completed?.kind === "bet") setChosenId(null);
    if (completed?.kind === "created") setIsCreating(false);
  }, [completed]);

  const messages = ready
    ? [
        ...(ready.failure
          ? [
              {
                id: "read",
                text: ready.failure,
                onDismiss: ready.dismissFailure,
              },
            ]
          : []),
        ...(completed
          ? [
              {
                id: "done",
                tone: "success" as const,
                text:
                  completed.kind === "bet"
                    ? "Bet placed"
                    : "Prediction created.",
                onDismiss: ready.dismissCompleted,
              },
            ]
          : []),
      ]
    : [];

  const open: Open = (prediction, button) => {
    trigger.current = button;
    setMode(entryOf(prediction));
    setChosenId(prediction.id);
  };
  const openCreate = (button: HTMLElement) => {
    trigger.current = button;
    setIsCreating(true);
  };

  let content: ReactNode;
  if (source.status === "loading") content = <SkeletonCards count={3} />;
  else if (source.status === "signed-out" || source.status === "unregistered")
    content = <EmptyState {...GAP_COPY[source.status]} />;
  else if (source.status === "unavailable")
    content = (
      <div className={s.accountGap}>
        <p className={s.emptyTitle}>Predictions unavailable</p>
        <p style={{ margin: 0 }}>Your account is unavailable.</p>
      </div>
    );
  else
    content =
      tab === "explore" ? (
        <Explore source={source} query={params.q} onOpen={open} body={body} />
      ) : (
        <Mine
          source={source}
          query={params.mq}
          onOpen={open}
          onExplore={() => void setParams({ tab: null })}
          onCreate={openCreate}
        />
      );

  return (
    <section className={s.root} aria-label="Predictions" role="region">
      <div className={s.head}>
        <h1 className={s.title}>
          Predictions
          <TitleTrail />
        </h1>
        <div className={s.searchCell}>
          <TextField
            type="search"
            label={
              tab === "explore" ? "Search predictions" : "Search my predictions"
            }
            placeholder="Search predictions…"
            hideLabel
            treatment="compact-form"
            value={query}
            disabled={!ready}
            onValueChange={(value) =>
              void setParams(
                tab === "explore"
                  ? { q: value || null }
                  : { mq: value || null },
              )
            }
          />
        </div>
        <ActionButton
          ref={createButton}
          className={s.createButton}
          aria-haspopup="dialog"
          disabled={!ready}
          onClick={(event) => openCreate(event.currentTarget)}
        >
          <Plus aria-hidden="true" />
          Create prediction
        </ActionButton>
      </div>
      <div className={s.tabs} role="tablist" aria-label="Predictions">
        {TABS.map((value) => (
          <button
            key={value}
            type="button"
            role="tab"
            id={`prediction-tab-${value}`}
            aria-selected={tab === value}
            aria-controls="prediction-panel"
            tabIndex={tab === value ? 0 : -1}
            className={s.tab}
            onClick={() =>
              void setParams({ tab: value === "explore" ? null : value })
            }
            onKeyDown={(event) => {
              if (event.key !== "ArrowLeft" && event.key !== "ArrowRight")
                return;
              event.preventDefault();
              const next = value === "explore" ? "mine" : "explore";
              void setParams({ tab: next === "explore" ? null : next });
              document.getElementById(`prediction-tab-${next}`)?.focus();
            }}
          >
            {value === "explore" ? "Explore" : "My predictions"}
            {tab === value && <span className={s.tabMark} aria-hidden="true" />}
          </button>
        ))}
      </div>
      <div
        ref={body}
        className={s.body}
        id="prediction-panel"
        role="tabpanel"
        aria-labelledby={`prediction-tab-${tab}`}
      >
        {content}
      </div>

      {ready && (
        <>
          <BetDialog
            open={chosen !== null && mode === "bet"}
            prediction={mode === "bet" ? chosen : null}
            source={ready}
            returnFocusRef={trigger}
            onClose={() => {
              ready.dismissCommandFailure();
              setChosenId(null);
            }}
          />
          <PredictionView
            open={chosen !== null && mode === "view"}
            prediction={mode === "view" ? chosen : null}
            returnFocusRef={trigger}
            onClose={() => setChosenId(null)}
          />
          <CreatePredictionDialog
            open={isCreating}
            source={ready}
            returnFocusRef={createButton}
            onClose={() => {
              ready.dismissCommandFailure();
              setIsCreating(false);
            }}
          />
        </>
      )}
      <FeedbackMessages
        label="Prediction messages"
        presentation="toast"
        items={messages}
      />
    </section>
  );
}

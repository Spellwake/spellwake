import {
  useEffect,
  useId,
  useMemo,
  useRef,
  useState,
  type KeyboardEvent,
  type ReactElement,
  type RefObject,
} from "react";
import {
  CalendarDays,
  Check,
  ChevronDown,
  ChevronLeft,
  ChevronRight,
  Copy,
} from "lucide-react";
import {
  ActionButton,
  Dialog,
  FeedbackMessages,
  TextField,
} from "@spellwake/ui";
import {
  addDays,
  dateProblem,
  longDay,
  printingLine,
  scheduleFrom,
  targetProblem,
  utcDay,
  type Condition,
  type PrintingLabel,
  type ReadyPredictions,
  type Schedule,
} from "../../market/prediction-sources";
import { PredictionArt } from "./PredictionParts";
import * as s from "./predictions.css";

const CONDITIONS: { value: Condition; label: string }[] = [
  { value: "reached_by", label: "Reaches this price by the end date" },
  { value: "at_deadline", label: "At least this price on the end date" },
];
const WEEKDAYS = ["Mo", "Tu", "We", "Th", "Fr", "Sa", "Su"];
const MONTH_NAMES = [
  "January",
  "February",
  "March",
  "April",
  "May",
  "June",
  "July",
  "August",
  "September",
  "October",
  "November",
  "December",
];
const monthOf = (ymd: string) => ymd.slice(0, 7);
const shiftMonth = (month: string, by: number) => {
  const [year, index] = month.split("-").map(Number);
  const date = new Date(Date.UTC(year!, index! - 1 + by, 1));
  return utcDay(date).slice(0, 7);
};

/** A month of dates; arrows move the focused day, Enter chooses it, Escape closes. */
function Calendar({
  value,
  schedule,
  onChoose,
  onClose,
}: {
  value: string | null;
  schedule: Schedule;
  onChoose: (ymd: string) => void;
  onClose: () => void;
}): ReactElement {
  const [focused, setFocused] = useState(value ?? schedule.earliest);
  const [month, setMonth] = useState(monthOf(focused));
  const grid = useRef<HTMLDivElement>(null);
  useEffect(() => {
    grid.current
      ?.querySelector<HTMLButtonElement>(`[data-day="${focused}"]`)
      ?.focus();
  }, [focused]);
  const first = `${month}-01`;
  // Monday-first offset of the month's first day.
  const offset = (new Date(`${first}T00:00:00Z`).getUTCDay() + 6) % 7;
  const length = new Date(
    Date.UTC(Number(month.slice(0, 4)), Number(month.slice(5, 7)), 0),
  ).getUTCDate();
  const usable = (ymd: string) =>
    ymd >= schedule.earliest && ymd <= schedule.latest;
  const move = (days: number) => {
    const next = addDays(focused, days);
    setFocused(next);
    setMonth(monthOf(next));
  };
  const onKeyDown = (event: KeyboardEvent<HTMLDivElement>) => {
    const steps: Record<string, number> = {
      ArrowLeft: -1,
      ArrowRight: 1,
      ArrowUp: -7,
      ArrowDown: 7,
    };
    if (event.key in steps) {
      event.preventDefault();
      move(steps[event.key]!);
    } else if (event.key === "PageUp" || event.key === "PageDown") {
      event.preventDefault();
      const next = `${shiftMonth(monthOf(focused), event.key === "PageUp" ? -1 : 1)}-${focused.slice(8)}`;
      const valid = new Date(`${next}T00:00:00Z`).toISOString().slice(0, 10);
      setFocused(valid);
      setMonth(monthOf(valid));
    } else if (event.key === "Escape") {
      // Closes the calendar only; the dialog stays.
      event.preventDefault();
      event.stopPropagation();
      onClose();
    }
  };
  const [year, index] = month.split("-").map(Number);
  return (
    <div
      className={s.calendar}
      role="dialog"
      aria-label="Choose end date"
      onKeyDown={onKeyDown}
    >
      <div className={s.calendarHead}>
        <button
          type="button"
          className={s.monthButton}
          aria-label="Previous month"
          disabled={month <= monthOf(schedule.earliest)}
          onClick={() => setMonth(shiftMonth(month, -1))}
        >
          <ChevronLeft aria-hidden="true" />
        </button>
        <span aria-live="polite">
          {MONTH_NAMES[index! - 1]} {year}
        </span>
        <button
          type="button"
          className={s.monthButton}
          aria-label="Next month"
          disabled={month >= monthOf(schedule.latest)}
          onClick={() => setMonth(shiftMonth(month, 1))}
        >
          <ChevronRight aria-hidden="true" />
        </button>
      </div>
      <div className={s.weekdays} aria-hidden="true">
        {WEEKDAYS.map((day) => (
          <span key={day}>{day}</span>
        ))}
      </div>
      <div className={s.days} role="grid" ref={grid}>
        {Array.from({ length: offset }, (_, at) => (
          <span key={`gap-${at}`} />
        ))}
        {Array.from({ length }, (_, at) => {
          const ymd = `${month}-${String(at + 1).padStart(2, "0")}`;
          return (
            <button
              key={ymd}
              type="button"
              className={s.day}
              data-day={ymd}
              role="gridcell"
              aria-selected={ymd === value}
              aria-label={longDay(ymd)}
              tabIndex={ymd === focused ? 0 : -1}
              disabled={!usable(ymd)}
              onClick={() => onChoose(ymd)}
            >
              {at + 1}
            </button>
          );
        })}
      </div>
    </div>
  );
}

/** Choose card: every accepted card's printings by name, then Select. */
function Picker({
  source,
  initial,
  onBack,
  onSelect,
}: {
  source: ReadyPredictions;
  initial: PrintingLabel | null;
  onBack: () => void;
  onSelect: (card: PrintingLabel) => void;
}): ReactElement {
  const [query, setQuery] = useState(initial?.name ?? "");
  const [pick, setPick] = useState<PrintingLabel | null>(initial);
  const [isNoticeDismissed, setIsNoticeDismissed] = useState(false);
  const { searchCards } = source;
  useEffect(() => {
    searchCards(query);
    setIsNoticeDismissed(false);
  }, [query, searchCards]);
  useEffect(() => () => searchCards(null), [searchCards]);
  const { cards } = source;
  const list = useRef<HTMLUListElement>(null);
  return (
    <div className={s.createBody}>
      <div className={s.pickerHead}>
        <button
          type="button"
          className={s.backButton}
          aria-label="Back to Create prediction"
          onClick={onBack}
        >
          <ChevronLeft aria-hidden="true" />
        </button>
        <TextField
          type="search"
          label="Search cards"
          placeholder="Search cards…"
          hideLabel
          treatment="compact-form"
          value={query}
          onValueChange={setQuery}
          data-autofocus=""
        />
      </div>
      {cards.state === "loading" ? (
        <ul className={s.results} aria-hidden="true">
          {[0, 1, 2].map((row) => (
            <li key={row} className={s.result}>
              <span
                className={`${s.skeleton} ${s.artSmall}`}
                style={{ aspectRatio: "1" }}
              />
              <span>
                <span
                  className={s.skeleton}
                  style={{ width: "60%", height: 14 }}
                />
                <span
                  className={s.skeleton}
                  style={{ width: "35%", height: 10, marginTop: 8 }}
                />
              </span>
            </li>
          ))}
        </ul>
      ) : cards.items.length > 0 ? (
        <ul
          className={s.results}
          role="listbox"
          aria-label="Cards"
          ref={list}
          onScroll={(event) => {
            const node = event.currentTarget;
            if (node.scrollHeight - node.scrollTop - node.clientHeight < 120)
              cards.loadMore();
          }}
        >
          {cards.items.map((card) => {
            const selected = pick?.printingId === card.printingId;
            return (
              <li key={card.printingId}>
                <button
                  type="button"
                  role="option"
                  aria-selected={selected}
                  className={s.result}
                  onClick={() => setPick(card)}
                  onDoubleClick={() => onSelect(card)}
                >
                  <PredictionArt card={card} small />
                  <span>
                    <span className={s.resultName}>{card.name}</span>
                    <span className={s.resultMeta}>{printingLine(card)}</span>
                  </span>
                  {selected ? <Check aria-hidden="true" /> : <span />}
                </button>
              </li>
            );
          })}
          {cards.isAppending && (
            <li className={s.appending}>
              <span className={s.spinner} aria-hidden="true" />
            </li>
          )}
        </ul>
      ) : cards.state === "ready" ? (
        <p className={s.pickerNote} role="status">
          No matching cards.
        </p>
      ) : (
        <p className={s.pickerNote} />
      )}
      {cards.state === "failed" && !isNoticeDismissed && (
        <div className={s.notice}>
          <FeedbackMessages
            label="Card messages"
            presentation="inline"
            items={[
              {
                id: "read",
                text: "Couldn’t load cards.",
                onDismiss: () => setIsNoticeDismissed(true),
              },
            ]}
          />
        </div>
      )}
      <ActionButton
        className={s.wide}
        disabled={!pick}
        onClick={() => pick && onSelect(pick)}
      >
        Select
      </ActionButton>
    </div>
  );
}

/**
 * Create prediction, as approved (PC-01 to PC-05): the exact printing, a
 * positive dollar target, an end date after the seven-day betting window and
 * one of two prediction types. Create publishes the question only; it never
 * places a bet. Closing before publishing discards the draft. After a
 * failure the same Create prediction can be pressed again.
 */
export function CreatePredictionDialog({
  open,
  source,
  onClose,
  returnFocusRef,
}: {
  open: boolean;
  source: ReadyPredictions;
  onClose: () => void;
  returnFocusRef: RefObject<HTMLElement | null>;
}): ReactElement {
  const targetId = useId();
  const typeId = useId();
  const [stage, setStage] = useState<"form" | "picker">("form");
  const [card, setCard] = useState<PrintingLabel | null>(null);
  const [target, setTarget] = useState("");
  const [deadline, setDeadline] = useState<string | null>(null);
  const [condition, setCondition] = useState<Condition>("reached_by");
  const [touched, setTouched] = useState({ target: false, date: false });
  const [isCalendarOpen, setIsCalendarOpen] = useState(false);
  const [isTypeOpen, setIsTypeOpen] = useState(false);
  const [openedAt, setOpenedAt] = useState(() => new Date());
  const dateButton = useRef<HTMLButtonElement>(null);
  const typeButton = useRef<HTMLButtonElement>(null);
  // Each opening starts a fresh draft.
  useEffect(() => {
    if (!open) return;
    setStage("form");
    setCard(null);
    setTarget("");
    setDeadline(null);
    setCondition("reached_by");
    setTouched({ target: false, date: false });
    setIsCalendarOpen(false);
    setIsTypeOpen(false);
    setOpenedAt(new Date());
  }, [open]);
  const schedule = useMemo(() => scheduleFrom(openedAt), [openedAt]);
  const { checkPrice } = source;
  useEffect(() => {
    checkPrice(open ? (card?.printingId ?? null) : null);
  }, [open, card?.printingId, checkPrice]);

  const price =
    card && source.price?.printingId === card.printingId ? source.price : null;
  const targetError = targetProblem(target);
  const dateError = dateProblem(deadline, schedule);
  const canCreate =
    !!card &&
    price?.state === "ready" &&
    targetError === null &&
    dateError === null &&
    !source.publishPending;
  const closeType = (focusButton: boolean) => {
    setIsTypeOpen(false);
    if (focusButton) typeButton.current?.focus();
  };

  return (
    <Dialog
      open={open}
      title={stage === "picker" ? "Choose card" : "Create prediction"}
      size="standard"
      busy={source.publishPending}
      returnFocusRef={returnFocusRef}
      onClose={onClose}
    >
      {stage === "picker" ? (
        <Picker
          source={source}
          initial={card}
          onBack={() => setStage("form")}
          onSelect={(chosen) => {
            setCard(chosen);
            setStage("form");
          }}
        />
      ) : (
        <div className={s.createBody}>
          <div className={s.chosen}>
            {card ? (
              <PredictionArt card={card} small />
            ) : (
              <span className={s.placeholderArt} aria-hidden="true">
                <Copy />
              </span>
            )}
            <div>
              <p className={s.name}>{card ? card.name : "No card selected"}</p>
              {card && <p className={s.meta}>{printingLine(card)}</p>}
            </div>
            <ActionButton
              className={s.smallButton}
              disabled={source.publishPending}
              data-autofocus={card ? undefined : ""}
              onClick={() => setStage("picker")}
            >
              {card ? "Change" : "Choose card"}
            </ActionButton>
          </div>
          {price?.state === "unavailable" && (
            <p className={s.blockedLine} role="status">
              Price unavailable for this printing.
            </p>
          )}
          <div className={s.pair}>
            <div>
              <label className={s.fieldLabel} htmlFor={targetId}>
                Target price
              </label>
              <div
                className={s.inputBox}
                data-invalid={touched.target && targetError !== null}
              >
                <span aria-hidden="true">$</span>
                <input
                  id={targetId}
                  className={s.plainInput}
                  inputMode="decimal"
                  autoComplete="off"
                  value={target}
                  disabled={source.publishPending}
                  aria-invalid={touched.target && targetError !== null}
                  onChange={(event) => setTarget(event.target.value)}
                  onBlur={() =>
                    setTouched((current) => ({ ...current, target: true }))
                  }
                />
              </div>
              {touched.target && targetError && (
                <p className={s.fieldError} role="alert">
                  {targetError}
                </p>
              )}
            </div>
            <div>
              <span className={s.fieldLabel} id={`${targetId}-date`}>
                End date
              </span>
              <button
                ref={dateButton}
                type="button"
                className={s.dateButton}
                aria-labelledby={`${targetId}-date ${targetId}-date-value`}
                aria-expanded={isCalendarOpen}
                data-invalid={touched.date && dateError !== null}
                disabled={source.publishPending}
                onClick={() => setIsCalendarOpen((value) => !value)}
              >
                <CalendarDays aria-hidden="true" />
                <span id={`${targetId}-date-value`}>
                  {deadline ? longDay(deadline) : "—"}
                </span>
              </button>
              {touched.date && dateError && deadline && (
                <p className={s.fieldError} role="alert">
                  {dateError}
                </p>
              )}
            </div>
          </div>
          {isCalendarOpen && (
            <Calendar
              value={deadline}
              schedule={schedule}
              onChoose={(ymd) => {
                setDeadline(ymd);
                setTouched((current) => ({ ...current, date: true }));
                setIsCalendarOpen(false);
                dateButton.current?.focus();
              }}
              onClose={() => {
                setIsCalendarOpen(false);
                dateButton.current?.focus();
              }}
            />
          )}
          <div>
            <span className={s.fieldLabel} id={typeId}>
              Prediction type
            </span>
            <button
              ref={typeButton}
              type="button"
              className={s.dateButton}
              aria-haspopup="listbox"
              aria-expanded={isTypeOpen}
              aria-labelledby={`${typeId} ${typeId}-value`}
              disabled={source.publishPending}
              onClick={() => setIsTypeOpen((value) => !value)}
            >
              <span id={`${typeId}-value`} className={s.plainInput}>
                {CONDITIONS.find((entry) => entry.value === condition)?.label}
              </span>
              <ChevronDown aria-hidden="true" />
            </button>
            {isTypeOpen && (
              <ul
                className={s.results}
                role="listbox"
                aria-labelledby={typeId}
                onKeyDown={(event) => {
                  if (event.key === "Escape") {
                    event.preventDefault();
                    event.stopPropagation();
                    closeType(true);
                  }
                }}
                style={{ marginTop: 6 }}
              >
                {CONDITIONS.map((entry, at) => (
                  <li key={entry.value}>
                    <button
                      type="button"
                      role="option"
                      aria-selected={entry.value === condition}
                      className={s.result}
                      style={{ gridTemplateColumns: "auto minmax(0,1fr)" }}
                      autoFocus={
                        entry.value === condition || (at === 0 && !condition)
                      }
                      onKeyDown={(event) => {
                        if (
                          event.key !== "ArrowDown" &&
                          event.key !== "ArrowUp"
                        )
                          return;
                        event.preventDefault();
                        const sibling =
                          event.currentTarget.parentElement?.[
                            event.key === "ArrowDown"
                              ? "nextElementSibling"
                              : "previousElementSibling"
                          ];
                        sibling?.querySelector("button")?.focus();
                      }}
                      onClick={() => {
                        setCondition(entry.value);
                        closeType(true);
                      }}
                    >
                      {entry.value === condition ? (
                        <Check aria-hidden="true" />
                      ) : (
                        <span style={{ width: 26 }} />
                      )}
                      <span className={s.resultName}>{entry.label}</span>
                    </button>
                  </li>
                ))}
              </ul>
            )}
          </div>
          {price?.state === "checking" ? (
            <p className={s.checkingLine} role="status">
              <span className={s.smallSpinner} aria-hidden="true" />
              Checking price…
            </p>
          ) : price?.state === "failed" ? (
            <p className={s.blockedLine} role="status">
              Couldn’t check the price.
            </p>
          ) : (
            <p className={s.closesNote}>
              Betting closes {longDay(utcDay(schedule.closesAt))} · UTC
            </p>
          )}
          {source.commandFailure && (
            <div className={s.notice}>
              <FeedbackMessages
                label="Create outcome"
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
          <ActionButton
            className={s.wide}
            pending={source.publishPending}
            disabled={!canCreate}
            onClick={() => {
              setTouched({ target: true, date: true });
              if (!canCreate || !card || !deadline) return;
              source.publish({
                card,
                targetUsd: target,
                condition,
                deadline,
                schedule,
              });
            }}
          >
            Create prediction
          </ActionButton>
        </div>
      )}
    </Dialog>
  );
}

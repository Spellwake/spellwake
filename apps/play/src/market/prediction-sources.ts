/**
 * Market Predictions: the shape both sources supply to the screens, and the
 * pure rules the screens follow. A prediction asks whether one exact printing
 * and finish reaches a dollar price by (or is at least that price on) an end
 * date. Bets are whole coins on Yes or No; one accepted bet per prediction.
 */

export type Side = "yes" | "no";
export type Condition = "reached_by" | "at_deadline";
export type Outcome = "yes" | "no" | "void";

/** One exact printing and finish, as a card row names it. */
export interface PrintingLabel {
  printingId: string;
  name: string;
  setCode: string | null;
  finish: string;
  faceUrl: string | null;
}

/**
 * The viewer's own bet. "none" only when a complete read proves there is
 * none; "unknown" while that read is loading or after it failed; "unverified"
 * for records that do not make one bet, so no single result is claimed.
 */
export type MyBet =
  | { kind: "none" }
  | { kind: "unknown" }
  | { kind: "unverified" }
  | { kind: "bet"; side: Side; cost: number; win: number };

export interface Prediction {
  id: string;
  card: PrintingLabel;
  targetUsd: string;
  condition: Condition;
  /** The end date, a UTC calendar date (YYYY-MM-DD). */
  deadline: string;
  /** When betting closes, an instant. */
  closesAt: string;
  state: "open" | "closed" | "resolved";
  accepting: boolean;
  outcome: Outcome | null;
  /** The dated price a Yes or No rests on; null when it cannot be read. */
  evidence: { date: string; usd: string } | null;
  /** Why a void prediction returned its bets, when the source says. */
  voidReason: string | null;
  mine: MyBet;
  authored: boolean;
}

export type Phase = "betting" | "awaiting" | "resolved";
/** Betting while the prediction still accepts bets; then awaiting until it resolves. */
export const phaseOf = (prediction: Prediction): Phase =>
  prediction.state === "resolved"
    ? "resolved"
    : prediction.state === "open" && prediction.accepting
      ? "betting"
      : "awaiting";

/** A card's action: a new bet only while betting and no bet is already placed. */
export const entryOf = (prediction: Prediction): "bet" | "view" =>
  phaseOf(prediction) === "betting" &&
  prediction.mine.kind !== "bet" &&
  prediction.mine.kind !== "unverified"
    ? "bet"
    : "view";

const MONTHS = [
  "Jan",
  "Feb",
  "Mar",
  "Apr",
  "May",
  "Jun",
  "Jul",
  "Aug",
  "Sep",
  "Oct",
  "Nov",
  "Dec",
];
const parts = (ymd: string) => {
  const [year, month, day] = ymd.split("-").map(Number);
  return { year: year!, month: month!, day: day! };
};
/** "Jan 31". */
export const shortDay = (ymd: string): string => {
  const { month, day } = parts(ymd);
  return `${MONTHS[month - 1]} ${day}`;
};
/** "Jan 31, 2027". */
export const longDay = (ymd: string): string =>
  `${shortDay(ymd)}, ${parts(ymd).year}`;
/** The UTC calendar date of an instant. */
export const utcDay = (instant: string | Date): string =>
  (typeof instant === "string" ? new Date(instant) : instant)
    .toISOString()
    .slice(0, 10);
export const addDays = (ymd: string, days: number): string =>
  utcDay(new Date(Date.parse(`${ymd}T00:00:00Z`) + days * 86_400_000));

/** A dollar amount as the approved cards write it: "$4", "$3.50", "$0.125". */
export function usd(text: string): string {
  const [whole = "0", fraction = ""] = text.split(".");
  const trimmed = fraction.replace(/0+$/, "");
  const wholePart = whole.replace(/^0+(?=\d)/, "");
  if (!trimmed) return `$${wholePart}`;
  return `$${wholePart}.${trimmed.padEnd(2, "0")}`;
}

/** The question in words, from the exact card and terms. */
export function questionOf(prediction: {
  card: { name: string };
  targetUsd: string;
  condition: Condition;
  deadline: string;
}): string {
  const price = usd(prediction.targetUsd);
  const day = shortDay(prediction.deadline);
  return prediction.condition === "reached_by"
    ? `Will ${prediction.card.name} reach ${price} by ${day}?`
    : `Will ${prediction.card.name} be at least ${price} on ${day}?`;
}

/** "M11 · Nonfoil". */
export const printingLine = (card: PrintingLabel): string =>
  [card.setCode, card.finish].filter(Boolean).join(" · ");

/** Coins a resolved bet returned: the fixed win, nothing, or its cost back on a void. */
export function returnedOf(
  bet: Extract<MyBet, { kind: "bet" }>,
  outcome: Outcome,
): number {
  return outcome === "void" ? bet.cost : outcome === bet.side ? bet.win : 0;
}

/** The status a personal bet row shows. */
export function betStatus(
  prediction: Prediction,
  bet: Extract<MyBet, { kind: "bet" }>,
): "Active" | "Awaiting result" | "Won" | "Lost" | "Void" {
  const phase = phaseOf(prediction);
  if (phase === "betting") return "Active";
  if (phase === "awaiting" || !prediction.outcome) return "Awaiting result";
  if (prediction.outcome === "void") return "Void";
  return prediction.outcome === bet.side ? "Won" : "Lost";
}

/** The status an authored question shows: the question's, never the creator's winnings. */
export function questionStatus(
  prediction: Prediction,
): "Betting open" | "Awaiting result" | "Yes" | "No" | "Void" {
  const phase = phaseOf(prediction);
  if (phase === "betting") return "Betting open";
  if (phase === "awaiting" || !prediction.outcome) return "Awaiting result";
  return prediction.outcome === "yes"
    ? "Yes"
    : prediction.outcome === "no"
      ? "No"
      : "Void";
}

/** Search by card name, set code or the question itself. */
export function matchPredictions(
  predictions: readonly Prediction[],
  query: string,
): Prediction[] {
  const needle = query.trim().toLowerCase();
  if (!needle) return [...predictions];
  return predictions.filter((prediction) =>
    [
      prediction.card.name,
      prediction.card.setCode ?? "",
      questionOf(prediction),
    ]
      .join(" ")
      .toLowerCase()
      .includes(needle),
  );
}

/** A bet is a positive whole number of coins the wallet can cover. */
export function amountProblem(
  text: string,
  balance: number | null,
): string | null {
  const value = text.trim();
  if (value === "" || /^0+$/.test(value)) return "Enter an amount.";
  if (!/^\d+$/.test(value)) return "Enter a whole number.";
  if (balance !== null && Number(value) > balance) return "Not enough coins.";
  return null;
}

/** A target is a positive exact decimal of dollars, cents or finer. */
export function targetProblem(text: string): string | null {
  const value = text.trim().replace(/^\$/, "");
  return /^\d+(\.\d+)?$/.test(value) && /[1-9]/.test(value)
    ? null
    : "Enter a price above $0.";
}
/** The canonical decimal a valid target is sent as. */
export const canonicalTarget = (text: string): string =>
  text
    .trim()
    .replace(/^\$/, "")
    .replace(/^0+(?=\d)/, "");

/** Betting closes seven days after creation; the end date follows the first full day after. */
export interface Schedule {
  closesAt: string;
  earliest: string;
  latest: string;
}
export const BETTING_DAYS = 7;
export function scheduleFrom(now: Date): Schedule {
  const closes = new Date(
    Math.floor(now.getTime() / 1000) * 1000 + BETTING_DAYS * 86_400_000,
  );
  const earliest = addDays(utcDay(closes), 1);
  return {
    closesAt: closes.toISOString().replace(".000Z", "Z"),
    earliest,
    latest: addDays(earliest, 364),
  };
}
export function dateProblem(ymd: string | null, schedule: Schedule) {
  if (!ymd) return "Choose an end date.";
  if (ymd < schedule.earliest)
    return `Choose ${shortDay(schedule.earliest)} or later.`;
  if (ymd > schedule.latest)
    return `Choose ${longDay(schedule.latest)} or earlier.`;
  return null;
}
/** The exact publication instants for an end date under a schedule. */
export function publicationTimes(ymd: string, schedule: Schedule) {
  return {
    tradingClosesAt: schedule.closesAt,
    observationStartsAt: new Date(Date.parse(schedule.closesAt) + 1000)
      .toISOString()
      .replace(".000Z", "Z"),
    deadline: `${ymd}T23:59:59Z`,
  };
}

/** What one side's fresh terms say for the entered amount. */
export type SideQuote =
  | { kind: "checking" }
  | { kind: "ready"; win: number; revision: string }
  | { kind: "expired" }
  | { kind: "failed" }
  | { kind: "unavailable" }
  | { kind: "too-much" };
export interface Quote {
  predictionId: string;
  amount: number;
  yes: SideQuote;
  no: SideQuote;
}
/** Why betting cannot start at all, before any amount. */
export type BetBlock = "closed" | "unavailable" | "account" | "not-accepting";
export const BET_BLOCK_COPY: Record<BetBlock, string> = {
  closed: "Betting closed.",
  unavailable: "Prediction unavailable.",
  account: "Betting unavailable for your account.",
  "not-accepting": "This prediction is not accepting bets.",
};

export interface Group {
  items: readonly Prediction[];
  state: "loading" | "ready" | "failed" | "unavailable";
}
export interface CardResults {
  items: readonly PrintingLabel[];
  state: "idle" | "loading" | "ready" | "failed";
  isAppending: boolean;
  hasMore: boolean;
  loadMore: () => void;
}
export type PriceCheck =
  | { printingId: string; state: "checking" }
  | { printingId: string; state: "ready" }
  | { printingId: string; state: "unavailable" }
  | { printingId: string; state: "failed" };
export interface PredictionDraft {
  card: PrintingLabel;
  targetUsd: string;
  condition: Condition;
  deadline: string;
  schedule: Schedule;
}
export type Completed =
  { kind: "bet" } | { kind: "created"; predictionId: string };

export interface ReadyPredictions {
  status: "ready";
  balance: number | null;
  explore: {
    items: readonly Prediction[];
    state: "loading" | "ready" | "failed";
    isAppending: boolean;
    hasMore: boolean;
    loadMore: () => void;
  };
  mine: { bets: Group; created: Group };
  /** A read failed: the passive notice. Known content stays. */
  failure: string | null;
  dismissFailure: () => void;
  /** Ask for fresh terms on both sides for an amount; null stops asking. */
  quoteFor: (predictionId: string | null, amount: number) => void;
  quote: Quote | null;
  /** Why no bet can start here, whatever the prediction. */
  betBlock: BetBlock | null;
  placeBet: (
    predictionId: string,
    side: Side,
    amount: number,
    revision: string,
  ) => void;
  betPending: boolean;
  searchCards: (query: string | null) => void;
  cards: CardResults;
  checkPrice: (printingId: string | null) => void;
  price: PriceCheck | null;
  publish: (draft: PredictionDraft) => void;
  publishPending: boolean;
  /** A command failed: the passive notice. The same action can be pressed again. */
  commandFailure: string | null;
  dismissCommandFailure: () => void;
  completed: Completed | null;
  dismissCompleted: () => void;
}
export type PredictionsSource =
  | { status: "loading" }
  | { status: "signed-out" }
  | { status: "unregistered" }
  | { status: "unavailable" }
  | ReadyPredictions;

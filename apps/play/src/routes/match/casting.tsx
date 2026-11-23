import {
  useEffect,
  useLayoutEffect,
  useMemo,
  useRef,
  useState,
  type ReactElement,
} from "react";
import { AnimatePresence, motion, useReducedMotion } from "motion/react";
import type { PlayerObject } from "@spellwake/application-contracts";
import {
  ActionButton,
  ManaCost,
  OriginalCardFace,
  PrintedModePicker,
  type PrintedMode,
} from "@spellwake/ui";
import { manaSymbolUrl } from "../../data/builder-catalogue";
import {
  adjustShare,
  backCast,
  beginCast,
  canAddShare,
  canConfirmModes,
  canConfirmShares,
  canGoBack,
  canShareMinus,
  canSharePlus,
  castSteps,
  castTargetKey,
  castTargets,
  chooseCastMode,
  chooseCastTarget,
  confirmCastModes,
  confirmShares,
  costSymbols,
  modeCount,
  offerFor,
  payCast,
  reconcileCast,
  sharedOut,
  toggleShare,
  type CastDraft,
  type CastOffer,
  type CastStep,
  type CastTarget,
} from "../../match/cast-draft";
import type { ReadyMatch } from "../../match/match-sources";
import type {
  PrintedModeRegion,
  PrintedModes,
} from "../../match/printed-modes";
import { useAnchors } from "./anchors";
import type { CardMark, CombatUi, SeatMark } from "./combat";
import type { DamagePill } from "./damage";
import { usePilePlace } from "./stack";
import * as s from "./match-table.css";

export interface Casting {
  /** The draft's marks and actions, while one waits for the player. */
  ui: CombatUi | null;
  /** Hand cards the game offers to cast, rimmed as playable. */
  playable: ReadonlySet<string>;
  /** Start casting a card. False when the game does not offer it. */
  begin: (handle: string) => boolean;
  /** Players the waiting spell may be given, or has been given, chosen by their portrait. */
  targetSeats: ReadonlySet<string>;
  chooseSeat: (seatId: string) => void;
  /** Spells on the stack the waiting spell may target, chosen on the pile. */
  stackTargets: ReadonlySet<string>;
  chooseStack: (handle: string) => void;
  /** The spell being cast, shown in the pile until the game accepts it. */
  pending: { source: string; target: CastTarget | null } | null;
  /**
   * The spell being cast on its own complete face, with the sentences it is
   * following outlined. Shown instead of the pile's copy where the face and
   * its printed regions are known.
   */
  sourceFace: CastSourceView | null;
  prompt:
    | {
        kind: "target";
        text: string;
        /** Amounts stand above the cards, so the words keep clear of them. */
        raised?: boolean;
      }
    | { kind: "payment"; cost: string }
    | null;
  /** A modal spell waiting for its modes, chosen on its own face. */
  modeChoice: ModeChoiceView | null;
  /** A divided spell's amounts, one beside each chosen target. */
  pills: readonly DamagePill[];
  /** A divided spell's shares are being sent after its cast. */
  dividing: boolean;
}

/** The original face of a card and where its modes or instructions are printed. */
export interface ModalFace {
  name: string;
  src: string;
  printed: PrintedModes;
}
export interface ModeChoiceView {
  face: ModalFace;
  modes: readonly PrintedMode[];
  /** How many modes the spell asks for. */
  count: number;
  /** The modes marked so far, for a spell that asks for several. */
  selected: readonly string[];
  /** Press a mode: it is chosen, or marked and unmarked. */
  onChoose: (mode: string) => void;
  /** Go on once enough are marked. Null for a spell that asks for one. */
  next: { enabled: boolean; onActivate: () => void } | null;
  onCancel: () => void;
  /** Hide the choice to look at the table; the draft is kept. */
  onPeek: () => void;
}
export interface CastSourceView {
  face: ModalFace;
  highlights: readonly {
    id: string;
    region: PrintedModeRegion;
    tone: "available" | "selected" | "earlier";
  }[];
  /** A line from a sentence to what was chosen for it. */
  links: readonly { id: string; region: PrintedModeRegion; to: string }[];
}

/** A screen too short to keep words clear of the amounts over the table: a phone on its side. */
const SHORT = "(max-height: 480px)";
function useShortScreen(): boolean {
  const [short, setShort] = useState(
    () => typeof window !== "undefined" && window.matchMedia(SHORT).matches,
  );
  useEffect(() => {
    const query = window.matchMedia(SHORT);
    const update = () => setShort(query.matches);
    update();
    query.addEventListener("change", update);
    return () => query.removeEventListener("change", update);
  }, []);
  return short;
}

const NONE: ReadonlySet<string> = new Set();
const NO_PILLS: readonly DamagePill[] = [];
const anchorOf = (target: CastTarget): string =>
  target.kind === "seat"
    ? `seat:${target.seatId}`
    : target.kind === "stack"
      ? `stack:${target.handle}`
      : `card:${target.handle}`;

/** The printed region a step's target belongs to, where the face is known. */
function regionOf(
  offer: CastOffer,
  face: ModalFace,
  of: ReturnType<typeof castSteps>[number]["of"],
): PrintedModeRegion | undefined {
  if (!of) return undefined;
  const index =
    of.kind === "slot"
      ? of.slot
      : (offer.modes?.findIndex((mode) => mode.mode === of.mode) ?? -1);
  return face.printed.regions[index];
}

/**
 * Casting from hand, on the table itself: offered cards are rimmed, a modal
 * spell's modes are chosen on its face, each thing it targets is chosen in
 * printed order with Back to the one before, a divided amount is shared out
 * beside its targets, and a cost the game pauses for shows Pay with Auto Pay.
 * The last choice sends the cast; a refusal puts the draft back exactly as it
 * was.
 */
export function useCasting(
  source: ReadyMatch,
  /** Nothing else is being declared, and the player may act. */
  free: boolean,
  /** A card's original face and printed regions, where they are known. */
  modalFace: (handle: string) => ModalFace | null,
  /** What a target is called, for the names of its controls. */
  describe: (target: CastTarget) => string,
): Casting {
  const { castOffers, sendAction, commandPending, commandError, snapshot } =
    source;
  // A modal spell is offered here only when its modes can be shown on its face.
  const offers = (free ? (castOffers ?? []) : []).filter((entry) => {
    const printed = entry.modes ? modalFace(entry.source)?.printed : null;
    return !entry.modes || (printed?.regions.length ?? 0) >= entry.modes.length;
  });
  const offered = offers.map((entry) => entry.source).join("|");
  // Looking at the table while a mode is still to be chosen.
  const [peeking, setPeeking] = useState(false);
  const [draft, setDraft] = useState<CastDraft | null>(null);
  // While a cast is with the game: what to put back if it is refused.
  const [sent, setSent] = useState<{
    origin: CastDraft | null;
    settled: boolean;
  } | null>(null);
  // A divided spell's shares, owed to the game once it asks for them.
  const [owed, setOwed] = useState<NonNullable<
    Extract<CastStep, { kind: "submit" }>["division"]
  > | null>(null);

  useEffect(() => {
    if (!sent?.settled || commandPending) return;
    setDraft(commandError ? sent.origin : null);
    if (commandError) setOwed(null);
    setSent(null);
  }, [sent, commandPending, commandError]);
  // A draft the game no longer offers is dropped; nothing is chosen for it.
  useEffect(() => {
    if (sent) return;
    const kept = reconcileCast(offers, draft);
    if (kept !== draft) setDraft(kept);
    // `offered` and the offers' own contents stand for the list.
  }, [castOffers, offered, draft, sent]);
  useEffect(() => {
    if (draft?.stage !== "mode") setPeeking(false);
  }, [draft?.stage]);

  // The game asks how a divided spell is shared once it is cast: the shares
  // already chosen answer it, so the player is not asked twice.
  const asked = owed
    ? snapshot.decisions.find(
        (entry) =>
          entry.domain.kind === "distribution" &&
          entry.domain.targets.length === owed.length &&
          owed.every((share) =>
            entry.domain.kind === "distribution"
              ? entry.domain.targets.some(
                  (target) =>
                    castTargetKey(target) === castTargetKey(share.target),
                )
              : false,
          ),
      )
    : undefined;
  const askedKey = asked?.decision ?? null;
  useLayoutEffect(() => {
    if (!askedKey || !owed || commandPending) return;
    setOwed(null);
    void sendAction({
      kind: "choose",
      decision: askedKey,
      response: { kind: "distribution", allocations: [...owed] },
    });
    // Sent once for each question.
  }, [askedKey, commandPending]);
  // Shares the game never asked for are not kept once the player may act again.
  useEffect(() => {
    if (owed && free && !sent && !commandPending) setOwed(null);
  }, [owed, free, sent, commandPending]);

  const apply = (step: CastStep, origin: CastDraft | null): boolean => {
    if (step.kind === "ignored") return false;
    setDraft(step.draft);
    if (step.kind === "submit") {
      setSent({ origin, settled: false });
      setOwed(step.division ?? null);
      void sendAction(step.action).finally(() =>
        setSent((current) => current && { ...current, settled: true }),
      );
    }
    return true;
  };

  const short = useShortScreen();
  const sending = sent !== null;
  const offer = draft ? offerFor(castOffers ?? [], draft.source) : undefined;
  const targets = sending ? [] : castTargets(offers, draft);
  const dividing = draft?.stage === "divide" && !sending;
  const press = (target: CastTarget): boolean => {
    if (!draft || sending) return false;
    return draft.stage === "divide"
      ? apply(toggleShare(offers, draft, target), draft)
      : apply(chooseCastTarget(offers, draft, target), draft);
  };

  let ui: CombatUi | null = null;
  // A spell sent with nothing to choose shows no draft: the turn action carries its wait.
  const unseen = sent !== null && sent.origin === null;
  const choosingMode = draft?.stage === "mode" && !sending;
  const shown = draft && offer && !unseen && !choosingMode;
  const cancelDraft = () => setDraft(null);
  const face = draft ? modalFace(draft.source) : null;
  const steps = draft && offer ? castSteps(offer, draft.modes) : [];
  const step = draft?.stage === "target" ? steps[draft.step] : undefined;
  if (choosingMode && peeking) {
    // The table is in view; nothing on it can be played until the choice is made.
    ui = {
      marks: new Map<string, CardMark>([[draft.source, "gold"]]),
      seatMarks: new Map<string, SeatMark>(),
      lines: [],
      onCard: () => false,
      onSeat: () => false,
      primary: {
        label: "Return to choice",
        onActivate: () => setPeeking(false),
      },
      secondary: { label: "Cancel", onActivate: cancelDraft },
      status: null,
    };
  }
  // What a divided spell may still be given, and what it has.
  const addable =
    dividing && offer?.divide
      ? offer.divide.targets.filter((target) =>
          canAddShare(offers, draft, target),
        )
      : [];
  if (draft && shown) {
    const marks = new Map<string, CardMark>([[draft.source, "gold"]]);
    const seatMarks = new Map<string, SeatMark>();
    const paying = draft.stage === "payment";
    // Choices already made, and one gone back to, are ivory until replaced.
    draft.targets.forEach((target, index) => {
      if (!target || (!paying && index > draft.step)) return;
      if (target.kind === "object") marks.set(target.handle, "ivory");
      if (target.kind === "seat") seatMarks.set(target.seatId, "marked");
    });
    for (const target of [
      ...targets,
      ...addable,
      ...draft.shares.map((share) => share.target),
    ]) {
      if (target.kind === "object") marks.set(target.handle, "teal");
      if (target.kind === "seat") seatMarks.set(target.seatId, "target");
    }
    // The sources an automatic payment would use are shown, not tapped.
    if (paying) for (const land of offer.paySources) marks.set(land, "gold");
    const cancel = {
      label: "Cancel",
      disabled: sending,
      onActivate: () => setDraft(null),
    };
    ui = {
      marks,
      seatMarks,
      lines: [],
      onCard: (object) => press({ kind: "object", handle: object.handle }),
      // A player is chosen by their portrait; the rest of the chip still moves the camera.
      onSeat: () => false,
      ...(paying
        ? {
            layout: "payment" as const,
            primary: {
              label: "Auto Pay",
              pending: sending,
              onActivate: () => apply(payCast(offers, draft), draft),
            },
            secondary: cancel,
          }
        : draft.stage === "divide"
          ? {
              primary: {
                label: "Confirm",
                disabled: !canConfirmShares(offers, draft),
                pending: sending,
                onActivate: () => apply(confirmShares(offers, draft), draft),
              },
              secondary: cancel,
            }
          : {
              primary: cancel,
              // The first target has none before it: Cancel is the way out.
              secondary:
                canGoBack(draft) && !sending
                  ? {
                      label: "Back",
                      onActivate: () => apply(backCast(draft), draft),
                    }
                  : null,
            }),
      // A short screen has no clear place over the table for a divided
      // amount's words, so they stand with the actions instead.
      status:
        short && draft.stage === "divide" && offer.divide
          ? `${offer.divide.ask} · ${sharedOut(draft)}/${offer.divide.total}`
          : null,
    };
  }

  const sourceFace = useMemo((): CastSourceView | null => {
    if (!draft || !offer || !shown || !face) return null;
    if (!offer.modes && !offer.slots && !offer.divide) return null;
    const paying = draft.stage === "payment";
    const several = steps.length > 1;
    const highlights: CastSourceView["highlights"][number][] = [];
    const links: CastSourceView["links"][number][] = [];
    const stepOf = new Map<number, number>();
    steps.forEach((entry, index) => {
      const region = regionOf(offer, face, entry.of);
      if (!region) return;
      const target = draft.targets[index];
      const done = paying || index < draft.step;
      const current = !paying && index === draft.step;
      if (entry.of?.kind === "mode") stepOf.set(entry.of.mode, index);
      if (entry.of?.kind === "slot" && (done || current))
        highlights.push({
          id: `slot:${index}`,
          region,
          tone: current ? "available" : "earlier",
        });
      // A choice already made, or one gone back to, keeps its line.
      if (target && (done || current))
        links.push({ id: `link:${index}`, region, to: anchorOf(target) });
    });
    offer.modes?.forEach((mode, index) => {
      const region = face.printed.regions[index];
      if (!region || !draft.modes.includes(mode.mode)) return;
      const at = stepOf.get(mode.mode);
      const tone =
        !several || at === undefined || paying
          ? "selected"
          : at === draft.step
            ? "available"
            : at < draft.step
              ? "earlier"
              : "selected";
      highlights.push({ id: `mode:${mode.mode}`, region, tone });
    });
    return { face, highlights, links };
    // The draft and offer carry everything the face shows.
  }, [draft, offer, shown, face?.src]);

  const pills = useMemo((): readonly DamagePill[] => {
    if (!draft || !shown || draft.stage !== "divide" || !offer?.divide)
      return NO_PILLS;
    return draft.shares.map((share) => {
      const change = (by: -1 | 1) => () =>
        void apply(adjustShare(offers, draft, share.target, by), draft);
      return {
        key: castTargetKey(share.target),
        anchor: anchorOf(share.target),
        name: describe(share.target),
        caption: null,
        amount: share.amount,
        minus:
          !sending && canShareMinus(offers, draft, share.target)
            ? change(-1)
            : null,
        plus:
          !sending && canSharePlus(offers, draft, share.target)
            ? change(1)
            : null,
      };
    });
    // The draft and the offers decide every pill.
  }, [draft, shown, offered, castOffers, sending]);

  const seatsOf = (list: readonly CastTarget[]) =>
    list.flatMap((target) => (target.kind === "seat" ? [target.seatId] : []));
  return {
    ui,
    playable: useMemo(
      () => (draft || sent ? NONE : new Set(offered ? offered.split("|") : [])),
      [offered, draft, sent],
    ),
    begin: (handle) =>
      !draft && !sent && apply(beginCast(offers, handle), null),
    targetSeats: useMemo(
      () =>
        new Set(
          seatsOf([
            ...targets,
            ...addable,
            ...(dividing && draft ? draft.shares.map((s) => s.target) : []),
          ]),
        ),
      // The targets are a function of these.
      [castOffers, offered, draft, sending],
    ),
    chooseSeat: (seatId) => void press({ kind: "seat", seatId }),
    stackTargets: useMemo(
      () =>
        new Set(
          targets.flatMap((target) =>
            target.kind === "stack" ? [target.handle] : [],
          ),
        ),
      [castOffers, offered, draft, sending],
    ),
    chooseStack: (handle) => void press({ kind: "stack", handle }),
    pending:
      draft && shown && !sourceFace
        ? { source: draft.source, target: draft.targets[0] ?? null }
        : null,
    sourceFace,
    prompt:
      !draft || !shown
        ? null
        : draft.stage === "payment"
          ? { kind: "payment", cost: offer.cost }
          : draft.stage === "divide" && offer.divide
            ? {
                kind: "target",
                text: `${offer.divide.ask} · ${sharedOut(draft)}/${offer.divide.total}`,
                raised: true,
              }
            : {
                kind: "target",
                text:
                  steps.length > 1 && step
                    ? `${step.ask} · ${draft.step + 1}/${steps.length}`
                    : (step?.ask ?? "Choose a target"),
              },
    modeChoice:
      choosingMode && !peeking && draft && offer?.modes && face
        ? {
            face,
            modes: offer.modes.map((mode, index) => ({
              id: String(mode.mode),
              label: mode.label,
              region: face.printed.regions[index]!,
              ...(mode.unavailableReason
                ? { unavailableReason: mode.unavailableReason }
                : {}),
            })),
            count: modeCount(offer),
            selected: draft.modes.map(String),
            onChoose: (mode) =>
              apply(chooseCastMode(offers, draft, Number(mode)), draft),
            next:
              modeCount(offer) > 1
                ? {
                    enabled: canConfirmModes(offers, draft),
                    onActivate: () =>
                      apply(confirmCastModes(offers, draft), draft),
                  }
                : null,
            onCancel: cancelDraft,
            onPeek: () => setPeeking(true),
          }
        : null,
    pills,
    dividing: owed !== null,
  };
}

/** How the picker's faces are laid out: their width in design pixels, and how many to a row. */
function useFaceLayout(
  count: number,
  gap: number,
): { width: number; columns: number } {
  const [layout, setLayout] = useState({ width: 316, columns: count });
  useLayoutEffect(() => {
    const measure = () => {
      const density =
        parseFloat(getComputedStyle(document.documentElement).fontSize) / 16;
      // The heading above and the actions beneath are drawn in real pixels,
      // and smaller on a short screen, so the room left is measured the same
      // way and turned into design pixels at the end.
      const chrome = window.innerHeight < 480 ? 150 : 262;
      const fit = (columns: number) => {
        const rows = Math.ceil(count / columns);
        const byHeight =
          (window.innerHeight - chrome - (rows - 1) * (gap + 26)) / rows / 1.4;
        const byWidth =
          (window.innerWidth - 40 - gap * (columns - 1)) / columns;
        // Two rows may scroll rather than draw faces too small to press.
        const least = rows > 1 ? 124 : 0;
        return Math.min(316 * density, byWidth, Math.max(byHeight, least));
      };
      // Four faces stand in one row where they fit, and in two rows on a
      // narrow screen, whichever draws them larger.
      const columns =
        count > 2 && fit(Math.ceil(count / 2)) > fit(count)
          ? Math.ceil(count / 2)
          : count;
      const width = Math.round(Math.max(80, fit(columns) / density));
      setLayout((current) =>
        current.width === width && current.columns === columns
          ? current
          : { width, columns },
      );
    };
    measure();
    window.addEventListener("resize", measure);
    return () => window.removeEventListener("resize", measure);
  }, [count, gap]);
  return layout;
}

const COUNT_WORDS = ["none", "one", "two", "three", "four", "five"];

/**
 * Choosing a modal spell's modes, as approved (MC-01, MC-02, MM-01 to
 * MM-05): the complete original card once for each mode, with that mode's
 * own paragraph outlined. A spell that asks for one goes straight on to its
 * targets. A spell that asks for several marks each in gold, unmarks one
 * pressed again, and goes on with Next once exactly enough are marked.
 * View Battlefield hides the choice without ending it; Cancel ends the cast.
 */
export function ModeChoice({
  choice,
}: {
  choice: ModeChoiceView | null;
}): ReactElement {
  const dialog = useRef<HTMLDialogElement>(null);
  const gap = 30;
  const layout = useFaceLayout(choice?.modes.length ?? 2, gap);
  const open = choice !== null;
  useEffect(() => {
    const node = dialog.current;
    if (!node) return;
    if (open && !node.open) node.showModal();
    if (!open && node.open) node.close();
  }, [open]);
  const heading = `Choose ${COUNT_WORDS[choice?.count ?? 1] ?? choice?.count}`;
  return (
    <dialog
      ref={dialog}
      className={s.modeDialog}
      aria-label={heading}
      // Escape is not a way out of a cast: only Cancel is.
      onCancel={(event) => event.preventDefault()}
    >
      {choice && (
        <div className={s.modeScreen} data-mode-count={choice.count}>
          <div className={s.modeTop}>
            <button
              type="button"
              className={s.modePeek}
              onClick={choice.onPeek}
            >
              View Battlefield
            </button>
          </div>
          <h1 className={s.modeHeading}>{heading}</h1>
          <div className={s.modeFaces}>
            <PrintedModePicker
              image={{
                src: choice.face.src,
                name: choice.face.name,
                width: choice.face.printed.width,
                height: choice.face.printed.height,
              }}
              modes={choice.modes}
              faceWidth={layout.width}
              columns={layout.columns}
              gap={gap}
              choice={
                choice.next
                  ? {
                      kind: "multiple",
                      count: choice.count,
                      selected: choice.selected,
                      // The picker reports the whole set; one mode differs.
                      onChange: (ids) => {
                        const changed =
                          ids.find((id) => !choice.selected.includes(id)) ??
                          choice.selected.find((id) => !ids.includes(id));
                        if (changed !== undefined) choice.onChoose(changed);
                      },
                    }
                  : { kind: "one", onChoose: choice.onChoose }
              }
            />
          </div>
          <div className={s.modeFooter}>
            <ActionButton onClick={choice.onCancel}>Cancel</ActionButton>
            {choice.next && (
              <ActionButton
                className={s.modeNext}
                disabled={!choice.next.enabled}
                onClick={choice.next.onActivate}
              >
                Next
              </ActionButton>
            )}
          </div>
        </div>
      )}
    </dialog>
  );
}

/** The face beside the table, in the pile's own design pixels. */
const FACE = { width: 230, height: (230 * 680) / 488 };

/**
 * The spell being cast, as approved (MM-06, MT-01 to MT-05, DC-01): its
 * complete original face beside the table, the sentence being followed in
 * cyan, a chosen mode in gold, and a choice already made in ivory with one
 * quiet line to the card, player or spell chosen for it. It shows what was
 * chosen and is not itself pressed.
 */
export function CastSource({
  view,
  stackCount,
  follow,
  clearBottom = 0,
}: {
  view: CastSourceView | null;
  /** How many items the pile holds: the face stands to its left. */
  stackCount: number;
  /** Changes whenever the cards may have moved. */
  follow: unknown;
  clearBottom?: number;
}): ReactElement {
  const root = useRef<HTMLDivElement>(null);
  const reduced = useReducedMotion();
  const alone = usePilePlace(
    root,
    1,
    clearBottom,
    FACE,
    stackCount > 0 ? "left-upper" : "left",
  );
  const pile = usePilePlace(root, Math.max(stackCount, 1), clearBottom);
  const { rects, width } = useAnchors(
    root,
    view?.links.map((link) => link.to) ?? [],
    `${String(follow)}:${alone?.x}:${alone?.y}:${alone?.scale}:${stackCount}`,
  );
  let place = alone;
  if (alone && pile && stackCount > 0 && !alone.apart) {
    // Beside the pile, whose older cards peek out to its left.
    const peeks = Math.min(stackCount - 1, 5);
    const edge = pile.x - peeks * 21 * pile.scale - 16;
    // On a wide screen the cards reach under where the face would stand,
    // so it is drawn smaller rather than over them.
    const clear =
      width > 700
        ? Math.max((edge - width * 0.64) / FACE.width, alone.scale * 0.55)
        : alone.scale;
    const scale = Math.max(
      0.3,
      Math.min(alone.scale, clear, (edge - 8) / FACE.width),
    );
    place = { scale, x: Math.max(8, edge - FACE.width * scale), y: alone.y };
  }
  const shown = view && place && width > 0 ? { view, place } : null;
  const lines = shown
    ? shown.view.links.flatMap((link) => {
        const rect = rects.get(link.to);
        if (!rect) return [];
        const { x, y, scale } = shown.place;
        const unit = (FACE.width * scale) / shown.view.face.printed.width;
        const top = y + (link.region.y + link.region.height / 2) * unit;
        const centre = rect.left + rect.width / 2;
        // The line leaves the sentence on the side its target is on.
        const leftward = centre < x + (FACE.width * scale) / 2;
        const from = {
          x: x + (link.region.x + (leftward ? 0 : link.region.width)) * unit,
          y: top,
        };
        const to = link.to.startsWith("seat:")
          ? // The portrait is the round picture at the head of the seat's chip.
            { x: rect.left + rect.height / 2, y: rect.bottom - 2 }
          : {
              x: Math.min(Math.max(from.x, rect.left + 5), rect.right - 5),
              y: Math.min(Math.max(from.y, rect.top + 5), rect.bottom - 5),
            };
        const dx = to.x - from.x;
        const dy = to.y - from.y;
        const length = Math.hypot(dx, dy);
        if (length < 2) return [];
        const bend = Math.min(150, length * 0.22) * (leftward ? 1 : -1);
        const cx = (from.x + to.x) / 2 - (dy / length) * bend;
        const cy = (from.y + to.y) / 2 + (dx / length) * bend;
        return [
          {
            id: link.id,
            path: `M ${from.x} ${from.y} Q ${cx} ${cy} ${to.x} ${to.y}`,
            from,
            to,
          },
        ];
      })
    : [];
  return (
    <div ref={root} className={s.sourceLayer} aria-hidden="true">
      <AnimatePresence>
        {shown && (
          <motion.div
            key="face"
            className={s.sourceFace}
            data-cast-source={shown.view.face.name}
            style={{
              x: shown.place.x,
              y: shown.place.y,
              width: FACE.width * shown.place.scale,
            }}
            initial={reduced ? false : { opacity: 0 }}
            animate={{ opacity: 1 }}
            exit={{ opacity: 0 }}
            transition={{
              duration: reduced ? 0 : 0.24,
              ease: [0.22, 1, 0.36, 1],
            }}
          >
            <OriginalCardFace
              image={{
                src: shown.view.face.src,
                name: shown.view.face.name,
                width: shown.view.face.printed.width,
                height: shown.view.face.printed.height,
              }}
              highlights={shown.view.highlights}
            />
          </motion.div>
        )}
      </AnimatePresence>
      {lines.length > 0 && (
        <svg className={s.sourceLines} focusable="false">
          {lines.map((line) => (
            <g key={line.id} data-cast-link={line.id}>
              <path d={line.path} />
              <circle cx={line.from.x} cy={line.from.y} r={2.5} />
              <circle
                className={s.sourceLineEnd}
                cx={line.to.x}
                cy={line.to.y}
                r={5}
              />
            </g>
          ))}
        </svg>
      )}
    </div>
  );
}

const SYMBOL_NAMES: Record<string, string> = {
  W: "white",
  U: "blue",
  B: "black",
  R: "red",
  G: "green",
  C: "colourless",
};
const symbolName = (symbol: string): string =>
  SYMBOL_NAMES[symbol]
    ? `one ${SYMBOL_NAMES[symbol]} mana`
    : /^\d+$/.test(symbol)
      ? `${symbol} generic mana`
      : `${symbol} mana`;

/**
 * What the draft waits for, over the table: the target asked for and which
 * of several it is, how much of a divided amount is given out, or Pay with
 * the cost. It names the step only; the cards and portraits carry the choices.
 */
export function CastPrompt({
  prompt,
}: {
  prompt: Casting["prompt"];
}): ReactElement {
  const reduced = useReducedMotion();
  return (
    <AnimatePresence mode="wait">
      {prompt && (
        <motion.div
          key={prompt.kind}
          className={s.castPrompt}
          data-cast-prompt={prompt.kind}
          data-raised={(prompt.kind === "target" && prompt.raised) || undefined}
          initial={reduced ? false : { opacity: 0, y: 6 }}
          animate={{ opacity: 1, y: 0 }}
          exit={{ opacity: 0 }}
          transition={{
            duration: reduced ? 0 : 0.22,
            ease: [0.22, 1, 0.36, 1],
          }}
        >
          {prompt.kind === "target" ? (
            <p role="status">{prompt.text}</p>
          ) : (
            <ManaCost
              label="Pay"
              size="payment"
              symbols={costSymbols(prompt.cost).map((symbol) => ({
                name: symbolName(symbol),
                src: manaSymbolUrl(symbol),
              }))}
            />
          )}
        </motion.div>
      )}
    </AnimatePresence>
  );
}

/** The card a pending cast shows beside the table. */
export const pendingObject = (
  objects: ReadonlyMap<string, PlayerObject>,
  pending: Casting["pending"],
): { object: PlayerObject; target: CastTarget | null } | null => {
  const object = pending ? objects.get(pending.source) : undefined;
  return object && pending ? { object, target: pending.target } : null;
};

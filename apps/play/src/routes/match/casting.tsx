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
  PrintedModePicker,
  type PrintedMode,
} from "@spellwake/ui";
import { manaSymbolUrl } from "../../data/builder-catalogue";
import {
  beginCast,
  castTargets,
  chooseCastMode,
  chooseCastTarget,
  costSymbols,
  offerFor,
  payCast,
  reconcileCast,
  type CastDraft,
  type CastStep,
} from "../../match/cast-draft";
import type { CombatTarget } from "../../match/combat-draft";
import type { ReadyMatch } from "../../match/match-sources";
import type { PrintedModes } from "../../match/printed-modes";
import type { CardMark, CombatUi, SeatMark } from "./combat";
import * as s from "./match-table.css";

export interface Casting {
  /** The draft's marks and actions, while one waits for the player. */
  ui: CombatUi | null;
  /** Hand cards the game offers to cast, rimmed as playable. */
  playable: ReadonlySet<string>;
  /** Start casting a card. False when the game does not offer it. */
  begin: (handle: string) => boolean;
  /** Players the waiting spell may target, chosen by their portrait. */
  targetSeats: ReadonlySet<string>;
  chooseSeat: (seatId: string) => void;
  /** The spell being cast, shown beside the table until the game accepts it. */
  pending: { source: string; target: CombatTarget | null } | null;
  prompt: { kind: "target" } | { kind: "payment"; cost: string } | null;
  /** A modal spell waiting for its mode, chosen on its own face. */
  modeChoice: ModeChoiceView | null;
}

/** The original face of a modal spell and where its modes are printed. */
export interface ModalFace {
  name: string;
  src: string;
  printed: PrintedModes;
}
export interface ModeChoiceView {
  face: ModalFace;
  modes: readonly PrintedMode[];
  onChoose: (mode: string) => void;
  onCancel: () => void;
  /** Hide the choice to look at the table; the draft is kept. */
  onPeek: () => void;
}

const NONE: ReadonlySet<string> = new Set();

/**
 * Casting from hand, on the table itself: offered cards are rimmed, a spell
 * that needs a target waits with Cancel while its targets are rimmed, and a
 * cost the game pauses for shows Pay with Auto Pay. The last choice sends
 * the cast; a refusal puts the draft back exactly as it was.
 */
export function useCasting(
  source: ReadyMatch,
  /** Nothing else is being declared, and the player may act. */
  free: boolean,
  /** A modal card's face and printed mode regions, where they are known. */
  modalFace: (handle: string) => ModalFace | null,
): Casting {
  const { castOffers, sendAction, commandPending, commandError } = source;
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

  useEffect(() => {
    if (!sent?.settled || commandPending) return;
    setDraft(commandError ? sent.origin : null);
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

  const apply = (step: CastStep, origin: CastDraft | null): boolean => {
    if (step.kind === "ignored") return false;
    setDraft(step.draft);
    if (step.kind === "submit") {
      setSent({ origin, settled: false });
      void sendAction(step.action).finally(() =>
        setSent((current) => current && { ...current, settled: true }),
      );
    }
    return true;
  };

  const sending = sent !== null;
  const offer = draft ? offerFor(castOffers ?? [], draft.source) : undefined;
  const targets = sending ? [] : castTargets(offers, draft);
  const choose = (target: CombatTarget) => {
    if (draft) apply(chooseCastTarget(offers, draft, target), draft);
  };

  let ui: CombatUi | null = null;
  // A spell sent with nothing to choose shows no draft: the turn action carries its wait.
  const unseen = sent !== null && sent.origin === null;
  const choosingMode = draft?.stage === "mode" && !sending;
  const shown = draft && offer && !unseen && !choosingMode;
  const cancelDraft = () => setDraft(null);
  const face = choosingMode && draft ? modalFace(draft.source) : null;
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
  if (draft && shown) {
    const marks = new Map<string, CardMark>([[draft.source, "gold"]]);
    const seatMarks = new Map<string, SeatMark>();
    for (const target of targets) {
      if (target.kind === "object") marks.set(target.handle, "teal");
      if (target.kind === "seat") seatMarks.set(target.seatId, "target");
    }
    if (draft.target?.kind === "object")
      marks.set(draft.target.handle, "ivory");
    if (draft.target?.kind === "seat")
      seatMarks.set(draft.target.seatId, "marked");
    const paying = draft.stage === "payment";
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
      onCard: (object) => {
        const target = targets.find(
          (entry) => entry.kind === "object" && entry.handle === object.handle,
        );
        if (!target) return false;
        choose(target);
        return true;
      },
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
        : { primary: cancel, secondary: null }),
      status: null,
    };
  }

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
          targets.flatMap((target) =>
            target.kind === "seat" ? [target.seatId] : [],
          ),
        ),
      // The targets are a function of these.
      [castOffers, offered, draft, sending],
    ),
    chooseSeat: (seatId) => choose({ kind: "seat", seatId }),
    pending:
      draft && shown ? { source: draft.source, target: draft.target } : null,
    prompt:
      !draft || !shown
        ? null
        : draft.stage === "payment"
          ? { kind: "payment", cost: offer.cost }
          : { kind: "target" },
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
            onChoose: (mode) =>
              apply(chooseCastMode(offers, draft, Number(mode)), draft),
            onCancel: cancelDraft,
            onPeek: () => setPeeking(true),
          }
        : null,
  };
}

/** The picker's faces in design pixels: as wide as approved, less on a small screen. */
function useFaceWidth(count: number, gap: number): number {
  const [width, setWidth] = useState(316);
  useLayoutEffect(() => {
    const measure = () => {
      const density =
        parseFloat(getComputedStyle(document.documentElement).fontSize) / 16;
      const across = window.innerWidth / density;
      const tall = window.innerHeight / density;
      // Room for the heading above and Cancel beneath, at a card's proportions.
      // A short screen draws both smaller, so it reserves less.
      const reserved = window.innerHeight < 480 ? 150 / density : 250;
      const byHeight = (tall - reserved) / 1.4;
      const byWidth = (across - 40 - gap * (count - 1)) / count;
      setWidth(Math.round(Math.max(120, Math.min(316, byWidth, byHeight))));
    };
    measure();
    window.addEventListener("resize", measure);
    return () => window.removeEventListener("resize", measure);
  }, [count, gap]);
  return width;
}

/**
 * Choosing a modal spell's one mode, as approved (MC-01, MC-02): the complete
 * original card once for each mode, with that mode's own paragraph outlined
 * as the only thing to press. Choosing goes straight on to its targets.
 * View Battlefield hides the choice without ending it; Cancel ends the cast.
 */
export function ModeChoice({
  choice,
}: {
  choice: ModeChoiceView | null;
}): ReactElement {
  const dialog = useRef<HTMLDialogElement>(null);
  const gap = 30;
  const faceWidth = useFaceWidth(choice?.modes.length ?? 2, gap);
  const open = choice !== null;
  useEffect(() => {
    const node = dialog.current;
    if (!node) return;
    if (open && !node.open) node.showModal();
    if (!open && node.open) node.close();
  }, [open]);
  return (
    <dialog
      ref={dialog}
      className={s.modeDialog}
      aria-label="Choose one"
      // Escape is not a way out of a cast: only Cancel is.
      onCancel={(event) => event.preventDefault()}
    >
      {choice && (
        <div className={s.modeScreen}>
          <div className={s.modeTop}>
            <button
              type="button"
              className={s.modePeek}
              onClick={choice.onPeek}
            >
              View Battlefield
            </button>
          </div>
          <h1 className={s.modeHeading}>Choose one</h1>
          <PrintedModePicker
            image={{
              src: choice.face.src,
              name: choice.face.name,
              width: choice.face.printed.width,
              height: choice.face.printed.height,
            }}
            modes={choice.modes}
            faceWidth={faceWidth}
            gap={gap}
            choice={{ kind: "one", onChoose: choice.onChoose }}
          />
          <div className={s.modeFooter}>
            <ActionButton onClick={choice.onCancel}>Cancel</ActionButton>
          </div>
        </div>
      )}
    </dialog>
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
 * What the draft waits for, over the table: Choose a target, or Pay with the
 * cost. It names the step only; the cards and portraits carry the choices.
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
          initial={reduced ? false : { opacity: 0, y: 6 }}
          animate={{ opacity: 1, y: 0 }}
          exit={{ opacity: 0 }}
          transition={{
            duration: reduced ? 0 : 0.22,
            ease: [0.22, 1, 0.36, 1],
          }}
        >
          {prompt.kind === "target" ? (
            <p role="status">Choose a target</p>
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
): { object: PlayerObject; target: CombatTarget | null } | null => {
  const object = pending ? objects.get(pending.source) : undefined;
  return object && pending ? { object, target: pending.target } : null;
};

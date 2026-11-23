import { useEffect, useMemo, useState, type ReactElement } from "react";
import { AnimatePresence, motion, useReducedMotion } from "motion/react";
import type { PlayerObject } from "@spellwake/application-contracts";
import { ManaCost } from "@spellwake/ui";
import { manaSymbolUrl } from "../../data/builder-catalogue";
import {
  beginCast,
  castTargets,
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
): Casting {
  const { castOffers, sendAction, commandPending, commandError } = source;
  const offers = useMemo(
    () => (free ? (castOffers ?? []) : []),
    [castOffers, free],
  );
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
  }, [offers, draft, sent]);

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
  const shown = draft && offer && !unseen;
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
      () =>
        draft || sent ? NONE : new Set(offers.map((entry) => entry.source)),
      [offers, draft, sent],
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
      // The targets are a function of these three.
      [offers, draft, sending],
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
  };
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

import {
  useEffect,
  useLayoutEffect,
  useRef,
  useState,
  type ReactElement,
  type RefObject,
} from "react";
import type { Rect } from "@spellwake/booster-pack";
import { motion, useReducedMotion } from "motion/react";
import { X } from "lucide-react";
import { ActionButton } from "@spellwake/ui";
import type { Opening } from "../../collection/pack-sources";
import { SPELLWAKE_CARD_BACK_URL } from "../../cosmetics/card-backs";
import { useCardBackUrl } from "../../cosmetics/use-card-back-url";
import { motionLanguage as m } from "../../lib/motion";
import { CardFace } from "../cards/CardFace";
import { OpeningStage } from "./OpeningStage";
import * as s from "./packs.css";

const STATUS: Record<
  "opening" | "unconfirmed" | "checking" | "loading",
  string
> = {
  opening: "Opening pack…",
  unconfirmed: "Still checking this opening…",
  checking: "Still checking this opening…",
  loading: "Loading your cards…",
};

/** Where an element sits in the viewport. */
function viewportRect(element: Element): Rect {
  const box = element.getBoundingClientRect();
  return { x: box.left, y: box.top, width: box.width, height: box.height };
}

/**
 * The opening stage, borderless over the whole window. While the opening is
 * unresolved it cannot be closed: X is disabled and Escape does nothing. Once
 * the cards are known, each is a button that turns it over, Reveal all turns
 * the rest, and Done or X closes. The cards are owned either way.
 *
 * Where motion is welcome the pack is staged in 3D: it lifts off the shelf,
 * waits while the opening resolves, tears open and deals the cards face down
 * onto their slots, where the page's own cards take over. A click or key
 * press lands them at once.
 */
export function PackOpening({
  opening,
  setName,
  setCode,
  canOpenAnother,
  onCheck,
  onClose,
  onOpenAnother,
  returnFocusRef,
}: {
  opening: Opening;
  /** The opened pack's set, which colours its wrapper. */
  setName: string | undefined;
  /** Its set code, which finds the set's symbol. */
  setCode: string | null;
  canOpenAnother: boolean;
  onCheck: () => void;
  onClose: () => void;
  onOpenAnother: () => void;
  returnFocusRef: RefObject<HTMLElement | null>;
}): ReactElement {
  const dialog = useRef<HTMLDialogElement>(null);
  const done = useRef<HTMLButtonElement>(null);
  const reduce = useReducedMotion() ?? false;
  const cardBack = useCardBackUrl();
  const [revealed, setRevealed] = useState<ReadonlySet<string>>(new Set());
  const [isRevealingAll, setIsRevealingAll] = useState(false);
  const cards = opening.cards;
  const isResolved = opening.stage === "opened";
  const isAllRevealed =
    !!cards && cards.every((card) => revealed.has(card.key));

  // Each opening in this stage is a round; Open another starts the next.
  const [round, setRound] = useState(0);
  const [isStaged, setIsStaged] = useState(!reduce);
  const [landedRound, setLandedRound] = useState(-1);
  const [slots, setSlots] = useState<readonly Rect[] | null>(null);
  const hadCards = useRef(false);
  const finishRef = useRef<(() => void) | null>(null);
  const packArea = useRef<HTMLDivElement>(null);
  const list = useRef<HTMLUListElement>(null);
  const wrapperSet = useRef(setName ?? opening.product);
  const wrapperCode = useRef(setCode);
  const [from] = useState(() => {
    const wrapper = returnFocusRef.current
      ?.closest("li")
      ?.querySelector("[data-pack-wrapper]");
    return wrapper ? viewportRect(wrapper) : undefined;
  });
  const staged = isStaged && !reduce;
  const isLanded = !staged || landedRound === round;
  useEffect(() => {
    if (cards) hadCards.current = true;
    else if (hadCards.current) {
      hadCards.current = false;
      setRound((current) => current + 1);
      setSlots(null);
      setIsStaged(!reduce);
    }
  }, [cards, reduce]);
  // The slots are laid out (unseen) before the deal, so it can aim at them.
  useLayoutEffect(() => {
    if (!staged || !cards || slots || !list.current) return;
    setSlots(
      [...list.current.querySelectorAll("[data-card-slot]")].map(viewportRect),
    );
  }, [staged, cards, slots]);
  const skip = (): void => {
    if (cards && !isLanded) finishRef.current?.();
  };

  // A new opening starts face down.
  useEffect(() => {
    setRevealed(new Set());
    setIsRevealingAll(false);
  }, [opening.packId, cards]);
  useEffect(() => {
    const element = dialog.current;
    if (element && !element.open) element.showModal();
    const returnTo = returnFocusRef.current;
    return () => {
      // The pack's own button, or the library heading when the last pack has gone.
      window.setTimeout(() => {
        const target =
          returnTo?.isConnected === true
            ? returnTo
            : document.querySelector<HTMLElement>("[data-packs-heading]");
        target?.focus();
      });
    };
  }, [returnFocusRef]);
  // Reveal all leaves focus on Done once the last card has turned.
  useEffect(() => {
    if (isRevealingAll && isAllRevealed) done.current?.focus();
  }, [isRevealingAll, isAllRevealed]);

  const close = (): void => {
    if (isResolved) onClose();
  };
  const revealAll = (): void => {
    if (!cards) return;
    setIsRevealingAll(true);
    setRevealed(new Set(cards.map((card) => card.key)));
  };

  return (
    <dialog
      ref={dialog}
      className={s.stage}
      aria-labelledby="pack-opening-title"
      aria-busy={!isResolved || !isLanded || undefined}
      onPointerDown={skip}
      onKeyDown={(event) => {
        if (!isLanded && (event.key === "Enter" || event.key === " ")) {
          event.preventDefault();
          skip();
        }
      }}
      onCancel={(event) => {
        // Escape closes only a resolved opening.
        event.preventDefault();
        close();
      }}
    >
      {/* The stage leaves in the same commit the page's cards appear. */}
      {staged && !isLanded && (
        <OpeningStage
          key={round}
          setName={wrapperSet.current}
          setCode={wrapperCode.current}
          product={opening.product}
          cardBackUrl={cardBack}
          from={round === 0 ? from : undefined}
          areaRef={packArea}
          slots={slots}
          finishRef={finishRef}
          onLanded={() => setLandedRound(round)}
          onUnavailable={() => setIsStaged(false)}
        />
      )}
      <div className={s.stageInner}>
        <div className={s.stageHead}>
          <h2 className={s.stageTitle} id="pack-opening-title">
            {opening.product}
          </h2>
          <button
            type="button"
            className={s.stageClose}
            aria-label="Close"
            disabled={!isResolved}
            onClick={close}
          >
            <X aria-hidden="true" />
          </button>
        </div>
        <div className={s.stageBody}>
          {staged && (opening.stage !== "opened" || !cards) && (
            <div className={s.packArea} ref={packArea} />
          )}
          {opening.stage !== "opened" || !cards ? (
            <div className={staged ? s.statusStaged : s.status} role="status">
              {!staged && <span className={s.spinner} aria-hidden="true" />}
              {STATUS[opening.stage === "opened" ? "loading" : opening.stage]}
              {(opening.stage === "unconfirmed" ||
                opening.stage === "checking") && (
                <ActionButton
                  treatment="compact-form"
                  emphasis="primary"
                  pending={opening.stage === "checking"}
                  onClick={onCheck}
                >
                  Check opening
                </ActionButton>
              )}
            </div>
          ) : (
            <>
              <ul
                ref={list}
                className={isLanded ? s.cards : s.cardsDealing}
                aria-label="Cards in this pack"
                inert={!isLanded}
              >
                {cards.map((card, index) => {
                  const isUp = revealed.has(card.key);
                  // Reveal all turns the cards in a quick sweep, in reading order.
                  const order = cards
                    .filter(
                      (other) => !revealed.has(other.key) || other === card,
                    )
                    .indexOf(card);
                  return (
                    <motion.li
                      key={card.key}
                      initial={
                        reduce || staged
                          ? false
                          : { opacity: 0, y: 40, scale: 0.94 }
                      }
                      animate={{ opacity: 1, y: 0, scale: 1 }}
                      transition={{
                        duration: m.duration.scene,
                        ease: m.ease.arrive,
                        delay: reduce ? 0 : index * m.stagger,
                      }}
                    >
                      <motion.button
                        type="button"
                        className={s.slot}
                        data-card-slot=""
                        aria-label={
                          isUp
                            ? `${card.name}, ${card.finish}`
                            : `Reveal card ${index + 1}`
                        }
                        aria-disabled={isUp || undefined}
                        onClick={() =>
                          !isUp &&
                          setRevealed((current) =>
                            new Set(current).add(card.key),
                          )
                        }
                        initial={false}
                        animate={{ rotateY: isUp ? 180 : 0 }}
                        whileHover={!isUp && !reduce ? { y: -6 } : {}}
                        transition={{
                          rotateY: {
                            duration: reduce ? 0 : m.duration.hero,
                            ease: m.ease.travel,
                            delay:
                              reduce || !isRevealingAll
                                ? 0
                                : Math.max(order, 0) * 0.05,
                          },
                          y: {
                            duration: m.duration.settle,
                            ease: m.ease.arrive,
                          },
                        }}
                      >
                        <span className={s.side}>
                          <img
                            className={s.backImage}
                            src={cardBack}
                            alt=""
                            onError={(event) => {
                              const image = event.currentTarget;
                              if (
                                image.getAttribute("src") !==
                                SPELLWAKE_CARD_BACK_URL
                              )
                                image.src = SPELLWAKE_CARD_BACK_URL;
                            }}
                          />
                        </span>
                        <span className={s.front}>
                          <CardFace
                            url={card.faceUrl}
                            name={card.name}
                            loading="eager"
                          />
                          {card.finish === "Foil" && (
                            <span className={s.foil} />
                          )}
                        </span>
                      </motion.button>
                    </motion.li>
                  );
                })}
              </ul>
              <div
                className={isLanded ? s.actions : s.actionsDealing}
                inert={!isLanded}
              >
                {!isAllRevealed ? (
                  <ActionButton treatment="compact-form" onClick={revealAll}>
                    Reveal all
                  </ActionButton>
                ) : (
                  <>
                    <ActionButton
                      ref={done}
                      treatment="compact-form"
                      onClick={close}
                    >
                      Done
                    </ActionButton>
                    {canOpenAnother && (
                      <ActionButton
                        treatment="compact-form"
                        emphasis="primary"
                        onClick={onOpenAnother}
                      >
                        Open another
                      </ActionButton>
                    )}
                  </>
                )}
              </div>
            </>
          )}
        </div>
      </div>
    </dialog>
  );
}

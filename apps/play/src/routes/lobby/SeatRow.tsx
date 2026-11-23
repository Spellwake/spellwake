import type { ReactElement } from "react";
import {
  Check,
  ChevronDown,
  Circle,
  Copy,
  Sparkle,
  UserRound,
} from "lucide-react";
import { SelectMenu } from "@spellwake/ui";
import { Portrait } from "../../account/Portrait";
import { useDeviceClass } from "../../lib/display-scale";
import type { AiDifficulty, LobbySeat } from "../../lobby/lobby-sources";
import * as s from "./lobby.css";

const DIFFICULTY: Record<AiDifficulty, string> = {
  normal: "Normal",
  advanced: "Advanced",
};

/**
 * One seat. Your own row chooses your deck; the host's rows set an empty or
 * AI seat's controller, an AI's difficulty and its deck. Everything else is
 * passive: another person's deck is private, and readiness is only shown.
 */
export function SeatRow({
  seat,
  isLocked,
  onChooseDeck,
  onChooseAiDeck,
  setController,
  setDifficulty,
}: {
  seat: LobbySeat;
  /** True during the countdown and start: nothing in a seat may change. */
  isLocked: boolean;
  onChooseDeck: () => void;
  onChooseAiDeck: (() => void) | null;
  setController: ((kind: "human" | "ai") => void) | null;
  setDifficulty: ((difficulty: AiDifficulty) => void) | null;
}): ReactElement {
  const isPhone = useDeviceClass() === "phone";
  const deckLabel =
    seat.deck === "random"
      ? "Random Deck"
      : seat.deck && typeof seat.deck === "object"
        ? `${seat.deck.name} · ${seat.deck.formatName}`
        : null;
  const controller = setController && (
    <span className={s.compact}>
      <SelectMenu
        label={`${seat.name}: controller`}
        value={seat.kind === "ai" ? "ai" : "human"}
        options={[
          { value: "human", label: "Human" },
          { value: "ai", label: "AI" },
        ]}
        onValueChange={(value) =>
          setController(value === "ai" ? "ai" : "human")
        }
        disabled={isLocked}
      />
    </span>
  );
  return (
    <li className={s.seat} data-seat={seat.kind}>
      <span className={s.pic}>
        {seat.kind === "ai" ? (
          <span className={s.aiMark} aria-hidden="true">
            <Sparkle fill="currentColor" strokeWidth={1} />
          </span>
        ) : seat.kind === "open" ? (
          <span className={s.openMark} aria-hidden="true">
            <UserRound strokeWidth={1.4} />
          </span>
        ) : (
          <Portrait portrait={seat.portrait} diameter={isPhone ? 56 : 76} />
        )}
      </span>
      <p className={s.name}>
        {seat.name}
        {seat.isHost && (
          <>
            {" · "}
            <span className={s.host}>Host</span>
          </>
        )}
      </p>
      <span className={s.divider} />
      <span className={s.controls}>
        {seat.kind === "you" ? (
          <button
            type="button"
            className={s.deckButton}
            disabled={isLocked}
            data-needs={seat.needsNewDeck}
            aria-label={
              deckLabel ? `Your deck: ${deckLabel}. Choose deck` : "Choose deck"
            }
            onClick={onChooseDeck}
          >
            <Copy className={s.icon} aria-hidden="true" />
            <span className={s.deckLabel}>{deckLabel ?? "Choose deck"}</span>
            <ChevronDown className={s.icon} aria-hidden="true" />
          </button>
        ) : seat.kind === "ai" ? (
          <>
            {controller ?? <span className={s.passive}>AI</span>}
            {setDifficulty && seat.difficulty ? (
              <span className={s.compact}>
                <SelectMenu
                  label="AI tier"
                  value={seat.difficulty}
                  options={[
                    { value: "normal", label: "Normal" },
                    { value: "advanced", label: "Advanced" },
                  ]}
                  onValueChange={(value) =>
                    setDifficulty(value === "advanced" ? "advanced" : "normal")
                  }
                  disabled={isLocked}
                />
              </span>
            ) : (
              seat.difficulty && (
                <span className={s.passive}>{DIFFICULTY[seat.difficulty]}</span>
              )
            )}
            {onChooseAiDeck ? (
              <button
                type="button"
                className={s.deckButton}
                disabled={isLocked}
                data-needs={seat.needsNewDeck}
                aria-label={`AI deck: ${deckLabel ?? "none"}. Choose deck`}
                onClick={onChooseAiDeck}
              >
                <span className={s.deckLabel}>
                  {deckLabel ?? "Choose deck"}
                </span>
                <ChevronDown className={s.icon} aria-hidden="true" />
              </button>
            ) : (
              <span className={s.passive}>
                {deckLabel ?? "Choosing a deck"}
              </span>
            )}
          </>
        ) : seat.kind === "open" ? (
          <>
            {controller ?? <span className={s.passive}>Human</span>}
            <span className={s.waiting}>Waiting for player</span>
          </>
        ) : (
          <>
            {controller ?? <span className={s.passive}>Human</span>}
            {seat.needsNewDeck ? (
              <span className={s.warning}>Choosing a new deck</span>
            ) : (
              seat.deck !== "private" && (
                <span className={s.passive}>
                  {seat.deck ? "Deck selected" : "No deck yet"}
                </span>
              )
            )}
          </>
        )}
        {seat.kind === "you" && seat.needsNewDeck && (
          <span className={s.warning}>Choose a deck for this format</span>
        )}
      </span>
      {seat.kind !== "open" && (
        <span className={s.readiness} data-ready={seat.isReady}>
          {seat.isReady ? (
            <Check className={s.readyIcon} aria-hidden="true" />
          ) : (
            <Circle className={s.readyIcon} aria-hidden="true" />
          )}
          {seat.isReady ? "Ready" : "Not ready"}
        </span>
      )}
    </li>
  );
}

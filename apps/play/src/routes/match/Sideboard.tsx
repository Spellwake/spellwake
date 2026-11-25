import { useState, type ReactElement } from "react";
import { ArrowLeft, ArrowRight } from "lucide-react";
import { ActionButton, Dialog } from "@spellwake/ui";
import type { Sideboard } from "../../match/match-sources";
import {
  changed,
  counts,
  moveCard,
  type SideboardCard,
} from "../../match/sideboard";
import * as s from "./sideboard.css";

function Column({
  title,
  count,
  cards,
  side,
  onMove,
}: {
  title: string;
  count: number;
  cards: readonly SideboardCard[];
  side: "main" | "side";
  onMove: (key: string) => void;
}): ReactElement {
  const shown = cards.filter((card) => card[side] > 0);
  const to = side === "main" ? "the sideboard" : "the main deck";
  return (
    <section className={s.column} aria-label={title}>
      <h3 className={s.columnTitle}>
        {title} <span className={s.count}>{count}</span>
      </h3>
      {shown.length === 0 ? (
        <p className={s.empty}>No cards</p>
      ) : (
        <ul className={s.list}>
          {shown.map((card) => (
            <li key={card.key} className={s.row}>
              <span className={s.quantity}>{card[side]}</span>
              <span className={s.name}>{card.name}</span>
              <button
                type="button"
                className={s.move}
                data-side={side}
                aria-label={`Move one ${card.name} to ${to}`}
                onClick={() => onMove(card.key)}
              >
                {side === "main" ? (
                  <ArrowRight aria-hidden="true" />
                ) : (
                  <ArrowLeft aria-hidden="true" />
                )}
              </button>
            </li>
          ))}
        </ul>
      )}
    </section>
  );
}

/**
 * Sideboarding between games. A card moves one copy at a time between the
 * main deck and the sideboard; nothing is added or taken out, and the main
 * deck keeps at least as many cards as it began the match with.
 */
export function SideboardDialog({
  sideboard,
  onClose,
}: {
  sideboard: Sideboard;
  onClose: () => void;
}): ReactElement {
  const [cards, setCards] = useState<readonly SideboardCard[] | null>(null);
  const start = sideboard.cards;
  const shown = cards ?? start;
  const total = shown ? counts(shown) : null;
  const short = total !== null && total.main < sideboard.minimumMain;
  const move = (key: string, to: "main" | "side") =>
    setCards((current) => moveCard(current ?? start ?? [], key, to));
  return (
    <Dialog
      open
      size="wide"
      title="Sideboard"
      {...(total
        ? { subtitle: `Main deck ${total.main} · Sideboard ${total.side}` }
        : {})}
      busy={sideboard.pending}
      onClose={onClose}
      actions={
        <>
          {start && shown && changed(start, shown) && (
            <ActionButton
              disabled={sideboard.pending}
              onClick={() => setCards(null)}
            >
              Reset
            </ActionButton>
          )}
          <ActionButton
            emphasis="primary"
            pending={sideboard.pending}
            disabled={!shown || short}
            onClick={() => shown && sideboard.use(shown)}
          >
            Use this deck
          </ActionButton>
        </>
      }
    >
      {shown ? (
        <div className={s.body}>
          <div className={s.columns}>
            <Column
              title="Main deck"
              count={total?.main ?? 0}
              cards={shown}
              side="main"
              onMove={(key) => move(key, "side")}
            />
            <Column
              title="Sideboard"
              count={total?.side ?? 0}
              cards={shown}
              side="side"
              onMove={(key) => move(key, "main")}
            />
          </div>
          {short && (
            <p className={s.note} role="status">
              Keep at least {sideboard.minimumMain} cards in your main deck.
            </p>
          )}
          {sideboard.failure && (
            <p className={s.note} role="alert">
              {sideboard.failure}
            </p>
          )}
        </div>
      ) : (
        <p className={s.empty} role="status">
          Reading your deck…
        </p>
      )}
    </Dialog>
  );
}

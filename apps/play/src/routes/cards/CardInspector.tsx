import { useEffect, useState, type ReactElement } from "react";
import { Check, RefreshCw } from "lucide-react";
import { ActionButton } from "@spellwake/ui";
import {
  copiesLabel,
  printingLabel,
  type OwnedCard,
  type OwnedPrinting,
} from "../../collection/card-sources";
import { CardFace, type ArtState } from "./CardFace";
import * as s from "./cards.css";

/**
 * The chosen card, read-only: its face large, its name and total copies, and
 * every owned printing with its own count. Choosing a printing changes the
 * face, never a count. A two-faced printing can be flipped.
 */
export function CardInspector({
  card,
  printing,
  onChoosePrinting,
  inDialog = false,
  onArtState,
}: {
  card: OwnedCard;
  printing: OwnedPrinting;
  onChoosePrinting: (printing: OwnedPrinting) => void;
  /** In a dialog the name and count are its title, so they are not repeated. */
  inDialog?: boolean;
  onArtState?: (state: ArtState) => void;
}): ReactElement {
  const [isBack, setIsBack] = useState(false);
  useEffect(() => setIsBack(false), [printing.id]);
  const shown =
    isBack && printing.backUrl ? printing.backUrl : printing.faceUrl;
  return (
    <div className={s.inspectorBody} data-in-dialog={inDialog || undefined}>
      <div className={s.inspectorFace} data-card-landing="">
        <CardFace
          url={shown}
          name={card.name}
          reportStatus
          loading="eager"
          {...(onArtState ? { onStateChange: onArtState } : {})}
        />
      </div>
      {printing.backUrl && (
        <span className={s.flip}>
          <ActionButton
            treatment="compact-form"
            icon={<RefreshCw />}
            onClick={() => setIsBack((back) => !back)}
          >
            Flip card
          </ActionButton>
        </span>
      )}
      <h2 className={s.inspectorName}>{card.name}</h2>
      <p className={s.copies}>{copiesLabel(card.copies)}</p>
      <h3 className={s.printingsHeading} id={`printings-${card.id}`}>
        Owned printings
      </h3>
      <div
        className={s.rows}
        role="radiogroup"
        aria-labelledby={`printings-${card.id}`}
        onKeyDown={(event) => {
          // Arrow keys move between printings, as in any radio group.
          const step =
            event.key === "ArrowDown" || event.key === "ArrowRight"
              ? 1
              : event.key === "ArrowUp" || event.key === "ArrowLeft"
                ? -1
                : 0;
          if (!step) return;
          event.preventDefault();
          const index = card.printings.findIndex((p) => p.id === printing.id);
          const next =
            card.printings[
              (index + step + card.printings.length) % card.printings.length
            ];
          if (!next) return;
          onChoosePrinting(next);
          const target = event.currentTarget.querySelector<HTMLElement>(
            `[data-printing="${CSS.escape(next.id)}"]`,
          );
          target?.focus();
        }}
      >
        {card.printings.map((owned) => {
          const isChosen = owned.id === printing.id;
          return (
            <button
              key={owned.id}
              type="button"
              role="radio"
              aria-checked={isChosen}
              tabIndex={isChosen ? 0 : -1}
              data-printing={owned.id}
              className={s.row}
              onClick={() => onChoosePrinting(owned)}
            >
              {isChosen ? (
                <Check className={s.tick} aria-hidden="true" />
              ) : (
                <span />
              )}
              {owned.faceUrl ? (
                <img className={s.thumb} src={owned.faceUrl} alt="" />
              ) : (
                <span className={s.thumbBlank} />
              )}
              <span className={s.rowText}>
                <span>{printingLabel(owned)}</span>
                <span>{owned.finish}</span>
              </span>
              <span className={s.rowCopies}>{copiesLabel(owned.copies)}</span>
            </button>
          );
        })}
      </div>
    </div>
  );
}

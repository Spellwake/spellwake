import type { ReactElement } from "react";
import { Check, RefreshCw } from "lucide-react";
import { CoinAmount } from "@spellwake/ui";
import type { DailyQuest, QuestsSource } from "../../play/quest-sources";
import * as s from "./quest-strip.css";

const COIN = `${import.meta.env.BASE_URL}coin.svg`;

function Quest({
  quest,
  onReroll,
  rerolling,
}: {
  quest: DailyQuest;
  /** Absent once today's swap is used, or for a finished quest. */
  onReroll?: (() => void) | undefined;
  rerolling: boolean;
}): ReactElement {
  const share = Math.min(1, quest.progress / quest.target);
  return (
    <li className={s.quest} data-done={quest.done} aria-busy={rerolling}>
      <div className={s.top}>
        <span className={s.text}>{quest.text}</span>
        {onReroll && (
          <button
            type="button"
            className={s.swap}
            aria-label={`Swap “${quest.text}” for another quest`}
            disabled={rerolling}
            onClick={onReroll}
          >
            <RefreshCw aria-hidden="true" />
          </button>
        )}
      </div>
      <div
        className={s.track}
        role="progressbar"
        aria-label={quest.text}
        aria-valuemin={0}
        aria-valuemax={quest.target}
        aria-valuenow={quest.progress}
      >
        <span className={s.fill} style={{ inlineSize: `${share * 100}%` }} />
      </div>
      <div className={s.bottom}>
        <span className={s.count}>
          {quest.done ? (
            <>
              <Check aria-hidden="true" className={s.check} /> Done
            </>
          ) : (
            `${quest.progress} / ${quest.target}`
          )}
        </span>
        <span className={s.reward}>
          <CoinAmount amount={quest.rewardCoins} iconSrc={COIN} />
        </span>
      </div>
    </li>
  );
}

/**
 * Today's quests above the decks: what each asks, how far along it is and
 * what it pays, with one swap a day. It shows nothing until there is
 * something to show.
 */
export function QuestStrip({
  source,
}: {
  source: QuestsSource;
}): ReactElement | null {
  if (source.status !== "ready" || source.quests.length === 0) return null;
  return (
    <section className={s.strip} aria-label="Daily quests">
      <h2 className={s.heading}>Daily quests</h2>
      <ul className={s.list}>
        {source.quests.map((quest) => (
          <Quest
            key={quest.id}
            quest={quest}
            rerolling={source.rerolling === quest.id}
            onReroll={
              source.canReroll && !quest.done
                ? () => source.reroll(quest.id)
                : undefined
            }
          />
        ))}
      </ul>
      {source.wins && (
        <p className={s.wins}>
          <span className={s.winsCount}>{source.wins.count}</span>
          <span className={s.winsLabel}>
            {source.wins.count === 1 ? "win today" : "wins today"}
          </span>
          <span className={s.reward}>
            <CoinAmount amount={source.wins.coins} iconSrc={COIN} />
          </span>
        </p>
      )}
    </section>
  );
}

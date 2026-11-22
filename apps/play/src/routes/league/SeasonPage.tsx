import { useEffect, useState, type ReactElement } from "react";
import { RefreshCw, Swords } from "lucide-react";
import {
  ActionButton,
  CoinAmount,
  EmptyState,
  FeedbackMessages,
  LoadingShape,
  LoadingState,
  RankIdentity,
  RankProgress,
  SelectMenu,
} from "@spellwake/ui";
import type { SeasonSource } from "../../league/league-sources";
import { COIN } from "../market/ListingDialogs";
import * as s from "./league.css";

export function SeasonPage({ source }: { source: SeasonSource }): ReactElement {
  const ready = source.status === "ready" ? source : null;
  const [formatId, setFormatId] = useState("");
  const [deckId, setDeckId] = useState("");

  useEffect(() => {
    if (!ready) return;
    if (!formatId && ready.formats[0]) setFormatId(ready.formats[0].id);
  }, [ready, formatId]);

  const matchingDecks = ready
    ? ready.decks.filter((d) => !formatId || d.formatId === formatId)
    : [];

  useEffect(() => {
    if (
      matchingDecks.length > 0 &&
      !matchingDecks.some((d) => d.id === deckId)
    ) {
      setDeckId(matchingDecks[0]!.id);
    }
  }, [matchingDecks, deckId]);

  if (source.status === "loading") {
    return (
      <section className={s.center} aria-label="This season">
        <LoadingState label="Loading season…">
          <LoadingShape width={260} height={36} />
        </LoadingState>
      </section>
    );
  }
  if (source.status !== "ready") {
    return (
      <section className={s.center} aria-label="This season">
        <EmptyState
          title="Season unavailable"
          description="Sign in to view your seasonal ranks and quests."
        />
      </section>
    );
  }

  return (
    <section className={s.page} aria-label="This season">
      <div className={s.header}>
        <h1 className={s.title}>{source.seasonName ?? "This season"}</h1>
        {source.seasonDates && (
          <p className={s.subtitle}>{source.seasonDates}</p>
        )}
      </div>

      <div className={s.grid}>
        {/* Ranked Queue */}
        <div className={s.panel}>
          <h2 className={s.panelTitle}>Play ranked</h2>
          {source.activeQueue ? (
            <div className={s.controlsRow}>
              <span className={s.recordText}>
                Searching {source.activeQueue.formatName}…
              </span>
              <ActionButton
                disabled={source.queueBusy}
                pending={source.queueBusy}
                onClick={() => void source.cancelQueue()}
              >
                Cancel queue
              </ActionButton>
            </div>
          ) : (
            <div className={s.controlsRow}>
              {source.formats.length > 0 && (
                <SelectMenu
                  label="Ranked format"
                  value={formatId}
                  onValueChange={setFormatId}
                  options={source.formats.map((f) => ({
                    value: f.id,
                    label: f.name,
                  }))}
                />
              )}
              {matchingDecks.length > 0 ? (
                <>
                  <SelectMenu
                    label="Ranked deck"
                    value={deckId}
                    onValueChange={setDeckId}
                    options={matchingDecks.map((d) => ({
                      value: d.id,
                      label: d.name,
                    }))}
                  />
                  <ActionButton
                    disabled={!formatId || !deckId || source.queueBusy}
                    pending={source.queueBusy}
                    onClick={() => void source.joinQueue(formatId, deckId)}
                  >
                    <Swords size={18} aria-hidden="true" />
                    Find match
                  </ActionButton>
                </>
              ) : (
                <p className={s.subtitle}>
                  Build a deck in this format to enter ranked play.
                </p>
              )}
            </div>
          )}
        </div>

        {/* Daily Quests */}
        <div className={s.panel}>
          <h2 className={s.panelTitle}>Daily quests</h2>
          {source.quests.length === 0 ? (
            <p className={s.subtitle}>No active quests right now.</p>
          ) : (
            source.quests.map((quest) => (
              <div key={quest.id} className={s.questRow}>
                <div className={s.questMeta}>
                  <span className={s.formatName}>{quest.title}</span>
                  <span className={s.recordText}>
                    {quest.progress} / {quest.target} ·{" "}
                    <CoinAmount amount={quest.rewardCoins} iconSrc={COIN} />
                  </span>
                </div>
                {quest.canReroll && (
                  <ActionButton
                    disabled={source.rerollingQuestId === quest.id}
                    pending={source.rerollingQuestId === quest.id}
                    onClick={() => void source.rerollQuest(quest.id)}
                  >
                    <RefreshCw size={16} aria-hidden="true" />
                    Reroll
                  </ActionButton>
                )}
              </div>
            ))
          )}
        </div>
      </div>

      {/* Format Ranks */}
      <div className={s.panel}>
        <h2 className={s.panelTitle}>Your ranks</h2>
        {source.ranks.length === 0 ? (
          <EmptyState
            title="No ranked matches yet"
            description="Complete a ranked match to place on the ladder."
          />
        ) : (
          <div className={s.grid}>
            {source.ranks.map((rank) => (
              <div key={rank.formatId} className={s.rankCard}>
                <div className={s.rankHead}>
                  <span className={s.formatName}>{rank.formatName}</span>
                  <span className={s.recordText}>
                    {rank.wins}W–{rank.losses}L
                    {rank.draws > 0 ? `–${rank.draws}D` : ""}
                  </span>
                </div>
                <RankIdentity
                  tier={rank.tier}
                  emblemSrc={rank.emblemUrl}
                  presentation="season"
                  ratingLabel={`${rank.rating} rating`}
                />
                {rank.isHighest ? (
                  <RankProgress highest label={rank.progressLabel} />
                ) : (
                  <RankProgress
                    rating={rank.rating}
                    lowerBound={rank.lowerBound}
                    nextBound={rank.nextBound}
                    label={rank.progressLabel}
                  />
                )}
              </div>
            ))}
          </div>
        )}
      </div>

      <FeedbackMessages
        label="Season notices"
        presentation="toast"
        items={
          source.notice
            ? [
                {
                  id: "league-notice",
                  text: source.notice,
                  onDismiss: source.dismissNotice,
                },
              ]
            : []
        }
      />
    </section>
  );
}

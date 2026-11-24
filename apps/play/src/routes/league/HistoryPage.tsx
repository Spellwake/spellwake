import { useEffect, useRef, type ReactElement } from "react";
import { FeedbackMessages } from "@spellwake/ui";
import {
  OUTCOME_LABEL,
  opponentsLine,
  playedLine,
  type HistoryMatch,
  type HistorySource,
} from "../../league/history-sources";
import { useArrival } from "../../lib/arrival";
import { TitleTrail } from "../market/PredictionParts";
import { GAP_COPY } from "./LeagueParts";
import * as h from "./history.css";
import * as s from "./league.css";

function Row({ match, now }: { match: HistoryMatch; now: Date }): ReactElement {
  return (
    <div className={h.row} role="row" data-outcome={match.outcome}>
      <span className={h.outcome} role="cell">
        {OUTCOME_LABEL[match.outcome]}
        {match.games && (
          <span className={h.outcomeGames} aria-hidden="true">
            {" "}
            {match.games.you}–{match.games.them}
          </span>
        )}
      </span>
      <span className={h.opponent} role="cell">
        <span className={h.versus} aria-hidden="true">
          vs{" "}
        </span>
        {opponentsLine(match.opponents)}
      </span>
      <span className={h.format} role="cell">
        {match.formatName}
        {match.ranked && <span className={h.ranked}> · Ranked</span>}
      </span>
      <span
        className={h.games}
        role="cell"
        {...(match.games
          ? {
              "aria-label": `${match.games.you} games to ${match.games.them}`,
            }
          : {})}
      >
        {match.games ? `${match.games.you}–${match.games.them}` : "—"}
      </span>
      <span className={h.played} role="cell">
        {playedLine(match.startedAt, now)}
      </span>
    </div>
  );
}

/** A match whose facts are still being read keeps its place in the list. */
function PendingRow(): ReactElement {
  return (
    <div className={`${h.row} ${s.skeletonRow}`} aria-hidden="true">
      <span className={s.skeleton} style={{ width: "60%" }} />
      <span className={s.skeleton} style={{ width: "45%" }} />
      <span className={`${s.skeleton} ${h.pendingFact}`} />
      <span className={`${s.skeleton} ${h.pendingFact}`} />
      <span className={`${s.skeleton} ${h.pendingFact}`} />
    </div>
  );
}

const NOTICE_MS = 5000;

/**
 * League · History: every match the player has finished, newest first, with
 * how it came out, who it was against, the format and when.
 */
export function HistoryPage({
  source,
}: {
  source: HistorySource;
}): ReactElement {
  const ready = source.status === "ready" ? source : null;
  const entries = ready?.entries ?? [];
  const arrival = useArrival(entries.length > 0);
  const now = new Date();
  const page = useRef<HTMLElement>(null);
  const body = useRef<HTMLDivElement>(null);
  const end = useRef<HTMLDivElement>(null);
  // Older matches are shown as the end of the list comes near.
  const loadMore = ready?.loadMore;
  const canLoad = !!ready?.hasMore;
  useEffect(() => {
    const node = end.current;
    if (!node || !canLoad || !loadMore) return;
    // The list scrolls itself, except on a sideways phone, where the page does.
    const root =
      body.current && getComputedStyle(body.current).overflowY !== "visible"
        ? body.current
        : page.current;
    const observer = new IntersectionObserver(
      (seen) => {
        if (seen.some((entry) => entry.isIntersecting)) loadMore();
      },
      { root, rootMargin: "0px 0px 400px 0px" },
    );
    observer.observe(node);
    return () => observer.disconnect();
  }, [canLoad, loadMore, entries.length]);
  // A read's passive notice leaves by itself.
  const notice = ready?.notice ?? null;
  const dismissNotice = ready?.dismissNotice;
  useEffect(() => {
    if (!notice || !dismissNotice) return;
    const timer = window.setTimeout(dismissNotice, NOTICE_MS);
    return () => window.clearTimeout(timer);
  }, [notice, dismissNotice]);

  let content: ReactElement;
  if (source.status === "loading")
    content = (
      <div aria-hidden="true">
        <PendingRow />
        <PendingRow />
        <PendingRow />
      </div>
    );
  else if (source.status === "signed-out" || source.status === "unregistered")
    content = <p className={s.quiet}>{GAP_COPY[source.status]}</p>;
  else if (source.status !== "ready")
    content = <p className={s.quiet}>History is unavailable.</p>;
  else if (entries.length === 0)
    // A list that could not be read says so in its notice, not as "none".
    content = notice ? (
      <div className={s.neutral} />
    ) : (
      <p className={s.quiet}>No matches yet</p>
    );
  else
    content = (
      <div role="rowgroup" {...arrival.group}>
        {entries.map((entry) =>
          entry.match ? (
            <Row key={entry.matchId} match={entry.match} now={now} />
          ) : (
            <PendingRow key={entry.matchId} />
          ),
        )}
        <div ref={end} />
      </div>
    );
  return (
    <section ref={page} className={s.root} aria-label="History" role="region">
      <header>
        <h1 className={s.title}>
          History
          <span className={s.trailSpace}>
            <TitleTrail />
          </span>
        </h1>
      </header>
      <div className={s.panel} role="table" aria-label="Match history">
        <div className={h.head} role="row">
          <span role="columnheader">Result</span>
          <span role="columnheader">Opponent</span>
          <span role="columnheader">Format</span>
          <span role="columnheader" className={s.center}>
            Games
          </span>
          <span role="columnheader" className={h.played}>
            Played
          </span>
        </div>
        <div className={s.body} ref={body} tabIndex={0}>
          {content}
        </div>
      </div>
      <FeedbackMessages
        label="History messages"
        presentation="toast"
        items={notice ? [{ id: "read", text: notice }] : []}
      />
    </section>
  );
}

import {
  useEffect,
  useRef,
  useState,
  type ReactElement,
  type RefObject,
} from "react";
import { parseAsString, useQueryState } from "nuqs";
import { RankIdentity } from "@spellwake/ui";
import {
  emblemUrl,
  ratingText,
  recordLong,
  recordShort,
  tierOf,
  type LeagueSource,
  type Standings,
  type StandingRow,
} from "../../league/league-sources";
import { Portrait } from "../../account/Portrait";
import { GAP_COPY, LeagueHead, LeagueNotice } from "./LeagueParts";
import * as s from "./league.css";

type Own = NonNullable<Standings["own"]>;

/** One player's row. Their name and picture open their public profile. */
function Row({
  row,
  onProfile,
  rowRef,
  pinned = false,
  hidden = false,
}: {
  row: StandingRow | Own;
  onProfile: (playerId: string) => void;
  rowRef?: RefObject<HTMLDivElement | null>;
  pinned?: boolean;
  hidden?: boolean;
}): ReactElement {
  const tier = tierOf(row.rating);
  return (
    <div
      ref={rowRef}
      className={pinned ? s.pinned : s.standingRow}
      data-you={row.isYou}
      data-hidden={hidden}
      aria-hidden={hidden || undefined}
      role="row"
    >
      <span className={s.place} role="cell">
        {row.place ?? "—"}
      </span>
      <span role="cell" style={{ minWidth: 0 }}>
        <button
          type="button"
          className={s.player}
          tabIndex={hidden ? -1 : undefined}
          aria-label={`${row.isYou ? "Your" : `${row.name}’s`} profile`}
          aria-haspopup="dialog"
          onClick={() => onProfile(row.playerId)}
        >
          <Portrait portrait={row.portrait} diameter={64} />
          <span className={s.playerName}>{row.name}</span>
        </button>
      </span>
      <span className={s.rankStanding} role="cell">
        <RankIdentity
          presentation="standings"
          tier={tier.name}
          emblemSrc={emblemUrl(tier.name)}
        />
      </span>
      <span className={s.center} role="cell">
        {ratingText(row.rating)}
      </span>
      <span
        className={s.center}
        role="cell"
        aria-label={recordLong(row.record)}
      >
        <span aria-hidden="true">{recordShort(row.record)}</span>
      </span>
    </div>
  );
}

function SkeletonRows({ count }: { count: number }): ReactElement {
  return (
    <div aria-hidden="true">
      {Array.from({ length: count }, (_, index) => (
        <div key={index} className={`${s.standingRow} ${s.skeletonRow}`}>
          <span
            className={s.skeleton}
            style={{ width: 28, justifySelf: "center" }}
          />
          <span style={{ display: "flex", gap: 18, alignItems: "center" }}>
            <span className={s.skeletonDot} />
            <span className={s.skeleton} style={{ width: "55%" }} />
          </span>
          <span
            className={s.skeleton}
            style={{ width: "60%", justifySelf: "center" }}
          />
          <span
            className={s.skeleton}
            style={{ width: "60%", justifySelf: "center" }}
          />
          <span
            className={s.skeleton}
            style={{ width: "60%", justifySelf: "center" }}
          />
        </div>
      ))}
    </div>
  );
}

/**
 * League · Standings, as approved (ST-01 to ST-05): a tab per ranked format,
 * then that format's full table for the current season, best first, with
 * shared places for equal ratings. The list reads on as its end comes into
 * view. Your row stays pinned below the list until it scrolls into view.
 */
export function StandingsPage({
  source,
  format,
  onFormat,
}: {
  source: LeagueSource;
  format: string | null;
  onFormat: (formatId: string) => void;
}): ReactElement {
  const [, setPlayer] = useQueryState("player", parseAsString);
  const body = useRef<HTMLDivElement>(null);
  const page = useRef<HTMLElement>(null);
  // The list scrolls itself, except on a sideways phone, where the page scrolls.
  const scroller = () =>
    body.current && getComputedStyle(body.current).overflowY !== "visible"
      ? body.current
      : page.current;
  const ownRow = useRef<HTMLDivElement>(null);
  const end = useRef<HTMLDivElement>(null);
  const [isOwnInView, setIsOwnInView] = useState(false);
  const ready = source.status === "ready" ? source : null;
  const standings = ready?.standings ?? null;
  const formats = ready?.formats ?? [];
  const ownLoaded = standings?.rows.some((row) => row.isYou) ?? false;

  // A new format starts at the top of its list.
  useEffect(() => {
    scroller()?.scrollTo({ top: 0 });
    setIsOwnInView(false);
  }, [format]);
  // Your pinned row steps aside while your own row is in view.
  useEffect(() => {
    const node = ownRow.current;
    if (!node || !body.current) {
      setIsOwnInView(false);
      return;
    }
    const observer = new IntersectionObserver(
      (entries) =>
        setIsOwnInView(entries.some((entry) => entry.isIntersecting)),
      { root: scroller(), threshold: 0.6 },
    );
    observer.observe(node);
    return () => observer.disconnect();
  }, [ownLoaded, format, standings?.rows.length]);
  // The next page is read as the end of the list comes near.
  const loadMore = standings?.loadMore;
  const canLoad = !!standings?.hasMore && !standings.isAppending;
  useEffect(() => {
    const node = end.current;
    if (!node || !canLoad || !loadMore) return;
    const observer = new IntersectionObserver(
      (entries) => {
        if (entries.some((entry) => entry.isIntersecting)) loadMore();
      },
      { root: scroller(), rootMargin: "0px 0px 400px 0px" },
    );
    observer.observe(node);
    return () => observer.disconnect();
  }, [canLoad, loadMore]);

  const openProfile = (playerId: string) => void setPlayer(playerId);
  let content: ReactElement;
  if (source.status === "loading" || standings?.state === "loading")
    content = <SkeletonRows count={3} />;
  else if (source.status === "signed-out" || source.status === "unregistered")
    content = <p className={s.quiet}>{GAP_COPY[source.status]}</p>;
  else if (source.status === "unavailable")
    content = <p className={s.quiet}>Standings are unavailable.</p>;
  else if (
    !standings ||
    (standings.state === "failed" && standings.rows.length === 0)
  )
    content = <div className={s.neutral} />;
  else if (standings.rows.length === 0)
    content = <p className={s.quiet}>No ranked matches yet</p>;
  else
    content = (
      <div role="rowgroup">
        {standings.rows.map((row) => (
          <Row
            key={row.playerId}
            row={row}
            onProfile={openProfile}
            {...(row.isYou ? { rowRef: ownRow } : {})}
          />
        ))}
        <div ref={end} />
        {standings.isAppending && <SkeletonRows count={2} />}
      </div>
    );
  const own = standings?.own ?? null;
  const showPinned =
    !!standings && standings.state !== "loading" && own !== null;

  return (
    <section ref={page} className={s.root} aria-label="Standings" role="region">
      <LeagueHead title="Standings" source={source} />
      <div className={s.tabs} role="tablist" aria-label="Formats">
        {formats.map((entry) => (
          <button
            key={entry.id}
            type="button"
            role="tab"
            id={`league-tab-${entry.id}`}
            aria-selected={entry.id === format}
            aria-controls="league-standings"
            tabIndex={entry.id === format ? 0 : -1}
            className={s.tab}
            onClick={() => onFormat(entry.id)}
            onKeyDown={(event) => {
              if (event.key !== "ArrowRight" && event.key !== "ArrowLeft")
                return;
              event.preventDefault();
              const at = formats.findIndex((item) => item.id === entry.id);
              const next =
                formats[
                  (at +
                    (event.key === "ArrowRight" ? 1 : -1) +
                    formats.length) %
                    formats.length
                ]!;
              onFormat(next.id);
              const tab = document.getElementById(`league-tab-${next.id}`);
              tab?.focus();
              tab?.scrollIntoView({ block: "nearest", inline: "nearest" });
            }}
          >
            {entry.name}
          </button>
        ))}
      </div>
      <div
        className={formats.length > 0 ? s.tabbedPanel : s.panel}
        id="league-standings"
        role="table"
        aria-label={
          standings ? `${standings.format.name} standings` : "Standings"
        }
      >
        <div className={s.headRow} data-table="standings" role="row">
          <span role="columnheader" className={s.place}>
            Place
          </span>
          <span role="columnheader">Player</span>
          <span role="columnheader" className={s.center}>
            Rank
          </span>
          <span role="columnheader" className={s.center}>
            Rating
          </span>
          <span role="columnheader" className={s.center}>
            Record
          </span>
        </div>
        <div className={s.body} ref={body} tabIndex={0}>
          {content}
        </div>
        {showPinned && (
          <Row
            row={own}
            onProfile={openProfile}
            pinned
            hidden={ownLoaded && isOwnInView}
          />
        )}
      </div>
      <LeagueNotice source={source} />
    </section>
  );
}

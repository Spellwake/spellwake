import type { ReactElement } from "react";
import { CoinAmount, RankIdentity, RankProgress } from "@spellwake/ui";
import {
  emblemUrl,
  progressOf,
  ratingText,
  recordLong,
  rewardOf,
  tierOf,
  type FormatRank,
  type LeagueSource,
} from "../../league/league-sources";
import { COIN } from "../market/ListingDialogs";
import { GAP_COPY, LeagueHead, LeagueNotice } from "./LeagueParts";
import * as s from "./league.css";

function SeasonRow({ row }: { row: FormatRank }): ReactElement {
  const tier = tierOf(row.rating);
  const progress = progressOf(row.rating);
  const reward = rewardOf(row.rating, row.record);
  return (
    <div className={s.seasonRow} role="row">
      <span className={s.format} role="rowheader">
        {row.format.name}
      </span>
      <span className={s.rankCell} role="cell">
        <RankIdentity
          presentation="season"
          tier={tier.name}
          emblemSrc={emblemUrl(tier.name)}
          ratingLabel={`Rating ${ratingText(row.rating)}`}
        />
      </span>
      <span className={s.progressCell} role="cell">
        {progress.highest ? (
          <RankProgress highest label={progress.label} />
        ) : (
          <RankProgress
            rating={row.rating}
            lowerBound={progress.lowerBound}
            nextBound={progress.nextBound}
            label={progress.label}
          />
        )}
      </span>
      <span className={s.recordCell} role="cell">
        {recordLong(row.record)}
      </span>
      <span className={s.rewardCell} role="cell">
        {reward === null ? (
          <span className={s.none} aria-label="No season reward yet">
            —
          </span>
        ) : (
          <CoinAmount amount={reward} iconSrc={COIN} />
        )}
      </span>
    </div>
  );
}

function SkeletonRows(): ReactElement {
  return (
    <div aria-hidden="true">
      {[0, 1, 2, 3].map((index) => (
        <div key={index} className={`${s.seasonRow} ${s.skeletonRow}`}>
          <span className={s.skeleton} style={{ width: "70%" }} />
          <span style={{ display: "flex", gap: 18, alignItems: "center" }}>
            <span className={s.skeletonGem} />
            <span className={s.skeleton} style={{ width: "50%" }} />
          </span>
          <span className={s.skeleton} style={{ width: "85%" }} />
          <span className={s.skeleton} style={{ width: "70%" }} />
          <span
            className={s.skeletonDot}
            style={{ width: 22, height: 22, justifySelf: "end" }}
          />
        </div>
      ))}
    </div>
  );
}

/**
 * League · This season, as approved (LS-01 to LS-05): every ranked format in
 * one read-only table, with its rank and rating, progress to the next tier,
 * the season's record and the reward its current tier would earn. More
 * formats than fit scroll inside the table under fixed headings.
 */
export function SeasonPage({ source }: { source: LeagueSource }): ReactElement {
  const ranks = source.status === "ready" ? source.ranks : null;
  let body: ReactElement;
  if (source.status === "loading" || ranks?.state === "loading")
    body = <SkeletonRows />;
  else if (source.status === "signed-out" || source.status === "unregistered")
    body = <p className={s.quiet}>{GAP_COPY[source.status]}</p>;
  else if (source.status === "unavailable")
    body = <p className={s.quiet}>Your ranks are unavailable.</p>;
  else if (ranks && ranks.rows.length > 0)
    body = (
      <div role="rowgroup">
        {ranks.rows.map((row) => (
          <SeasonRow key={row.format.id} row={row} />
        ))}
      </div>
    );
  // A failed first read leaves the body neutral; the notice says why.
  else body = <div className={s.neutral} />;
  return (
    <section className={s.root} aria-label="This season" role="region">
      <LeagueHead title="League" source={source} />
      <div className={s.panel} role="table" aria-label="Your ranks this season">
        <div className={s.headRow} data-table="season" role="row">
          <span role="columnheader">Format</span>
          <span role="columnheader">Rank</span>
          <span role="columnheader">Progress</span>
          <span role="columnheader">Record</span>
          <span
            role="columnheader"
            className={s.end}
            aria-description="The reward your current rank would earn this season"
          >
            Season reward
          </span>
        </div>
        <div className={s.body} tabIndex={0} aria-label="Formats">
          {body}
        </div>
      </div>
      <LeagueNotice source={source} />
    </section>
  );
}

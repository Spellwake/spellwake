import type { ReactElement } from "react";
import {
  EmptyState,
  LoadingShape,
  LoadingState,
  RankIdentity,
  SelectMenu,
} from "@spellwake/ui";
import type { StandingsSource } from "../../league/league-sources";
import * as s from "./league.css";

export function StandingsPage({
  source,
}: {
  source: StandingsSource;
}): ReactElement {
  if (source.status === "loading") {
    return (
      <section className={s.center} aria-label="Standings">
        <LoadingState label="Loading standings…">
          <LoadingShape width={260} height={36} />
        </LoadingState>
      </section>
    );
  }
  if (source.status !== "ready") {
    return (
      <section className={s.center} aria-label="Standings">
        <EmptyState
          title="Standings unavailable"
          description="Sign in to view the seasonal leaderboard."
        />
      </section>
    );
  }

  return (
    <section className={s.page} aria-label="Standings">
      <div className={s.header}>
        <div>
          <h1 className={s.title}>Standings</h1>
          {source.seasonName && (
            <p className={s.subtitle}>{source.seasonName}</p>
          )}
        </div>
        {source.formats.length > 0 && (
          <SelectMenu
            label="Leaderboard format"
            value={source.selectedFormatId}
            onValueChange={source.onSelectFormat}
            options={source.formats.map((f) => ({
              value: f.id,
              label: f.name,
            }))}
          />
        )}
      </div>

      {source.isLoadingRows ? (
        <LoadingState label="Loading leaderboard…">
          <LoadingShape width={320} height={42} />
        </LoadingState>
      ) : source.error ? (
        <EmptyState
          title="Couldn’t load standings"
          description={source.error}
        />
      ) : source.rows.length === 0 ? (
        <EmptyState
          title="No ranked players yet"
          description="Standings appear once ranked matches finish."
        />
      ) : (
        <div className={s.tableWrap}>
          <table className={s.table}>
            <thead>
              <tr>
                <th className={s.th}>#</th>
                <th className={s.th}>Player</th>
                <th className={s.th}>Tier</th>
                <th className={s.th}>Rating</th>
                <th className={`${s.th} ${s.hideMobile}`}>Record</th>
              </tr>
            </thead>
            <tbody>
              {source.rows.map((row) => (
                <tr key={row.accountId}>
                  <td className={s.td}>{row.rankPosition}</td>
                  <td className={`${s.td} ${s.playerCell}`}>
                    <button
                      type="button"
                      className={s.playerButton}
                      onClick={() => source.onOpenProfile(row.accountId)}
                    >
                      {row.username}
                    </button>
                  </td>
                  <td className={s.td}>
                    <RankIdentity
                      tier={row.tier}
                      emblemSrc={row.emblemUrl}
                      presentation="standings"
                    />
                  </td>
                  <td className={s.td}>{row.rating}</td>
                  <td className={`${s.td} ${s.hideMobile}`}>
                    {row.wins}–{row.losses}
                    {row.draws > 0 ? `–${row.draws}` : ""}
                  </td>
                </tr>
              ))}
            </tbody>
          </table>
        </div>
      )}
    </section>
  );
}

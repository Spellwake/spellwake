import { useEffect, type ReactElement } from "react";
import { MessageSquare } from "lucide-react";
import {
  ActionButton,
  Dialog,
  FeedbackMessages,
  LoadingShape,
  LoadingState,
  RankIdentity,
} from "@spellwake/ui";
import type {
  PublicProfileSource,
  RankRow,
} from "../../account/account-sources";
import { Portrait } from "../../account/Portrait";
import { useCommand, useNotice } from "../account/operation";
import * as s from "./profile.css";

const UNRANKED = `${import.meta.env.BASE_URL}ranks/unranked.svg`;
const count = new Intl.NumberFormat("en-US");
const plural = (amount: number, one: string, many: string): string =>
  `${count.format(amount)} ${amount === 1 ? one : many}`;

/** A season record in words, and by letter for a narrow table. */
export function recordText(record: RankRow["record"]): {
  long: string;
  short: string;
} {
  const { wins, losses, draws } = record;
  if (wins + losses + draws === 0)
    return { long: "No matches", short: "No matches" };
  return {
    long: [
      plural(wins, "win", "wins"),
      plural(losses, "loss", "losses"),
      ...(draws > 0 ? [plural(draws, "draw", "draws")] : []),
    ].join(" · "),
    short: [`${wins}W`, `${losses}L`, ...(draws > 0 ? [`${draws}D`] : [])].join(
      " · ",
    ),
  };
}

function Ranks({
  ranks,
}: {
  /** "blank" is a profile that could not be read at all: the body stays empty. */
  ranks: readonly RankRow[] | null | "blank";
}): ReactElement {
  const rows = ranks === "blank" || ranks === null ? [] : ranks;
  return (
    <div className={s.ranks} role="region" aria-label="Ranks" tabIndex={0}>
      <table className={s.table} role="table">
        <thead role="rowgroup">
          <tr role="row">
            <th scope="col" role="columnheader">
              Format
            </th>
            <th scope="col" role="columnheader">
              Rank
            </th>
            <th scope="col" role="columnheader">
              Rating
            </th>
            <th scope="col" role="columnheader">
              Record
            </th>
          </tr>
        </thead>
        <tbody role="rowgroup">
          {rows.map((row) => {
            const text = recordText(row.record);
            return (
              <tr role="row" key={row.format}>
                <td role="cell">{row.format}</td>
                <td role="cell">
                  <RankIdentity
                    presentation="profile"
                    tier={row.tier?.name ?? "Unranked"}
                    emblemSrc={row.tier?.emblemUrl ?? UNRANKED}
                  />
                </td>
                <td role="cell">{count.format(row.rating)}</td>
                <td role="cell" aria-label={text.long}>
                  <span className={s.long} aria-hidden="true">
                    {text.long}
                  </span>
                  <span className={s.short} aria-hidden="true">
                    {text.short}
                  </span>
                </td>
              </tr>
            );
          })}
        </tbody>
      </table>
      {ranks === null ? (
        // Ranks that could not be read are not the same as no ranked matches.
        <p className={s.note}>Ranks unavailable</p>
      ) : (
        ranks !== "blank" &&
        ranks.length === 0 && <p className={s.note}>No ranked matches yet</p>
      )}
    </div>
  );
}

/**
 * A player's public profile: picture, fixed name, and this season's rank in
 * each format. Message appears only where contact is currently permitted, and
 * is otherwise absent, never disabled.
 */
export function PublicProfileDialog({
  source,
  onClose,
}: {
  source: PublicProfileSource;
  onClose: () => void;
}): ReactElement {
  const notice = useNotice();
  const command = useCommand();
  const { say } = notice;
  const hasFailed = source.status === "failed";
  const isStale = source.status === "ready" && source.isStale;
  useEffect(() => {
    if (hasFailed) say("Couldn’t load profile.");
    else if (isStale) say("Couldn’t refresh profile.");
  }, [hasFailed, isStale, say]);

  if (source.status === "unavailable")
    return (
      <Dialog
        open
        title="Player unavailable"
        chrome="bare"
        className={s.unavailableShell}
        onClose={onClose}
      >
        <p className={s.unavailable} data-public-profile="unavailable">
          Player unavailable
        </p>
      </Dialog>
    );
  const ready = source.status === "ready" ? source : null;
  const message = ready?.message ?? null;
  return (
    <Dialog
      open
      title={ready ? `${ready.username}’s profile` : "Player profile"}
      chrome="bare"
      className={s.shell}
      busy={command.isBusy}
      onClose={onClose}
    >
      <div className={s.profile} data-public-profile={source.status}>
        {source.status === "loading" ? (
          <LoadingState label="Loading profile">
            <div className={s.identity}>
              <LoadingShape shape="portrait" width="230px" height="230px" />
              <LoadingShape width="420px" height="52px" />
            </div>
            <div className={s.flourish} />
            <div className={s.skeleton}>
              {[0, 1, 2].map((row) => (
                <LoadingShape key={row} width="100%" height="1.2em" />
              ))}
            </div>
          </LoadingState>
        ) : (
          <>
            {ready && (
              <div className={s.identity}>
                <Portrait
                  portrait={ready.portrait}
                  badge={ready.badge}
                  diameter={230}
                />
                <h3 className={s.username}>{ready.username}</h3>
                {message && (
                  <ActionButton
                    className={s.message}
                    icon={<MessageSquare aria-hidden="true" />}
                    pending={command.isBusy}
                    onClick={() =>
                      void command.run(message).then((outcome) => {
                        if (outcome?.status === "fail") say(outcome.message);
                      })
                    }
                  >
                    Message
                  </ActionButton>
                )}
              </div>
            )}
            <div className={s.flourish} />
            {ready?.season && <p className={s.season}>Season {ready.season}</p>}
            <Ranks ranks={ready ? ready.ranks : "blank"} />
          </>
        )}
        <div className={s.notice}>
          <FeedbackMessages
            label="Profile messages"
            presentation="toast"
            items={notice.items}
          />
        </div>
      </div>
    </Dialog>
  );
}

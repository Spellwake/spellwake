import { useEffect, type ReactElement } from "react";
import { FeedbackMessages } from "@spellwake/ui";
import type { LeagueSource } from "../../league/league-sources";
import { TitleTrail } from "../market/PredictionParts";
import * as s from "./league.css";

const NOTICE_MS = 5000;

/** The page title with its spark trail, and the season it shows. */
export function LeagueHead({
  title,
  source,
}: {
  title: string;
  source: LeagueSource;
}): ReactElement {
  return (
    <header>
      <h1 className={s.title}>
        {title}
        <span className={s.trailSpace}>
          <TitleTrail />
        </span>
      </h1>
      {source.status === "ready" && (
        <p className={s.seasonLine}>
          <span className={s.seasonName}>Season {source.season.label}</span>
          {"  •  "}Ends {source.season.ends} UTC
        </p>
      )}
    </header>
  );
}

/** A read's passive notice: no close and no action, it leaves by itself. */
export function LeagueNotice({
  source,
}: {
  source: LeagueSource;
}): ReactElement {
  const notice = source.status === "ready" ? source.notice : null;
  const dismiss = source.status === "ready" ? source.dismissNotice : null;
  useEffect(() => {
    if (!notice || !dismiss) return;
    const timer = window.setTimeout(dismiss, NOTICE_MS);
    return () => window.clearTimeout(timer);
  }, [notice, dismiss]);
  return (
    <FeedbackMessages
      label="League messages"
      presentation="toast"
      items={notice ? [{ id: "read", text: notice }] : []}
    />
  );
}

export const GAP_COPY = {
  "signed-out": "Sign in to see your ranks.",
  unregistered: "Finish creating your account to see your ranks.",
} as const;

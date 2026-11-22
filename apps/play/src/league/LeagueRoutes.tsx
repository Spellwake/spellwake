import { useCallback, useEffect, type ReactElement } from "react";
import { useAtom } from "jotai";
import { atomWithStorage } from "jotai/utils";
import { parseAsString, useQueryState } from "nuqs";
import { useAccount } from "../auth/AccountProvider";
import { SeasonPage } from "../routes/league/SeasonPage";
import { StandingsPage } from "../routes/league/StandingsPage";
import { useConnectedLeague } from "./connected-league";
import { usePreviewLeague } from "./demo-league";
import type { LeagueSource } from "./league-sources";

const useIsPreview = () => useAccount().state.status === "unconfigured";

function PreviewSeason(): ReactElement {
  return <SeasonPage source={usePreviewLeague(null)} />;
}
function AccountSeason(): ReactElement {
  return <SeasonPage source={useConnectedLeague(null)} />;
}
/** League · This season, from the account where an application API is named, else the preview. */
export function SeasonRoute(): ReactElement {
  return useIsPreview() ? <PreviewSeason /> : <AccountSeason />;
}

/** The Standings format last viewed, kept for the next visit. */
const rememberedFormat = atomWithStorage<string | null>(
  "spellwake:league:standings-format:v1",
  null,
  undefined,
  { getOnInit: true },
);
function useStandingsFormat() {
  const [param, setParam] = useQueryState("format", parseAsString);
  const [remembered, setRemembered] = useAtom(rememberedFormat);
  const choose = useCallback(
    (formatId: string, replace = false) => {
      void setParam(formatId, { history: replace ? "replace" : "push" });
      setRemembered(formatId);
    },
    [setParam, setRemembered],
  );
  return { wanted: param ?? remembered, choose };
}
function Standings({
  source,
  wanted,
  choose,
}: {
  source: LeagueSource;
  wanted: string | null;
  choose: (formatId: string, replace?: boolean) => void;
}): ReactElement {
  const formats = source.status === "ready" ? source.formats : [];
  // An unknown or missing format opens the first one.
  const chosen =
    formats.find((entry) => entry.id === wanted)?.id ?? formats[0]?.id ?? null;
  useEffect(() => {
    if (chosen && chosen !== wanted) choose(chosen, true);
  }, [chosen, wanted, choose]);
  return <StandingsPage source={source} format={chosen} onFormat={choose} />;
}
function PreviewStandings(): ReactElement {
  const { wanted, choose } = useStandingsFormat();
  return (
    <Standings
      source={usePreviewLeague(wanted)}
      wanted={wanted}
      choose={choose}
    />
  );
}
function AccountStandings(): ReactElement {
  const { wanted, choose } = useStandingsFormat();
  return (
    <Standings
      source={useConnectedLeague(wanted)}
      wanted={wanted}
      choose={choose}
    />
  );
}
/** League · Standings, from the account where an application API is named, else the preview. */
export function StandingsRoute(): ReactElement {
  return useIsPreview() ? <PreviewStandings /> : <AccountStandings />;
}

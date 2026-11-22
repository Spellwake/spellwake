import type { ReactElement } from "react";
import { useAccount } from "../auth/AccountProvider";
import { SeasonPage } from "../routes/league/SeasonPage";
import { StandingsPage } from "../routes/league/StandingsPage";
import { useConnectedSeason, useConnectedStandings } from "./connected-league";
import { useDemoSeason, useDemoStandings } from "./demo-league";

function PreviewSeason(): ReactElement {
  return <SeasonPage source={useDemoSeason()} />;
}
function AccountSeason(): ReactElement {
  return <SeasonPage source={useConnectedSeason()} />;
}
function PreviewStandings(): ReactElement {
  return <StandingsPage source={useDemoStandings()} />;
}
function AccountStandings(): ReactElement {
  return <StandingsPage source={useConnectedStandings()} />;
}

export function SeasonRoute(): ReactElement {
  return useAccount().state.status === "unconfigured" ? (
    <PreviewSeason />
  ) : (
    <AccountSeason />
  );
}

export function StandingsRoute(): ReactElement {
  return useAccount().state.status === "unconfigured" ? (
    <PreviewStandings />
  ) : (
    <AccountStandings />
  );
}

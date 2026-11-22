import { useState, type ReactElement } from "react";
import { useSearchParams } from "react-router-dom";
import type {
  AccountSettingsSource,
  PublicProfileSource,
  PurchaseHistory,
} from "../../account/account-sources";
import { succeeded } from "../../decks/deck-sources";
import {
  AccountSettingsDialog,
  type AccountSection,
} from "../account/AccountSettingsDialog";
import { PublicProfileDialog } from "../profile/PublicProfileDialog";

const noop = (): void => undefined;
const done = () => Promise.resolve(succeeded(undefined));
const purchases = (state: PurchaseHistory["state"]): PurchaseHistory => ({
  state,
  rows: [],
  loadMore: null,
});
type Ready = Extract<AccountSettingsSource, { status: "ready" }>;
/** An account as a configured build reads it today: a name, and no way to change the rest. */
const base: Ready = {
  status: "ready",
  username: "Planeswalker",
  portrait: null,
  badge: "none",
  picture: null,
  badges: null,
  patreon: {
    link: "unknown",
    connect: null,
    manage: null,
    isLinking: false,
    problem: null,
    dismissProblem: noop,
  },
  billing: { manage: null, purchases: purchases("ready") },
  deletion: { state: "none", request: done },
};
const SETTINGS: Record<
  string,
  { source: AccountSettingsSource; section: AccountSection }
> = {
  loading: { source: { status: "loading" }, section: "profile" },
  unavailable: { source: { status: "unavailable" }, section: "profile" },
  "no-commands": { source: base, section: "profile" },
  "patreon-unknown": { source: base, section: "patreon" },
  "patreon-inactive": {
    source: {
      ...base,
      patreon: { ...base.patreon, link: "inactive", manage: done },
    },
    section: "patreon",
  },
  "billing-empty": { source: base, section: "billing" },
  "billing-loading": {
    source: {
      ...base,
      billing: { manage: null, purchases: purchases("loading") },
    },
    section: "billing",
  },
  "billing-failed": {
    source: {
      ...base,
      billing: { manage: null, purchases: purchases("failed") },
    },
    section: "billing",
  },
  "deletion-requested": {
    source: { ...base, deletion: { state: "requested", request: done } },
    section: "account",
  },
};
const player: Extract<PublicProfileSource, { status: "ready" }> = {
  status: "ready",
  username: "EmberFox",
  portrait: null,
  badge: "none",
  season: "Oct–Dec 2026",
  ranks: [
    {
      format: "Modern",
      tier: null,
      rating: 1000,
      record: { wins: 0, losses: 0, draws: 0 },
    },
  ],
  isStale: false,
  message: null,
};
const PROFILES: Record<string, PublicProfileSource> = {
  "profile-loading": { status: "loading" },
  "profile-failed": { status: "failed" },
  "profile-stale": { ...player, isStale: true },
  "profile-ranks-unavailable": { ...player, ranks: null },
  "profile-unavailable": { status: "unavailable" },
};

/**
 * Account probe, development only. It shows the Account settings and profile
 * states the preview person cannot reach, chosen by `?state=`, so they can be
 * reviewed and stress-tested without an account. Nothing here is saved.
 */
export function AccountProbe(): ReactElement | null {
  const [parameters] = useSearchParams();
  const state = parameters.get("state") ?? "loading";
  const settings = SETTINGS[state];
  const [section, setSection] = useState<AccountSection>(
    settings?.section ?? "profile",
  );
  const profile = PROFILES[state];
  if (profile) return <PublicProfileDialog source={profile} onClose={noop} />;
  return settings ? (
    <AccountSettingsDialog
      source={settings.source}
      section={section}
      onSectionChange={setSection}
      onClose={noop}
    />
  ) : null;
}

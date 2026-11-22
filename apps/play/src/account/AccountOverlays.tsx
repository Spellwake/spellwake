import type { ReactElement, RefObject } from "react";
import {
  parseAsBoolean,
  parseAsString,
  parseAsStringLiteral,
  useQueryState,
} from "nuqs";
import { useAccount } from "../auth/AccountProvider";
import type { ApplicationServices } from "../auth/services";
import type { SourceGap } from "../decks/deck-sources";
import {
  ACCOUNT_SECTIONS,
  AccountSettingsDialog,
  type AccountSection,
} from "../routes/account/AccountSettingsDialog";
import { PublicProfileDialog } from "../routes/profile/PublicProfileDialog";
import {
  useConnectedAccountSettings,
  useConnectedPublicProfile,
  type RegisteredSession,
} from "./connected-account";
import { useDemoAccountSettings, useDemoPublicProfile } from "./demo-account";
import {
  useConnectedCosmetics,
  useConnectedDefaultSettings,
} from "../cosmetics/connected-cosmetics";
import {
  useDemoCosmetics,
  useDemoDefaultSettings,
} from "../cosmetics/demo-cosmetics";
import {
  COSMETIC_VIEWS,
  CosmeticsDialog,
  type CosmeticView,
} from "../routes/cosmetics/CosmeticsDialog";
import { DefaultSettingsDialog } from "../routes/cosmetics/DefaultSettingsDialog";
import {
  POLICY_SECTIONS,
  PolicyCommitmentDialog,
} from "../routes/policy/PolicyCommitmentDialog";

/**
 * Account settings and a player's profile open over whatever screen the
 * person is on, and the address says which is open: `?account=` names the
 * section of Account settings, and `?player=` names whose profile. Closing
 * clears the address, so Back and a shared link both behave.
 */
const section = parseAsStringLiteral(ACCOUNT_SECTIONS);

const cosmeticView = parseAsStringLiteral(COSMETIC_VIEWS);
const policySection = parseAsStringLiteral(POLICY_SECTIONS);

/** Opens Policy & Commitment at Community agreement. `?policy=` names the section. */
export function useOpenPolicy(): () => void {
  const [, setSection] = useQueryState("policy", policySection);
  return () => void setSection("agreement");
}

/** Opens Cosmetics at Your cosmetics, and Default settings. */
export function useOpenCosmetics(): {
  openCosmetics: () => void;
  openDefaults: () => void;
} {
  const [, setView] = useQueryState("cosmetics", cosmeticView);
  const [, setDefaults] = useQueryState("defaults", parseAsBoolean);
  return {
    openCosmetics: () => void setView("owned"),
    openDefaults: () => void setDefaults(true),
  };
}

type Closable = {
  returnFocusRef: RefObject<HTMLElement | null>;
  onClose: () => void;
};
function PreviewDefaults(props: Closable): ReactElement {
  return <DefaultSettingsDialog {...props} source={useDemoDefaultSettings()} />;
}
function AccountDefaults(props: Closable): ReactElement {
  return (
    <DefaultSettingsDialog {...props} source={useConnectedDefaultSettings()} />
  );
}
type CosmeticsProps = Closable & {
  view: CosmeticView;
  onViewChange: (view: CosmeticView) => void;
};
function PreviewCosmetics(props: CosmeticsProps): ReactElement {
  return <CosmeticsDialog {...props} source={useDemoCosmetics()} />;
}
function AccountCosmetics(props: CosmeticsProps): ReactElement {
  return <CosmeticsDialog {...props} source={useConnectedCosmetics()} />;
}

/** Opens Account settings at Profile, which is what View profile leads to. */
export function useOpenAccountSettings(): () => void {
  const [, setSection] = useQueryState("account", section);
  return () => void setSection("profile");
}

type SettingsProps = {
  returnFocusRef: RefObject<HTMLElement | null>;
  section: AccountSection;
  onSectionChange: (section: AccountSection) => void;
  onClose: () => void;
};
function PreviewSettings(props: SettingsProps): ReactElement {
  return <AccountSettingsDialog {...props} source={useDemoAccountSettings()} />;
}
function SignedInSettings({
  owner,
  services,
  ...props
}: SettingsProps & {
  owner: RegisteredSession;
  services: ApplicationServices;
}): ReactElement {
  return (
    <AccountSettingsDialog
      {...props}
      source={useConnectedAccountSettings(owner, services)}
    />
  );
}
function AccountSettings(props: SettingsProps): ReactElement {
  const { state, services } = useAccount();
  if (state.status === "unconfigured") return <PreviewSettings {...props} />;
  if (state.status === "ready" && state.session.accountId && services)
    return (
      <SignedInSettings {...props} owner={state.session} services={services} />
    );
  const gap: SourceGap =
    state.status === "loading"
      ? "loading"
      : state.status === "signed_out"
        ? "signed-out"
        : state.status === "ready"
          ? "unregistered"
          : "unavailable";
  return <AccountSettingsDialog {...props} source={{ status: gap }} />;
}

function PreviewProfile({
  playerId,
  onClose,
}: {
  playerId: string;
  onClose: () => void;
}): ReactElement {
  return (
    <PublicProfileDialog
      source={useDemoPublicProfile(playerId)}
      onClose={onClose}
    />
  );
}

function ConnectedProfile({
  playerId,
  owner,
  services,
  onClose,
}: {
  playerId: string;
  owner: RegisteredSession;
  services: ApplicationServices;
  onClose: () => void;
}): ReactElement {
  return (
    <PublicProfileDialog
      source={useConnectedPublicProfile(playerId, owner, services)}
      onClose={onClose}
    />
  );
}

function AccountProfile({
  playerId,
  onClose,
}: {
  playerId: string;
  onClose: () => void;
}): ReactElement {
  const { state, services } = useAccount();
  if (state.status === "unconfigured")
    return <PreviewProfile playerId={playerId} onClose={onClose} />;
  if (state.status === "ready" && state.session.accountId && services)
    return (
      <ConnectedProfile
        playerId={playerId}
        owner={state.session as RegisteredSession}
        services={services}
        onClose={onClose}
      />
    );
  return (
    <PublicProfileDialog
      source={{
        status: state.status === "loading" ? "loading" : "unavailable",
      }}
      onClose={onClose}
    />
  );
}

/** Mounted once by the application frame, for every signed-in screen. */
export function AccountOverlays({
  accountButton,
}: {
  /** The header's account control, where focus returns when a dialog closes. */
  accountButton: RefObject<HTMLElement | null>;
}): ReactElement {
  const [open, setOpen] = useQueryState("account", section);
  const [player, setPlayer] = useQueryState("player", parseAsString);
  const [view, setView] = useQueryState("cosmetics", cosmeticView);
  const [isDefaultsOpen, setDefaults] = useQueryState(
    "defaults",
    parseAsBoolean,
  );
  const [policy, setPolicy] = useQueryState("policy", policySection);
  const isPreview = useAccount().state.status === "unconfigured";
  return (
    <>
      {policy && (
        <PolicyCommitmentDialog
          returnFocusRef={accountButton}
          section={policy}
          onSectionChange={(next) => void setPolicy(next)}
          onClose={() => void setPolicy(null)}
        />
      )}
      {open && (
        <AccountSettings
          returnFocusRef={accountButton}
          section={open}
          onSectionChange={(next) => void setOpen(next)}
          onClose={() => void setOpen(null)}
        />
      )}
      {isDefaultsOpen &&
        (isPreview ? (
          <PreviewDefaults
            returnFocusRef={accountButton}
            onClose={() => void setDefaults(null)}
          />
        ) : (
          <AccountDefaults
            returnFocusRef={accountButton}
            onClose={() => void setDefaults(null)}
          />
        ))}
      {view &&
        (isPreview ? (
          <PreviewCosmetics
            returnFocusRef={accountButton}
            view={view}
            onViewChange={(next) => void setView(next)}
            onClose={() => void setView(null)}
          />
        ) : (
          <AccountCosmetics
            returnFocusRef={accountButton}
            view={view}
            onViewChange={(next) => void setView(next)}
            onClose={() => void setView(null)}
          />
        ))}
      {player && (
        <AccountProfile
          playerId={player}
          onClose={() => void setPlayer(null)}
        />
      )}
    </>
  );
}

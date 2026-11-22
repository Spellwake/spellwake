import {
  useLayoutEffect,
  useRef,
  useState,
  type ReactElement,
  type ReactNode,
  type Ref,
  type RefObject,
} from "react";
import { Link, useNavigate } from "react-router-dom";
import {
  ChevronDown,
  CircleUserRound,
  LogOut,
  MessageSquare,
  Palette,
  Settings,
} from "lucide-react";
import {
  ActionMenu,
  CoinAmount,
  designPixels,
  type MenuCommand,
} from "@spellwake/ui";
import { destinations, type Destination } from "../../routes/navigation";
import type { OwnIdentity } from "../../account/account-sources";
import { CroppedArt, SupporterMark } from "../../account/Portrait";
import { badge as portraitBadge } from "../../account/portrait.css";
import * as s from "./shell.css";

const asset = (path: string): string => `${import.meta.env.BASE_URL}${path}`;

const SIGN_OUT = "sign-out";
const VIEW_PROFILE = "view-profile";
const COSMETICS = "cosmetics";
const DEFAULTS = "default-settings";
/** The account menu, as approved: identity, cosmetics, defaults, then Sign out apart. */
const accountCommands = (
  username: string | null,
  canSignOut: boolean,
): MenuCommand[] => [
  {
    id: VIEW_PROFILE,
    label: username ?? "View profile",
    ...(username ? { detail: "View profile" } : {}),
    leading: <CircleUserRound />,
  },
  {
    id: COSMETICS,
    label: "Cosmetics",
    leading: <Palette />,
    dividerBefore: true,
  },
  { id: DEFAULTS, label: "Default settings", leading: <Settings /> },
  // Offered only where there is an account to leave.
  ...(canSignOut
    ? [
        {
          id: SIGN_OUT,
          label: "Sign out",
          leading: <LogOut />,
          dividerBefore: true,
        },
      ]
    : []),
];

/** Desktop destinations with one gold marker that travels to the current one. */
function DestinationNav({
  current,
}: {
  current: Destination | undefined;
}): ReactElement {
  const nav = useRef<HTMLElement>(null);
  const [marker, setMarker] = useState<{ x: number; width: number } | null>(
    null,
  );
  const [isSettled, setIsSettled] = useState(false);
  useLayoutEffect(() => {
    const host = nav.current;
    if (!host) return;
    const measure = (): void => {
      const active = host.querySelector<HTMLElement>('[aria-current="page"]');
      // Offsets are layout pixels, so the marker stays exact under display scaling.
      setMarker(
        active ? { x: active.offsetLeft, width: active.offsetWidth } : null,
      );
    };
    measure();
    const observer = new ResizeObserver(measure);
    observer.observe(host);
    void document.fonts.ready.then(measure);
    return () => observer.disconnect();
  }, [current?.id]);
  useLayoutEffect(() => {
    if (!marker || isSettled) return;
    // The first placement appears in position. Later changes travel.
    const frame = requestAnimationFrame(() => setIsSettled(true));
    return () => cancelAnimationFrame(frame);
  }, [marker, isSettled]);
  return (
    <nav ref={nav} className={s.destinationNav} aria-label="Application">
      {destinations.map((destination) => (
        <Link
          key={destination.id}
          to={destination.path}
          className={s.destinationLink}
          aria-current={destination.id === current?.id ? "page" : undefined}
        >
          {destination.label}
        </Link>
      ))}
      {marker && (
        <span
          aria-hidden="true"
          className={s.marker}
          data-ready={isSettled}
          style={{
            transform: `translateX(${marker.x}px)`,
            width: marker.width,
          }}
        />
      )}
    </nav>
  );
}

/** Application header shared by every device layout. Compact layouts hide the destination labels. */
export function AppHeader({
  current,
  coins,
  username,
  identity,
  accountRef,
  onViewProfile,
  onOpenCosmetics,
  onOpenDefaults,
  onToggleMessages,
  isMessagesOpen,
  messagesRef,
  onSignOut,
  hasDestinations,
  children,
}: {
  current: Destination | undefined;
  coins: number | null;
  username: string | null;
  identity: OwnIdentity;
  /** The account control, so a dialog it leads to can hand focus back. */
  accountRef: Ref<HTMLButtonElement>;
  /** Opens Account settings at Profile. */
  onViewProfile: () => void;
  onOpenCosmetics: () => void;
  onOpenDefaults: () => void;
  /** Opens or closes Messages under its button. */
  onToggleMessages: () => void;
  isMessagesOpen: boolean;
  messagesRef: RefObject<HTMLButtonElement | null>;
  /** Null where there is no account to leave, which keeps Sign out off the menu. */
  onSignOut: (() => void) | null;
  /** False on compact layouts, where the dock carries the destinations. */
  hasDestinations: boolean;
  /** Navigation that shares the header, between the mark and the account actions. */
  children?: ReactNode;
}): ReactElement {
  const navigate = useNavigate();
  return (
    <header className={s.header} data-frame="header">
      <Link to="/" className={s.logo} aria-label="Spellwake home">
        <img className={s.logoMark} src={asset("logo.svg")} alt="" />
      </Link>
      {hasDestinations && <DestinationNav current={current} />}
      {children}
      <div className={s.headerActions}>
        <span className={s.balance}>
          <CoinAmount amount={coins} iconSrc={asset("coin.svg")} />
        </span>
        <button
          ref={messagesRef}
          type="button"
          className={s.iconAction}
          aria-label="Messages"
          aria-expanded={isMessagesOpen}
          onClick={onToggleMessages}
        >
          <MessageSquare size={designPixels(31)} strokeWidth={1.5} />
        </button>
        <ActionMenu
          label="Account"
          commands={accountCommands(username, onSignOut !== null)}
          onCommand={(command) => {
            if (command === SIGN_OUT) onSignOut?.();
            else if (command === VIEW_PROFILE) onViewProfile();
            else if (command === COSMETICS) onOpenCosmetics();
            else if (command === DEFAULTS) onOpenDefaults();
            else void navigate(command);
          }}
          trigger={
            <button
              ref={accountRef}
              type="button"
              className={s.account}
              aria-label="Account"
            >
              <span className={s.portrait}>
                {identity.portrait ? (
                  <span className={s.portraitArt}>
                    <CroppedArt {...identity.portrait} />
                  </span>
                ) : (
                  <CircleUserRound strokeWidth={1.1} />
                )}
                {identity.badge === "supporter" && (
                  <span className={portraitBadge} data-badge="supporter">
                    <SupporterMark />
                  </span>
                )}
              </span>
              <ChevronDown className={s.chevron} size={designPixels(24)} />
            </button>
          }
        />
      </div>
    </header>
  );
}

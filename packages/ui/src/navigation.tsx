import { type ReactElement, type ReactNode } from "react";
import {
  Bell,
  ChevronDown,
  Coins,
  Layers,
  Library,
  Package,
  Settings,
  LogOut,
  UserRound,
} from "lucide-react";
import { IconButton, Menu } from "./components";
import * as s from "./navigation.css";

export type NavigationItem = { id: string; label: string; icon?: ReactNode };
export const mainNavigation = [
  { id: "play", label: "Open Play" },
  { id: "collection", label: "Collection" },
  { id: "market", label: "Market" },
  { id: "league", label: "League" },
];
export const libraryNavigation = [
  { id: "decks", label: "Decks", icon: <Layers size={24} strokeWidth={1.4} /> },
  {
    id: "cards",
    label: "Cards",
    icon: <Library size={24} strokeWidth={1.4} />,
  },
  {
    id: "packs",
    label: "Packs",
    icon: <Package size={24} strokeWidth={1.4} />,
  },
];

/** Header carries navigation only; the host owns route/account data and actions. */
export function TopNavigation({
  logoUrl,
  activeId,
  onNavigate,
  items = mainNavigation,
  coins,
  avatarUrl,
  onAccountAction,
  onNotifications,
}: {
  logoUrl: string;
  activeId: string;
  onNavigate: (id: string) => void;
  items?: NavigationItem[];
  coins: number;
  avatarUrl?: string;
  onAccountAction: (action: string) => void;
  onNotifications: () => void;
}): ReactElement {
  return (
    <header className={s.topbar}>
      <a className={s.brand} href="#" aria-label="Spellwake home">
        <img className={s.logo} src={logoUrl} alt="" />
      </a>
      <nav aria-label="Main navigation" className={s.links}>
        {items.map((item) => (
          <button
            type="button"
            key={item.id}
            className={s.topItem}
            aria-current={activeId === item.id ? "page" : undefined}
            onClick={() => onNavigate(item.id)}
          >
            {item.label}
          </button>
        ))}
      </nav>
      <div className={s.account}>
        <span className={s.coin} aria-label={`${coins.toLocaleString()} coins`}>
          <Coins className={s.coinIcon} size={23} strokeWidth={1.6} />
          {coins.toLocaleString()}
        </span>
        <IconButton
          label="Notifications"
          variant="transparent"
          onClick={onNotifications}
        >
          <Bell size={24} />
        </IconButton>
        <Menu
          label="Account menu"
          trigger={
            <IconButton
              label="Open account menu"
              variant="transparent"
              style={{ width: 78 }}
            >
              {avatarUrl ? (
                <img className={s.avatar} src={avatarUrl} alt="" />
              ) : (
                <UserRound size={25} />
              )}
              <ChevronDown size={16} />
            </IconButton>
          }
          entries={[
            { kind: "label", text: "Your account" },
            {
              kind: "item",
              text: "Profile",
              icon: <UserRound size={17} />,
              onSelect: () => onAccountAction("Profile"),
            },
            {
              kind: "item",
              text: "Settings",
              icon: <Settings size={17} />,
              onSelect: () => onAccountAction("Settings"),
            },
            { kind: "divider", id: "account-separator" },
            {
              kind: "item",
              text: "Sign out",
              icon: <LogOut size={17} />,
              onSelect: () => onAccountAction("Sign out"),
            },
          ]}
        />
      </div>
    </header>
  );
}
export function SideNavigation({
  activeId,
  onNavigate,
  items = libraryNavigation,
}: {
  activeId: string;
  onNavigate: (id: string) => void;
  items?: NavigationItem[];
}): ReactElement {
  return (
    <nav aria-label="Collection navigation" className={s.side}>
      {items.map((item) => (
        <button
          type="button"
          key={item.id}
          className={s.sideItem}
          aria-current={activeId === item.id ? "page" : undefined}
          onClick={() => onNavigate(item.id)}
        >
          <span className={s.sideIcon}>{item.icon}</span>
          {item.label}
        </button>
      ))}
    </nav>
  );
}

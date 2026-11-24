import type { ReactElement } from "react";
import {
  createBrowserRouter,
  Navigate,
  RouterProvider,
  type RouteObject,
} from "react-router-dom";
import { NuqsAdapter } from "nuqs/adapters/react-router/v7";
import { AppShell } from "../components/shell/AppShell";
import { DeletionRecovery } from "../account/DeletionRecovery";
import { AccountProvider } from "../auth/AccountProvider";
import { CardsRoute, PacksRoute } from "../collection/CollectionRoutes";
import { DeckBuilderRoute, DeckShelfRoute } from "../decks/DeckRoutes";
import {
  HistoryRoute,
  SeasonRoute,
  StandingsRoute,
} from "../league/LeagueRoutes";
import { LobbyRoute } from "../lobby/LobbyRoutes";
import {
  ListingsRoute,
  PredictionsRoute,
  SupplyRoute,
} from "../market/MarketRoutes";
import { MatchRoute } from "../match/MatchRoutes";
import { TradeRoute } from "../trade/TradeRoutes";
import {
  AccountGate,
  AuthCallbackRoute,
  EntryRoute,
  UsernameRoute,
} from "./entry/EntryRoutes";
import type { FrameHandle } from "./frame-mode";
import { PatreonReturnRoute } from "./patreon/PatreonReturnRoute";

/** A screen of the signed-in app. In a configured build it waits behind account entry. */
const gated = (element: ReactElement): ReactElement => (
  <AccountGate>{element}</AccountGate>
);
const entry = { frame: "focus" } satisfies FrameHandle;
const immersive = { frame: "immersive" } satisfies FrameHandle;

/**
 * Development-only probes for stressing individual layouts. A production build drops them.
 */
const probes: RouteObject[] = import.meta.env.DEV
  ? [
      {
        path: "probe/match",
        handle: immersive,
        lazy: async () => ({
          Component: (await import("./probes/MatchProbe")).MatchProbe,
        }),
      },
      {
        path: "probe/table",
        handle: immersive,
        lazy: async () => ({
          Component: (await import("./probes/MatchTableProbe")).MatchTableProbe,
        }),
      },
      {
        path: "probe/entry",
        handle: entry,
        lazy: async () => ({
          Component: (await import("./probes/EntryProbe")).EntryProbe,
        }),
      },
      {
        path: "probe/messages",
        lazy: async () => ({
          Component: (await import("./probes/MessagesProbe")).MessagesProbe,
        }),
      },
      {
        path: "probe/lobby",
        lazy: async () => ({
          Component: (await import("./probes/LobbyProbe")).LobbyProbe,
        }),
      },
      {
        path: "probe/market",
        lazy: async () => ({
          Component: (await import("./probes/MarketProbe")).MarketProbe,
        }),
      },
      {
        path: "probe/league",
        lazy: async () => ({
          Component: (await import("./probes/LeagueProbe")).LeagueProbe,
        }),
      },
      {
        path: "probe/predictions",
        lazy: async () => ({
          Component: (await import("./probes/PredictionsProbe"))
            .PredictionsProbe,
        }),
      },
      {
        path: "probe/supply",
        lazy: async () => ({
          Component: (await import("./probes/SupplyProbe")).SupplyProbe,
        }),
      },
      {
        path: "probe/trade",
        lazy: async () => ({
          Component: (await import("./probes/TradeProbe")).TradeProbe,
        }),
      },
      {
        path: "probe/collection",
        lazy: async () => ({
          Component: (await import("./probes/CollectionProbe")).CollectionProbe,
        }),
      },
      {
        path: "probe/account",
        lazy: async () => ({
          Component: (await import("./probes/AccountProbe")).AccountProbe,
        }),
      },
    ]
  : [];

const router = createBrowserRouter(
  [
    {
      element: <AppShell />,
      children: [
        { index: true, element: <Navigate to="/collection/decks" replace /> },
        { path: "open-play", element: gated(<DeckShelfRoute mode="open" />) },
        { path: "open-play/lobby", element: gated(<LobbyRoute mode="open" />) },
        {
          path: "collection/lobby",
          element: gated(<LobbyRoute mode="collection" />),
        },
        {
          path: "collection",
          element: <Navigate to="/collection/decks" replace />,
        },
        {
          path: "collection/decks",
          element: gated(<DeckShelfRoute mode="collection" />),
        },
        { path: "collection/cards", element: gated(<CardsRoute />) },
        { path: "collection/packs", element: gated(<PacksRoute />) },
        { path: "collection/trade/:personId", element: gated(<TradeRoute />) },
        { path: "market/listings", element: gated(<ListingsRoute />) },
        { path: "market/predictions", element: gated(<PredictionsRoute />) },
        { path: "market/supply", element: gated(<SupplyRoute />) },
        { path: "league/season", element: gated(<SeasonRoute />) },
        { path: "league/standings", element: gated(<StandingsRoute />) },
        { path: "league/history", element: gated(<HistoryRoute />) },
        {
          path: "decks/import",
          element: <Navigate to="/collection/decks?import=true" replace />,
        },
        {
          path: "decks/:deckId",
          handle: entry,
          element: gated(<DeckBuilderRoute />),
        },
        {
          path: "sign-in",
          handle: entry,
          element: <EntryRoute mode="sign-in" />,
        },
        {
          path: "sign-up",
          handle: entry,
          element: <EntryRoute mode="sign-up" />,
        },
        {
          path: "auth/callback",
          handle: entry,
          element: <AuthCallbackRoute />,
        },
        { path: "username", handle: entry, element: <UsernameRoute /> },
        // Where Patreon returns a person. It only reopens Account settings.
        {
          path: "patreon/return",
          handle: entry,
          element: <PatreonReturnRoute />,
        },
        { path: "lobby", element: <Navigate to="/open-play/lobby" replace /> },
        { path: "match", handle: immersive, element: gated(<MatchRoute />) },
        {
          path: "match/:gameId",
          handle: immersive,
          element: gated(<MatchRoute />),
        },
        // View profile and Account settings are one dialog, opened over a screen.
        {
          path: "profile",
          element: <Navigate to="/collection/decks?account=profile" replace />,
        },
        {
          path: "cosmetics",
          element: <Navigate to="/collection/decks?cosmetics=owned" replace />,
        },
        {
          path: "default-settings",
          element: <Navigate to="/collection/decks?defaults=true" replace />,
        },
        {
          path: "account",
          element: <Navigate to="/collection/decks?account=profile" replace />,
        },
        ...probes,
        { path: "*", element: <Navigate to="/collection/decks" replace /> },
      ],
    },
  ],
  { basename: import.meta.env.BASE_URL.replace(/\/$/, "") || "/" },
);

/**
 * Routes for the shared browser and Tauri application. The account provider
 * owns the one query cache, so a change of sign-in clears every owner's data.
 */
export function Application(): ReactElement {
  return (
    <AccountProvider>
      <DeletionRecovery />
      <NuqsAdapter>
        <RouterProvider router={router} />
      </NuqsAdapter>
    </AccountProvider>
  );
}

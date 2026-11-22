import type { ReactElement } from "react";
import { Scale, Swords, Trophy } from "lucide-react";
import {
  CardsIcon,
  DecksIcon,
  ListingsIcon,
  PacksIcon,
  PredictionsIcon,
  SeasonIcon,
  StandingsIcon,
  SupplyIcon,
} from "@spellwake/ui";

export type SectionLink = {
  path: string;
  label: string;
  icon: ReactElement;
};
export type Destination = {
  id: "open-play" | "collection" | "market" | "league";
  label: string;
  /** Where the destination opens. Sections remember nothing between visits yet. */
  path: string;
  /** Compact-layout glyph. Desktop destinations are labels only. */
  icon: ReactElement;
  sections: readonly SectionLink[];
};

/** Primary destinations in reading order. Route transitions follow this order. */
export const destinations: readonly Destination[] = [
  {
    id: "open-play",
    label: "Open Play",
    path: "/open-play",
    icon: <Swords />,
    sections: [],
  },
  {
    id: "collection",
    label: "Collection",
    path: "/collection/decks",
    icon: <DecksIcon />,
    sections: [
      { path: "/collection/decks", label: "Decks", icon: <DecksIcon /> },
      { path: "/collection/cards", label: "Cards", icon: <CardsIcon /> },
      { path: "/collection/packs", label: "Packs", icon: <PacksIcon /> },
    ],
  },
  {
    id: "market",
    label: "Market",
    path: "/market/listings",
    icon: <Scale />,
    sections: [
      { path: "/market/listings", label: "Listings", icon: <ListingsIcon /> },
      {
        path: "/market/predictions",
        label: "Predictions",
        icon: <PredictionsIcon />,
      },
      { path: "/market/supply", label: "Supply", icon: <SupplyIcon /> },
    ],
  },
  {
    id: "league",
    label: "League",
    path: "/league/season",
    icon: <Trophy />,
    sections: [
      { path: "/league/season", label: "This season", icon: <SeasonIcon /> },
      {
        path: "/league/standings",
        label: "Standings",
        icon: <StandingsIcon />,
      },
    ],
  },
];

/** The destination that owns a pathname, if any. */
export function destinationFor(pathname: string): Destination | undefined {
  const head = pathname.split("/")[1];
  return destinations.find((destination) => destination.id === head);
}

/** Signed distance between two destinations in reading order. */
export function travelDirection(from: string, to: string): -1 | 0 | 1 {
  const start = destinations.findIndex((d) => d === destinationFor(from));
  const end = destinations.findIndex((d) => d === destinationFor(to));
  if (start < 0 || end < 0 || start === end) return 0;
  return end > start ? 1 : -1;
}

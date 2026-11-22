import {
  CardsIcon,
  DecksIcon,
  ListingsIcon,
  PacksIcon,
  PredictionsIcon,
  SeasonIcon,
  SectionNavigation,
  StandingsIcon,
  SupplyIcon,
} from "@spellwake/ui";

const sections = {
  Collection: [
    { id: "DL-01", label: "Decks", icon: <DecksIcon /> },
    { id: "CLC-01", label: "Cards", icon: <CardsIcon /> },
    { id: "CP-01", label: "Packs", icon: <PacksIcon /> },
  ],
  Market: [
    { id: "ML-01", label: "Listings", icon: <ListingsIcon /> },
    { id: "EX-01", label: "Predictions", icon: <PredictionsIcon /> },
    { id: "SP-01", label: "Supply", icon: <SupplyIcon /> },
  ],
  League: [
    { id: "LS-01", label: "This season", icon: <SeasonIcon /> },
    { id: "ST-01", label: "Standings", icon: <StandingsIcon /> },
  ],
} as const;

export type RailArea = keyof typeof sections;

export function railAreaFor(view: string): RailArea {
  const id = view.split(":")[0];
  const area = (Object.keys(sections) as RailArea[]).find((area) =>
    sections[area].some((item) => item.id === id),
  );
  if (!area) throw new Error("Unknown section destination: " + view);
  return area;
}

const reviewHref = (id: string) =>
  `?${window === window.parent ? "scene" : "review"}=${id}`;
export function SectionRail({
  area,
  current,
  go,
  hrefFor = reviewHref,
}: {
  area: RailArea;
  current: string;
  go: (id: string) => void;
  hrefFor?: (id: string) => string;
}) {
  const items = sections[area].map((item) => ({
    ...item,
    href: hrefFor(item.id),
  }));
  return (
    <SectionNavigation
      label={area}
      links={items}
      currentHref={hrefFor(current)}
      onNavigate={(event, link) => {
        const destination = items.find((item) => item.href === link.href);
        if (destination) {
          event.preventDefault();
          go(destination.id);
        }
      }}
    />
  );
}

import {
  useCallback,
  useEffect,
  useMemo,
  useRef,
  useState,
  type ReactElement,
} from "react";
import {
  parseAsArrayOf,
  parseAsBoolean,
  parseAsInteger,
  parseAsString,
  parseAsStringLiteral,
  useQueryStates,
} from "nuqs";
import { AnimatePresence, motion } from "motion/react";
import { ChevronDown, Search } from "lucide-react";
import {
  ActionButton,
  CardsIcon,
  Dialog,
  EmptyState,
  FeedbackMessages,
  FilterChip,
  SelectMenu,
  TextField,
} from "@spellwake/ui";
import {
  activeFilterCount,
  CARD_COLORS,
  COLOR_NAMES,
  copiesLabel,
  matchCards,
  NEUTRAL_QUERY,
  ownedSets,
  printingLabel,
  RARITIES,
  RARITY_NAMES,
  SORTS,
  type CardColor,
  type CardQuery,
  type CardsSource,
  type OwnedPrinting,
  type Rarity,
  type SortKey,
} from "../../collection/card-sources";
import { useArrival } from "../../lib/arrival";
import { useDeviceClass, useShellLayout } from "../../lib/display-scale";
import { motionLanguage as m } from "../../lib/motion";
import { flyToDetail } from "./card-flight";
import { CardFace, type ArtState } from "./CardFace";
import { CardFilters } from "./CardFilters";
import { FilterPanel } from "./FilterPanel";
import { CardInspector } from "./CardInspector";
import * as s from "./cards.css";

const ART_FAILED = "Couldn’t load card artwork.";
const GAP_COPY = {
  "signed-out": {
    title: "Sign in to see your cards",
    description: "Your cards are saved to your account.",
  },
  unregistered: {
    title: "Finish creating your account",
    description: "Choose a username to start collecting.",
  },
} as const;

/** Everything the gallery is narrowed and ordered by lives in the address, so returning restores it. */
const queryParsers = {
  q: parseAsString.withDefault(""),
  sort: parseAsStringLiteral(SORTS.map(({ key }) => key)).withDefault(
    "name-asc",
  ),
  format: parseAsString,
  set: parseAsArrayOf(parseAsString).withDefault([]),
  color: parseAsArrayOf(parseAsStringLiteral(CARD_COLORS)).withDefault([]),
  identity: parseAsBoolean.withDefault(false),
  colorless: parseAsBoolean.withDefault(false),
  type: parseAsArrayOf(parseAsString).withDefault([]),
  rarity: parseAsArrayOf(parseAsStringLiteral(RARITIES)).withDefault([]),
  mvmin: parseAsInteger,
  mvmax: parseAsInteger,
  card: parseAsString,
  printing: parseAsString,
};

/**
 * Collection Cards: every owned card once, with its total copies, narrowed by
 * search and filters and ordered by the chosen sort. The inspector shows the
 * chosen card and its owned printings. Nothing here changes what is owned.
 */
export function CardsPage({ source }: { source: CardsSource }): ReactElement {
  const [params, setParams] = useQueryStates(queryParsers);
  const isCompact = useShellLayout() === "compact";
  const isPhone = useDeviceClass() === "phone";
  const [isFilterOpen, setIsFilterOpen] = useState(false);
  const [isInspecting, setIsInspecting] = useState(false);
  const [dismissed, setDismissed] = useState<string | null>(null);
  const [artFailed, setArtFailed] = useState(false);
  const searchRegion = useRef<HTMLDivElement>(null);
  const filtersButton = useRef<HTMLButtonElement>(null);
  const scrollRegion = useRef<HTMLDivElement>(null);

  const ready = source.status === "ready" ? source : null;
  const facets = ready?.facets;
  // Filters a source cannot honour never narrow it, even from an old address.
  const query: CardQuery = {
    search: params.q,
    sort: params.sort,
    format: facets?.formats?.includes(params.format ?? "")
      ? params.format
      : null,
    sets: params.set,
    colorMode: params.identity ? "identity" : "color",
    colors: facets?.color ? params.color : [],
    colorless: facets?.color ? params.colorless : false,
    types: facets?.type ? params.type : [],
    rarities: facets?.rarity ? params.rarity : [],
    manaMin: facets?.manaValue ? params.mvmin : null,
    manaMax: facets?.manaValue ? params.mvmax : null,
  };
  const sorts = SORTS.filter(
    ({ key }) => facets?.manaValue || !key.startsWith("mana"),
  );
  // The query is rebuilt from the address each render, so it is compared by value.
  const queryKey = JSON.stringify(query);
  const matches = useMemo(
    () =>
      ready ? matchCards(ready.cards, JSON.parse(queryKey) as CardQuery) : [],
    [ready, queryKey],
  );
  const sets = useMemo(() => ownedSets(ready?.cards ?? []), [ready]);

  // The chosen card stays chosen while it matches; otherwise the first result is.
  const chosen =
    matches.find((match) => match.card.id === params.card) ??
    matches[0] ??
    null;
  const printing: OwnedPrinting | null =
    chosen?.card.printings.find((p) => p.id === params.printing) ??
    chosen?.printing ??
    null;

  const update = useCallback(
    (next: Partial<CardQuery>) => {
      void setParams({
        ...("search" in next ? { q: next.search || null } : {}),
        ...("sort" in next ? { sort: next.sort } : {}),
        ...("format" in next ? { format: next.format ?? null } : {}),
        ...("sets" in next
          ? { set: next.sets?.length ? [...next.sets] : null }
          : {}),
        ...("colorMode" in next
          ? { identity: next.colorMode === "identity" || null }
          : {}),
        ...("colors" in next
          ? { color: next.colors?.length ? [...next.colors] : null }
          : {}),
        ...("colorless" in next ? { colorless: next.colorless || null } : {}),
        ...("types" in next
          ? { type: next.types?.length ? [...next.types] : null }
          : {}),
        ...("rarities" in next
          ? { rarity: next.rarities?.length ? [...next.rarities] : null }
          : {}),
        ...("manaMin" in next ? { mvmin: next.manaMin ?? null } : {}),
        ...("manaMax" in next ? { mvmax: next.manaMax ?? null } : {}),
      });
    },
    [setParams],
  );
  const resetFilters = (): void =>
    update({
      format: null,
      sets: [],
      colors: [],
      colorless: false,
      types: [],
      rarities: [],
      manaMin: null,
      manaMax: null,
    });
  const clearAll = (): void => {
    update({ ...NEUTRAL_QUERY, sort: query.sort });
    searchRegion.current?.querySelector("input")?.focus();
  };

  // A newly chosen card scrolls into view without taking keyboard focus.
  const chosenId = chosen?.card.id ?? null;
  useEffect(() => {
    if (!chosenId) return;
    scrollRegion.current
      ?.querySelector<HTMLElement>(`[data-card="${CSS.escape(chosenId)}"]`)
      ?.scrollIntoView({ block: "nearest" });
  }, [chosenId, query.sort]);
  useEffect(() => setArtFailed(false), [printing?.id]);
  const onArtState = useCallback(
    (state: ArtState) => state === "failed" && setArtFailed(true),
    [],
  );

  const count = activeFilterCount(query);
  const chips: { key: string; label: string; remove: () => void }[] = [
    ...(query.format
      ? [
          {
            key: "format",
            label: query.format,
            remove: () => update({ format: null }),
          },
        ]
      : []),
    ...query.sets.map((key) => ({
      key: `set-${key}`,
      label: sets.find((set) => set.key === key)?.code ?? key,
      remove: () =>
        update({ sets: query.sets.filter((value) => value !== key) }),
    })),
    ...query.colors.map((color: CardColor) => ({
      key: `color-${color}`,
      label: COLOR_NAMES[color],
      remove: () =>
        update({ colors: query.colors.filter((value) => value !== color) }),
    })),
    ...(query.colorless
      ? [
          {
            key: "colorless",
            label: "Colorless",
            remove: () => update({ colorless: false }),
          },
        ]
      : []),
    ...query.types.map((type) => ({
      key: `type-${type}`,
      label: type,
      remove: () =>
        update({ types: query.types.filter((value) => value !== type) }),
    })),
    ...query.rarities.map((rarity: Rarity) => ({
      key: `rarity-${rarity}`,
      label: RARITY_NAMES[rarity],
      remove: () =>
        update({
          rarities: query.rarities.filter((value) => value !== rarity),
        }),
    })),
    ...(query.manaMin !== null || query.manaMax !== null
      ? [
          {
            key: "mana",
            label: `MV ${query.manaMin ?? "0"}–${query.manaMax ?? "any"}`,
            remove: () => update({ manaMin: null, manaMax: null }),
          },
        ]
      : []),
  ];

  // A dense grid takes a shorter beat between tiles than a list of rows.
  const arrival = useArrival(matches.length > 0, {
    stagger: m.stagger / 2,
    most: 14,
  });
  const failure = ready?.failure ?? null;
  const messages = [
    ...(failure && dismissed !== failure
      ? [{ id: "read", text: failure, onDismiss: () => setDismissed(failure) }]
      : []),
    ...(artFailed && dismissed !== ART_FAILED
      ? [
          {
            id: "art",
            text: ART_FAILED,
            onDismiss: () => setDismissed(ART_FAILED),
          },
        ]
      : []),
  ];

  const filters =
    ready && facets ? (
      <CardFilters
        query={query}
        facets={facets}
        sets={sets}
        onChange={update}
      />
    ) : null;

  return (
    <section className={s.page} aria-label="Cards">
      <div className={s.main}>
        <div className={s.toolbar}>
          <div className={s.search} ref={searchRegion}>
            <TextField
              type="search"
              label="Search cards"
              placeholder="Search cards…"
              value={query.search}
              onValueChange={(value) => update({ search: value })}
              hideLabel
              treatment="compact-form"
            />
          </div>
          <button
            ref={filtersButton}
            type="button"
            className={s.filtersButton}
            aria-expanded={isFilterOpen}
            aria-haspopup="dialog"
            disabled={!ready}
            onClick={() => setIsFilterOpen((open) => !open)}
          >
            {count > 0 ? `Filters (${count})` : "Filters"}
            <ChevronDown className={s.chevron} aria-hidden="true" />
          </button>
          <SelectMenu
            label="Sort"
            {...(isPhone ? {} : { valuePrefix: "Sort: " })}
            value={query.sort}
            options={sorts.map(({ key, label }) => ({ value: key, label }))}
            onValueChange={(value) => update({ sort: value as SortKey })}
            disabled={!ready}
          />
        </div>
        {chips.length > 0 && (
          <div className={s.chips} aria-label="Active filters" role="group">
            <AnimatePresence initial={false}>
              {chips.map((chip) => (
                <FilterChip
                  key={chip.key}
                  label={chip.label}
                  onRemove={chip.remove}
                />
              ))}
            </AnimatePresence>
            <button
              type="button"
              className={s.clearFilters}
              onClick={resetFilters}
            >
              Clear filters
            </button>
          </div>
        )}

        {matches.length > 0 ? (
          <div
            ref={scrollRegion}
            className={s.scroll}
            role="region"
            aria-label="Card gallery"
            tabIndex={-1}
          >
            <ul className={s.grid}>
              {matches.map(({ card, printing: shown }, index) => (
                <motion.li key={card.id} {...arrival.item(index)}>
                  <button
                    type="button"
                    className={s.tile}
                    data-card={card.id}
                    // Without a side inspector a tile opens its card instead of staying chosen.
                    {...(isCompact
                      ? {}
                      : { "aria-pressed": card.id === chosen?.card.id })}
                    aria-label={`${card.name}, ${copiesLabel(card.copies)}, ${printingLabel(shown)}`}
                    onClick={(event) => {
                      const choose = () => {
                        void setParams({ card: card.id, printing: null });
                        if (isCompact) setIsInspecting(true);
                      };
                      // Beside the gallery, the chosen card travels into its details.
                      if (isCompact || card.id === chosen?.card.id) choose();
                      else flyToDetail(event.currentTarget, choose);
                    }}
                  >
                    <span className={s.tileFace}>
                      <CardFace url={shown.faceUrl} name={card.name} />
                      <span className={s.count} aria-hidden="true">
                        ×{card.copies}
                      </span>
                    </span>
                    <span className={s.caption} aria-hidden="true">
                      <span className={s.name}>{card.name}</span>
                      <span className={s.printing}>{printingLabel(shown)}</span>
                    </span>
                  </button>
                </motion.li>
              ))}
            </ul>
            {ready?.isAppending && (
              <p className={s.appending} role="status">
                <span className={s.smallSpinner} aria-hidden="true" />
                Loading more cards…
              </p>
            )}
          </div>
        ) : (
          <div className={s.center}>
            {source.status === "loading" ||
            (ready?.isAppending && ready.cards.length === 0) ? (
              <p className={s.loading} role="status">
                <span className={s.spinner} aria-hidden="true" />
                Loading cards…
              </p>
            ) : source.status === "signed-out" ||
              source.status === "unregistered" ? (
              <EmptyState {...GAP_COPY[source.status]} />
            ) : source.status === "unavailable" ||
              ready?.isUnread ? null : ready && ready.cards.length > 0 ? (
              <div className={s.empty}>
                <Search className={s.emptyIcon} aria-hidden="true" />
                <EmptyState
                  title="No matching cards"
                  description="Try another search or filter."
                  action={
                    <ActionButton treatment="compact-form" onClick={clearAll}>
                      Clear search and filters
                    </ActionButton>
                  }
                />
              </div>
            ) : (
              <div className={s.empty}>
                <CardsIcon className={s.emptyIcon} aria-hidden="true" />
                <EmptyState
                  title="No cards yet"
                  description="Cards you own will appear here."
                />
              </div>
            )}
          </div>
        )}
      </div>

      <aside
        className={s.inspector}
        aria-label="Card details"
        // The filter panel covers the inspector, which waits hidden underneath.
        inert={(isFilterOpen && !isCompact) || undefined}
        data-covered={(isFilterOpen && !isCompact) || undefined}
      >
        {chosen && printing && !isCompact && (
          <CardInspector
            card={chosen.card}
            printing={printing}
            onChoosePrinting={(next) =>
              void setParams({ card: chosen.card.id, printing: next.id })
            }
            onArtState={onArtState}
          />
        )}
      </aside>

      {isFilterOpen && filters && !isCompact && (
        <FilterPanel
          onClose={() => {
            setIsFilterOpen(false);
            filtersButton.current?.focus();
          }}
          onReset={resetFilters}
        >
          {filters}
        </FilterPanel>
      )}
      {isCompact && (
        <Dialog
          open={isFilterOpen && !!filters}
          title="Filters"
          onClose={() => setIsFilterOpen(false)}
          actions={
            <ActionButton treatment="compact-form" onClick={resetFilters}>
              Reset filters
            </ActionButton>
          }
        >
          {filters}
        </Dialog>
      )}
      {isCompact && chosen && printing && (
        <Dialog
          open={isInspecting}
          title={chosen.card.name}
          subtitle={copiesLabel(chosen.card.copies)}
          onClose={() => setIsInspecting(false)}
        >
          <CardInspector
            card={chosen.card}
            printing={printing}
            inDialog
            onChoosePrinting={(next) =>
              void setParams({ card: chosen.card.id, printing: next.id })
            }
            onArtState={onArtState}
          />
        </Dialog>
      )}

      <FeedbackMessages
        label="Cards messages"
        presentation="toast"
        items={messages}
      />
    </section>
  );
}

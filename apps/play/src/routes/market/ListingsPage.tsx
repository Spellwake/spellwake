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
import { AnimatePresence } from "motion/react";
import { ChevronDown, Plus } from "lucide-react";
import {
  ActionButton,
  Checkbox,
  CoinAmount,
  Dialog,
  EmptyState,
  FeedbackMessages,
  FilterChip,
  ListingItemSummary,
  ListingRow,
  SelectMenu,
  TextField,
} from "@spellwake/ui";
import {
  CARD_COLORS,
  COLOR_NAMES,
  RARITIES,
  RARITY_NAMES,
} from "../../collection/card-sources";
import {
  FINISH_LABELS,
  KIND_LABELS,
  listingFilterCount,
  listingName,
  listingSets,
  matchListings,
  nextSelection,
  purchasedMessage,
  SORT_LABELS,
  type Listing,
  type ListingKind,
  type ListingQuery,
  type ListingSort,
  type ListingsSource,
} from "../../market/listing-sources";
import { useDeviceClass, useShellLayout } from "../../lib/display-scale";
import { FilterPanel } from "../cards/FilterPanel";
import * as cards from "../cards/cards.css";
import { CreateListingDialog } from "./CreateListingDialog";
import { BuyDialog, CancelDialog, COIN, summaryItem } from "./ListingDialogs";
import { ListingFilters } from "./ListingFilters";
import * as s from "./market.css";

const KINDS = Object.keys(KIND_LABELS) as ListingKind[];
const FINISHES = ["any", "nonfoil", "foil"] as const;
const SORTS = Object.keys(SORT_LABELS) as ListingSort[];
const GAP_COPY = {
  "signed-out": {
    title: "Sign in to use the market",
    description: "Listings belong to your account.",
  },
  unregistered: {
    title: "Finish creating your account",
    description: "Choose a username to start trading.",
  },
} as const;

/** Everything Listings is narrowed and ordered by lives in the address, so returning restores it. */
const queryParsers = {
  q: parseAsString.withDefault(""),
  kind: parseAsStringLiteral(KINDS).withDefault("all"),
  mine: parseAsBoolean.withDefault(false),
  sort: parseAsStringLiteral(SORTS),
  format: parseAsString,
  set: parseAsArrayOf(parseAsString).withDefault([]),
  color: parseAsArrayOf(parseAsStringLiteral(CARD_COLORS)).withDefault([]),
  identity: parseAsBoolean.withDefault(false),
  colorless: parseAsBoolean.withDefault(false),
  type: parseAsArrayOf(parseAsString).withDefault([]),
  rarity: parseAsArrayOf(parseAsStringLiteral(RARITIES)).withDefault([]),
  mvmin: parseAsInteger,
  mvmax: parseAsInteger,
  finish: parseAsStringLiteral(FINISHES).withDefault("any"),
  pmin: parseAsInteger,
  pmax: parseAsInteger,
  listing: parseAsString,
};

/**
 * Market Listings, as approved (market-listings-approval-v1): whole lots in
 * rows beside a persistent inspector. Buying asks first; your own active
 * listings can only be cancelled, and asking first again. Create listing
 * chooses one owned printing or pack, then its quantity and total. Every
 * command keeps its dialog open until its outcome is known.
 */
export function ListingsPage({
  source,
}: {
  source: ListingsSource;
}): ReactElement {
  const [params, setParams] = useQueryStates(queryParsers);
  const isCompact = useShellLayout() === "compact";
  const isPhone = useDeviceClass() === "phone";
  const [isFilterOpen, setIsFilterOpen] = useState(false);
  const [isInspecting, setIsInspecting] = useState(false);
  const [buying, setBuying] = useState<Listing | null>(null);
  const [cancelling, setCancelling] = useState<Listing | null>(null);
  const [isCreating, setIsCreating] = useState(false);
  const filtersButton = useRef<HTMLButtonElement>(null);
  const actionButton = useRef<HTMLButtonElement>(null);
  /** Where a closing Buy or Cancel dialog returns focus: the action, or the list heading once nothing is left. */
  const returnTarget = useRef<HTMLElement | null>(null);
  const createButton = useRef<HTMLButtonElement>(null);
  const listHeading = useRef<HTMLHeadingElement>(null);
  const scrollRegion = useRef<HTMLDivElement>(null);

  const ready = source.status === "ready" ? source : null;
  const facets = ready?.facets;
  const sorts = facets?.sorts ?? SORTS;
  const query: ListingQuery = {
    search: params.q,
    kind: params.kind,
    mine: params.mine,
    sort:
      params.sort && sorts.includes(params.sort)
        ? params.sort
        : (sorts[0] ?? "name-asc"),
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
    finish: params.finish,
    priceMin: params.pmin,
    priceMax: params.pmax,
  };
  const queryKey = JSON.stringify(query);
  const shown = useMemo(
    () =>
      ready
        ? matchListings(ready.listings, JSON.parse(queryKey) as ListingQuery)
        : [],
    [ready, queryKey],
  );
  const sets = useMemo(
    () => listingSets(ready?.listings ?? [], query.kind),
    [ready, query.kind],
  );
  const selected =
    shown.find((listing) => listing.id === params.listing) ?? shown[0] ?? null;

  const update = useCallback(
    (next: Partial<ListingQuery>) => {
      void setParams({
        ...("search" in next ? { q: next.search || null } : {}),
        ...("kind" in next ? { kind: next.kind ?? null } : {}),
        ...("mine" in next ? { mine: next.mine || null } : {}),
        ...("sort" in next ? { sort: next.sort ?? null } : {}),
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
        ...("finish" in next
          ? { finish: next.finish === "any" ? null : (next.finish ?? null) }
          : {}),
        ...("priceMin" in next ? { pmin: next.priceMin ?? null } : {}),
        ...("priceMax" in next ? { pmax: next.priceMax ?? null } : {}),
      });
    },
    [setParams],
  );
  /** Reset filters clears every facet that applies, keeping the search and the kind. */
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
      finish: "any",
      priceMin: null,
      priceMax: null,
    });
  const select = (id: string | null) =>
    void setParams({ listing: id }, { history: "replace" });

  // The rows last shown, so a removed listing hands selection to its neighbour.
  const lastShown = useRef<readonly Listing[]>([]);
  useEffect(() => {
    if (shown.length > 0) lastShown.current = shown;
  }, [shown]);

  // A completed command: its toast, and where selection goes.
  const completed = ready?.completed ?? null;
  const handled = useRef<typeof completed>(null);
  useEffect(() => {
    if (!completed || handled.current === completed) return;
    handled.current = completed;
    if (completed.kind === "bought") {
      setBuying(null);
      const next = nextSelection(lastShown.current, completed.listing.id);
      select(next);
      if (!next) returnTarget.current = listHeading.current;
    } else if (completed.kind === "cancelled") {
      setCancelling(null);
      const next = nextSelection(lastShown.current, completed.listingId);
      select(next);
      if (!next) returnTarget.current = listHeading.current;
    } else {
      // A new listing shows among your own, with nothing hiding it.
      setIsCreating(false);
      void setParams({
        ...Object.fromEntries(
          Object.keys(queryParsers).map((key) => [key, null]),
        ),
        mine: true,
        sort: params.sort,
        listing: completed.listingId,
      });
    }
    // Selection follows each outcome once; later renders leave it alone.
  }, [completed, params.sort, setParams]);

  // The chosen row scrolls into view without taking keyboard focus.
  const selectedId = selected?.id ?? null;
  useEffect(() => {
    if (!selectedId) return;
    scrollRegion.current
      ?.querySelector<HTMLElement>(`[data-listing="${CSS.escape(selectedId)}"]`)
      ?.scrollIntoView({ block: "nearest" });
  }, [selectedId]);

  const operation = ready?.operation ?? null;
  const count = listingFilterCount(query);
  const chips: { key: string; label: string; remove: () => void }[] = [
    ...query.sets.map((key) => ({
      key: `set-${key}`,
      label: sets.find((set) => set.key === key)?.code ?? key,
      remove: () =>
        update({ sets: query.sets.filter((value) => value !== key) }),
    })),
    ...(query.priceMin !== null || query.priceMax !== null
      ? [
          {
            key: "price",
            label: `${query.priceMin ?? 0}–${query.priceMax ?? "any"} coins`,
            remove: () => update({ priceMin: null, priceMax: null }),
          },
        ]
      : []),
    ...(query.kind === "cards"
      ? [
          ...(query.format
            ? [
                {
                  key: "format",
                  label: query.format,
                  remove: () => update({ format: null }),
                },
              ]
            : []),
          ...query.colors.map((color) => ({
            key: `color-${color}`,
            label: COLOR_NAMES[color],
            remove: () =>
              update({ colors: query.colors.filter((c) => c !== color) }),
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
              update({ types: query.types.filter((t) => t !== type) }),
          })),
          ...query.rarities.map((rarity) => ({
            key: `rarity-${rarity}`,
            label: RARITY_NAMES[rarity],
            remove: () =>
              update({ rarities: query.rarities.filter((r) => r !== rarity) }),
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
          ...(query.finish !== "any"
            ? [
                {
                  key: "finish",
                  label: FINISH_LABELS[query.finish],
                  remove: () => update({ finish: "any" }),
                },
              ]
            : []),
        ]
      : []),
  ];

  const messages = [
    ...(ready?.failure
      ? [{ id: "read", text: ready.failure, onDismiss: ready.dismissFailure }]
      : []),
    ...(completed && ready
      ? [
          {
            id: "done",
            tone: "success" as const,
            text:
              completed.kind === "bought"
                ? purchasedMessage(completed.listing)
                : completed.kind === "created"
                  ? "Listing created."
                  : "Listing cancelled.",
            onDismiss: ready.dismissCompleted,
          },
        ]
      : []),
  ];

  const filters =
    ready && facets ? (
      <ListingFilters
        query={query}
        facets={facets}
        sets={sets}
        onChange={update}
      />
    ) : null;

  const inspector = selected ? (
    <div className={s.inspectorBody}>
      <ListingItemSummary
        item={summaryItem(selected.item, selected.quantity)}
        seller={selected.seller.name}
        layout="inspector"
      />
      <div className={s.inspectorPrice}>
        <CoinAmount amount={selected.total} iconSrc={COIN} />
      </div>
      {selected.isMine ? (
        <ActionButton
          ref={actionButton}
          className={s.inspectorAction}
          data-tone="danger"
          aria-haspopup="dialog"
          disabled={!ready?.canTrade}
          onClick={() => {
            returnTarget.current = actionButton.current;
            setCancelling(selected);
          }}
        >
          Cancel listing
        </ActionButton>
      ) : (
        <ActionButton
          ref={actionButton}
          className={s.inspectorAction}
          aria-haspopup="dialog"
          onClick={() => {
            returnTarget.current = actionButton.current;
            setBuying(selected);
          }}
        >
          Buy
        </ActionButton>
      )}
    </div>
  ) : null;

  const isEmptyMarket = ready !== null && ready.listings.length === 0;
  return (
    <section className={cards.page} aria-label="Listings">
      <div className={cards.main}>
        <div className={s.toolbar}>
          <div className={s.search}>
            <TextField
              type="search"
              label="Search listings"
              placeholder="Search listings…"
              value={query.search}
              onValueChange={(value) => update({ search: value })}
              hideLabel
              treatment="compact-form"
            />
          </div>
          <SelectMenu
            label="Item type"
            value={query.kind}
            options={KINDS.map((kind) => ({
              value: kind,
              label: KIND_LABELS[kind],
            }))}
            onValueChange={(value) => update({ kind: value as ListingKind })}
            disabled={!ready}
          />
          <button
            ref={filtersButton}
            type="button"
            className={cards.filtersButton}
            aria-expanded={isFilterOpen}
            aria-haspopup="dialog"
            disabled={!ready}
            onClick={() => setIsFilterOpen((open) => !open)}
          >
            {count > 0 ? `Filters (${count})` : "Filters"}
            <ChevronDown className={cards.chevron} aria-hidden="true" />
          </button>
          <ActionButton
            ref={createButton}
            className={s.create}
            treatment="compact-form"
            icon={<Plus aria-hidden="true" />}
            aria-haspopup="dialog"
            disabled={!ready?.canTrade}
            onClick={() => setIsCreating(true)}
          >
            Create listing
          </ActionButton>
        </div>
        <div className={s.secondRow}>
          <Checkbox
            treatment="outline"
            label="Only my listings"
            checked={query.mine}
            onCheckedChange={(checked) => update({ mine: checked })}
          />
          <SelectMenu
            label="Sort listings"
            // A phone keeps the sort beside Only my listings, so it drops "Sort: ".
            {...(isPhone ? {} : { valuePrefix: "Sort: " })}
            value={query.sort}
            options={sorts.map((sort) => ({
              value: sort,
              label: SORT_LABELS[sort],
            }))}
            onValueChange={(value) => update({ sort: value as ListingSort })}
            disabled={!ready}
          />
        </div>
        {chips.length > 0 && (
          <div className={cards.chips} aria-label="Active filters" role="group">
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
              className={cards.clearFilters}
              onClick={resetFilters}
            >
              Clear filters
            </button>
          </div>
        )}
        <h2 ref={listHeading} className={s.srOnly} tabIndex={-1}>
          Listings
        </h2>
        {shown.length > 0 ? (
          <div
            ref={scrollRegion}
            className={s.rows}
            role="region"
            aria-label="Listing rows"
          >
            {shown.map((listing) => (
              <div key={listing.id} data-listing={listing.id}>
                <ListingRow
                  item={summaryItem(listing.item, listing.quantity)}
                  seller={listing.seller.name}
                  price={{ amount: listing.total, iconSrc: COIN }}
                  selected={!isCompact && listing.id === selected?.id}
                  onSelect={() => {
                    select(listing.id);
                    if (isCompact) setIsInspecting(true);
                  }}
                />
              </div>
            ))}
            {ready?.isAppending && (
              <p className={cards.appending} role="status">
                <span className={cards.smallSpinner} aria-hidden="true" />
                Loading more listings…
              </p>
            )}
          </div>
        ) : (
          <div className={`${cards.center} ${s.quiet}`}>
            {source.status === "loading" ||
            (ready?.isAppending && ready.listings.length === 0) ? (
              <p className={cards.loading} role="status">
                <span className={cards.spinner} aria-hidden="true" />
                Loading listings…
              </p>
            ) : source.status === "signed-out" ||
              source.status === "unregistered" ? (
              <EmptyState {...GAP_COPY[source.status]} />
            ) : source.status === "unavailable" ||
              ready?.isUnread ? null : query.mine &&
              !ready?.listings.some((listing) => listing.isMine) ? (
              <EmptyState title="No active listings." />
            ) : isEmptyMarket ? (
              <EmptyState title="No listings yet." />
            ) : (
              <EmptyState
                title="No matching listings."
                // Clear filters keeps the search, so it is offered only when filters narrow the list.
                {...(count > 0
                  ? {
                      action: (
                        <ActionButton
                          treatment="compact-form"
                          onClick={resetFilters}
                        >
                          Clear filters
                        </ActionButton>
                      ),
                    }
                  : {})}
              />
            )}
          </div>
        )}
      </div>

      <aside
        className={cards.inspector}
        aria-label="Selected listing"
        inert={(isFilterOpen && !isCompact) || undefined}
        data-covered={(isFilterOpen && !isCompact) || undefined}
      >
        {!isCompact && inspector}
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
      {isCompact && selected && (
        <Dialog
          open={isInspecting && !buying && !cancelling}
          title={listingName(selected.item)}
          onClose={() => setIsInspecting(false)}
        >
          {inspector}
        </Dialog>
      )}

      {ready && (
        <>
          <BuyDialog
            open={buying !== null || operation?.kind === "buy"}
            listing={buying}
            operation={operation}
            balance={ready.balance}
            canTrade={ready.canTrade}
            onBuy={() => {
              const target =
                operation?.kind === "buy" ? operation.listing : buying;
              if (target) ready.buy(target.id);
            }}
            onCheck={ready.checkStatus}
            onClose={() => {
              if (operation?.kind === "buy") ready.dismissOperation();
              setBuying(null);
            }}
            returnFocusRef={returnTarget}
          />
          <CancelDialog
            open={cancelling !== null || operation?.kind === "cancel"}
            listing={cancelling}
            operation={operation}
            onCancelListing={() => {
              const target =
                operation?.kind === "cancel" ? operation.listing : cancelling;
              if (target) ready.cancel(target.id);
            }}
            onCheck={ready.checkStatus}
            onClose={() => {
              if (operation?.kind === "cancel") ready.dismissOperation();
              setCancelling(null);
            }}
            returnFocusRef={returnTarget}
          />
          <CreateListingDialog
            open={isCreating || operation?.kind === "create"}
            assets={ready.assets}
            operation={operation}
            canTrade={ready.canTrade}
            onCreate={ready.create}
            onCheck={ready.checkStatus}
            onDismissRejection={ready.dismissOperation}
            onClose={() => setIsCreating(false)}
            returnFocusRef={createButton}
          />
        </>
      )}

      <FeedbackMessages
        label="Listings messages"
        presentation="toast"
        items={messages}
      />
    </section>
  );
}

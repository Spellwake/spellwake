import {
  useEffect,
  useId,
  useLayoutEffect,
  useRef,
  useState,
  type ReactNode,
  type RefObject,
} from "react";
import {
  ActionButton,
  Checkbox,
  CoinAmount,
  ConfirmationDialog,
  ListingItemSummary,
  ListingRow,
  SelectMenu,
  TextField,
  type ListingItem,
} from "@spellwake/ui";
import "./market-listings.css";

export interface MarketListing {
  id: string;
  item: ListingItem;
  seller: string;
  total: number;
}
export const listingTypes = ["All items", "Cards", "Packs"] as const;
export const listingSorts = [
  "Newest",
  "Price: Low to high",
  "Price: High to low",
  "Name: A–Z",
] as const;
const options = (labels: readonly string[]) =>
  labels.map((value) => ({ value, label: value }));

interface ListingsPageProps {
  listings: readonly MarketListing[];
  selectedId?: string | undefined;
  coinIcon: string;
  search: string;
  type: string;
  sort: string;
  mine: boolean;
  onSearch: (value: string) => void;
  onType: (value: string) => void;
  onSort: (value: string) => void;
  onMine: (value: boolean) => void;
  onSelect: (id: string) => void;
  renderFilters: (close: () => void) => ReactNode;
  onCreate: () => void;
  inspectButton: RefObject<HTMLButtonElement | null>;
  onInspectAction: (listing: MarketListing) => void;
}

/** Desktop Listings content. The application owns the listing query, identity and economic actions. */
export function MarketListingsPage(props: ListingsPageProps) {
  const selected =
    props.listings.find((row) => row.id === props.selectedId) ??
    props.listings[0];
  const [open, setOpen] = useState(false);
  const filterAnchor = useRef<HTMLDivElement>(null),
    trigger = useRef<HTMLButtonElement>(null);
  const panelId = useId();
  function dismiss() {
    setOpen(false);
    trigger.current?.focus({ preventScroll: true });
  }
  useLayoutEffect(() => {
    if (open)
      filterAnchor.current
        ?.querySelector<HTMLElement>('[role="dialog"]')
        ?.focus();
  }, [open]);
  useEffect(() => {
    if (!open) return;
    const outside = (e: PointerEvent | FocusEvent) => {
      if (
        e.target instanceof Element &&
        !filterAnchor.current?.contains(e.target) &&
        !e.target.closest(
          '[role="listbox"],[data-radix-popper-content-wrapper]',
        )
      )
        setOpen(false);
    };
    document.addEventListener("pointerdown", outside);
    document.addEventListener("focusin", outside);
    return () => {
      document.removeEventListener("pointerdown", outside);
      document.removeEventListener("focusin", outside);
    };
  }, [open]);
  return (
    <div className="market-listings">
      <section className="market-browser" aria-label="Market listings">
        <div className="market-toolbar">
          <TextField
            type="search"
            label="Search listings"
            hideLabel
            value={props.search}
            placeholder="Search listings…"
            onValueChange={props.onSearch}
          />
          <SelectMenu
            label="Item type"
            value={props.type}
            options={options(listingTypes)}
            onValueChange={props.onType}
          />
          <div
            ref={filterAnchor}
            className="market-filter-anchor"
            onKeyDown={(e) => {
              if (e.key === "Escape" && open && !e.defaultPrevented) {
                e.preventDefault();
                dismiss();
              }
            }}
          >
            <ActionButton
              ref={trigger}
              aria-haspopup="dialog"
              aria-expanded={open}
              aria-controls={panelId}
              onClick={() => setOpen(!open)}
            >
              Filters <span className="market-chevron" aria-hidden="true" />
            </ActionButton>
            {open && (
              <div
                id={panelId}
                role="dialog"
                aria-label="Listing filters"
                tabIndex={-1}
                className="market-filter-panel"
              >
                {props.renderFilters(dismiss)}
              </div>
            )}
          </div>
          <ActionButton emphasis="primary" onClick={props.onCreate}>
            <span aria-hidden="true">＋</span> Create listing
          </ActionButton>
        </div>
        <div className="market-sort">
          <Checkbox
            treatment="outline"
            label="Only my listings"
            checked={props.mine}
            onCheckedChange={props.onMine}
          />
          <SelectMenu
            label="Sort listings"
            valuePrefix="Sort: "
            value={props.sort}
            options={options(listingSorts)}
            onValueChange={props.onSort}
          />
        </div>
        <div className="market-rows">
          {props.listings.map((row) => (
            <ListingRow
              key={row.id}
              item={row.item}
              seller={row.seller}
              selected={row === selected}
              price={{ amount: row.total, iconSrc: props.coinIcon }}
              onSelect={() => props.onSelect(row.id)}
            />
          ))}
          {!selected && <p role="status">No matching listings</p>}
        </div>
      </section>
      {selected && (
        <aside className="market-inspector" aria-label="Selected listing">
          <ListingItemSummary
            item={selected.item}
            seller={selected.seller}
            layout="inspector"
          />
          <div className="market-price">
            <CoinAmount amount={selected.total} iconSrc={props.coinIcon} />
          </div>
          <ActionButton
            emphasis="primary"
            ref={props.inspectButton}
            onClick={() => props.onInspectAction(selected)}
          >
            {props.mine ? "Cancel listing" : "Buy"}
          </ActionButton>
        </aside>
      )}
    </div>
  );
}

export function BuyListingDialog({
  listing,
  coinIcon,
  open,
  onCancel,
  onConfirm,
  returnFocusRef,
  pending = false,
}: {
  listing: MarketListing;
  coinIcon: string;
  open: boolean;
  onCancel: () => void;
  onConfirm: () => void;
  returnFocusRef: RefObject<HTMLElement | null>;
  pending?: boolean;
}) {
  return (
    <ConfirmationDialog
      returnFocusRef={returnFocusRef}
      open={open}
      appearance="market"
      title="Buy listing"
      cancelLabel="Cancel"
      confirmLabel="Buy"
      pending={pending}
      onCancel={onCancel}
      onConfirm={onConfirm}
      dismissOnBackdrop
      divider
    >
      <div className="market-buy-item">
        <ListingItemSummary
          item={listing.item}
          seller={listing.seller}
          layout="detail"
        />
      </div>
      <div className="market-buy-total">
        Total <CoinAmount amount={listing.total} iconSrc={coinIcon} />
      </div>
    </ConfirmationDialog>
  );
}

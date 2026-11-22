import { Artboard, Box } from "./Artboard";
import { useLayoutEffect, useRef, useState } from "react";
import {
  BuyListingDialog,
  MarketListingsPage,
  type MarketListing,
} from "../../../../play/src/market/MarketListingsPage";
import {
  ListingRow,
  Checkbox,
  SelectMenu,
  ListingItemSummary,
  OwnedItemPicker,
  QuantityControl,
  type ListingItem,
} from "@spellwake/ui";
import {
  art,
  coin,
  Board,
  Btn,
  Coins,
  Notice,
  Search,
  Shell,
  Skeleton,
  Window,
  type Go,
} from "./shared";
const entries = [
  ["bolt", "Lightning Bolt", "M11", "Nonfoil", "Mira", 4, 500],
  ["counter", "Counterspell", "MH2", "Nonfoil", "Ash", 2, 350],
  ["opt", "Opt", "M21", "Foil", "Rowan", 4, 220],
  ["ring", "Sol Ring", "CMM", "Nonfoil", "Juniper", 1, 700],
  ["foundations", "Foundations Play Booster", "", "", "Finch", 2, 6000],
  ["bloomburrow", "Bloomburrow Play Booster", "", "", "Mira", 1, 3600],
] as const;
type Offer = readonly [
  id: string,
  name: string,
  set: string,
  finish: string,
  seller: string,
  quantity: number,
  total: number,
];
function ownOffer(offer: Offer): Offer {
  const stored = sessionStorage.getItem("sw-own-lot-" + offer[0]);
  if (stored) {
    try {
      const saved = JSON.parse(stored);
      if (
        Number.isSafeInteger(saved?.quantity) &&
        saved.quantity > 0 &&
        Number.isSafeInteger(saved.total) &&
        saved.total > 0
      )
        return [
          offer[0],
          offer[1],
          offer[2],
          offer[3],
          "You",
          saved.quantity,
          saved.total,
        ];
    } catch {
      /* A stale local example can use its catalogue terms. */
    }
  }
  return offer;
}
function listingArt(id: string) {
  const src = art[id] ?? art[id + "-thumb"];
  return src ? <img src={src} alt="" /> : null;
}
function item(o: Offer): ListingItem {
  const base = {
    name: o[1],
    quantity: o[5],
    artwork: listingArt(o[0]),
  };
  return o[2]
    ? { ...base, kind: "card", setCode: o[2], finish: o[3] }
    : { ...base, kind: "pack" };
}
function listing(offer: Offer, mine = false): MarketListing {
  return {
    id: offer[0],
    item: item(offer),
    seller: mine ? "You" : offer[4],
    total: offer[6],
  };
}
// Completed fixture lots are stored together so the balance and available listings agree on revisit.
function purchasedListings(): Offer[] {
  const ids = (sessionStorage.getItem("sw-market-purchased-ids") ?? "").split(
    ",",
  );
  return entries.filter((offer) => ids.includes(offer[0]));
}
function remainingBalance(purchases: Offer[]) {
  return purchases.reduce((balance, offer) => balance - offer[6], 8000);
}
export function Stepper({
  qty,
  set,
  max,
  disabled = false,
}: {
  qty: string;
  set: (s: string) => void;
  max: number;
  disabled?: boolean;
}) {
  const n = Number(qty);
  return (
    <QuantityControl
      label="Quantity"
      value={n}
      onDecrease={() => set(String(n - 1))}
      onIncrease={() => set(String(n + 1))}
      canDecrease={n > 1}
      canIncrease={n < max}
      disabled={disabled}
      treatment="joined"
      edit={{ draft: qty, onChange: set }}
    />
  );
}
type Facets = {
  sets: string[];
  colors: string[];
  rarities: string[];
  kind: string;
  format: string;
  finish: string;
  minMana: string;
  maxMana: string;
  min: string;
  max: string;
};
const allFacets: Facets = {
  sets: [],
  colors: [],
  rarities: [],
  kind: "Any",
  format: "All formats",
  finish: "Any",
  minMana: "",
  maxMana: "",
  min: "",
  max: "",
};
const attributes: Record<string, [string, string, string, number, string[]]> = {
  bolt: ["Red", "Instant", "Common", 1, ["Modern", "Legacy"]],
  counter: ["Blue", "Instant", "Uncommon", 2, ["Modern", "Legacy"]],
  opt: ["Blue", "Instant", "Common", 1, ["Modern", "Standard", "Legacy"]],
  ring: ["Colorless", "Artifact", "Uncommon", 1, ["Commander", "Legacy"]],
};
function matchesFacets(o: Offer, f: Facets) {
  if (
    (f.min !== "" && o[6] < Number(f.min)) ||
    (f.max !== "" && o[6] > Number(f.max))
  )
    return false;
  if (
    f.sets.length &&
    !f.sets.includes(o[2] || o[1].replace(" Play Booster", ""))
  )
    return false;
  if (!o[2])
    return (
      !f.colors.length &&
      !f.rarities.length &&
      f.kind === "Any" &&
      f.finish === "Any" &&
      f.format === "All formats" &&
      !f.minMana &&
      !f.maxMana
    );
  const [color, kind, rarity, mana, formats] = attributes[o[0]]!;
  return (
    (!f.colors.length || f.colors.includes(color)) &&
    (!f.rarities.length || f.rarities.includes(rarity)) &&
    (f.kind === "Any" || f.kind === kind) &&
    (f.finish === "Any" || f.finish === o[3]) &&
    (f.format === "All formats" || formats.includes(f.format)) &&
    (!f.minMana || mana >= Number(f.minMana)) &&
    (!f.maxMana || mana <= Number(f.maxMana))
  );
}
function Filter({
  packs = false,
  initial = allFacets,
  change,
  close,
}: {
  packs?: boolean;
  initial?: Facets;
  change?: (v: Facets) => void;
  close?: () => void;
}) {
  const [q, set] = useState(""),
    [f, setF] = useState(initial);
  function edit(patch: Partial<Facets>) {
    const next = { ...f, ...patch };
    setF(next);
    if (
      (!next.min || !next.max || Number(next.min) <= Number(next.max)) &&
      (!next.minMana ||
        !next.maxMana ||
        Number(next.minMana) <= Number(next.maxMana))
    )
      change?.(next);
  }
  function checks(key: "sets" | "colors" | "rarities", values: string[]) {
    return values.map((v) => (
      <Checkbox
        key={v}
        label={v}
        checked={f[key].includes(v)}
        onCheckedChange={(checked) =>
          edit({
            [key]: checked ? [...f[key], v] : f[key].filter((n) => n !== v),
          })
        }
      />
    ));
  }
  function menu(
    key: "format" | "kind" | "finish",
    label: string,
    values: string[],
  ) {
    return (
      <label>
        {label}
        <SelectMenu
          label={label}
          value={f[key]}
          options={values.map((value) => ({ value, label: value }))}
          onValueChange={(value) => edit({ [key]: value })}
        />
      </label>
    );
  }
  function range(isMana: boolean) {
    const a = isMana ? "minMana" : "min",
      b = isMana ? "maxMana" : "max";
    return (
      <div className="soc-tools">
        {([a, b] as const).map((k, i) => (
          <input
            key={k}
            aria-label={
              (i ? "Maximum " : "Minimum ") + (isMana ? "mana" : "total price")
            }
            type="number"
            min={0}
            value={f[k]}
            placeholder={i ? "Max" : "Min"}
            onChange={(e) => edit({ [k]: e.target.value })}
          />
        ))}
      </div>
    );
  }
  return (
    <form
      className="soc-filter"
      onSubmit={(e) => e.preventDefault()}
      onKeyDown={(e) => {
        if (e.key === "Escape") close?.();
      }}
    >
      <div className="soc-line">
        <h2>Filters</h2>
        {close && (
          <button type="button" aria-label="Close filters" onClick={close}>
            ×
          </button>
        )}
      </div>
      {!packs &&
        menu("format", "Format", [
          "All formats",
          "Standard",
          "Modern",
          "Commander",
          "Legacy",
        ])}
      <p>Set</p>
      <Search label="Find sets" value={q} set={set} />
      {checks(
        "sets",
        (packs
          ? ["Foundations", "Bloomburrow"]
          : ["M11", "MH2", "M21", "CMM"]
        ).filter((v) => v.toLowerCase().includes(q.toLowerCase())),
      )}
      {!packs && (
        <>
          <p>Color</p>
          <div className="soc-tools">
            {checks("colors", [
              "White",
              "Blue",
              "Black",
              "Red",
              "Green",
              "Colorless",
            ])}
          </div>
          {menu("kind", "Card type", ["Any", "Instant", "Artifact"])}
          <p>Rarity</p>
          <div className="soc-tools">
            {checks("rarities", ["Common", "Uncommon", "Rare", "Mythic"])}
          </div>
          <p>Mana value</p>
          {range(true)}
          {menu("finish", "Finish", ["Any", "Nonfoil", "Foil"])}
        </>
      )}
      <p>Total price</p>
      {range(false)}
      <Btn
        onClick={() => {
          set("");
          edit(allFacets);
        }}
      >
        Reset filters
      </Btn>
    </form>
  );
}
function Terms({
  mode,
  offer,
  go,
  variant = "",
  mini = false,
}: {
  mode: "buy" | "create" | "cancel" | "pick";
  offer: Offer;
  go: Go;
  variant?: string;
  mini?: boolean;
}) {
  const [packs, setPacks] = useState(variant === "packs");
  const [q, search] = useState("");
  const [choice, choose] = useState(offer[0] as string);
  const [qty, setQty] = useState(
    String(variant === "invalid" ? offer[5] + 1 : offer[5]),
  );
  const [price, setPrice] = useState(variant === "invalid" ? "0" : "500");
  const frozen = ["pending", "unknown"].includes(variant),
    incorrect =
      Number(qty) < 1 ||
      Number(qty) > offer[5] ||
      !Number.isInteger(Number(qty)) ||
      !Number.isInteger(Number(price)) ||
      Number(price) < 1;
  const [notice, notify] = useState("");
  const close = () => {
    if (!frozen) go(mode === "cancel" ? "ML-02" : "ML-01");
  };
  const title =
    mode === "buy"
      ? "Buy listing"
      : mode === "cancel"
        ? "Cancel listing?"
        : "Create listing";
  const choices = entries.filter(
    (o) =>
      Boolean(o[2]) !== packs && o[1].toLowerCase().includes(q.toLowerCase()),
  );
  return (
    <Window title={title} embedded={mini} close={close}>
      {mode === "pick" ? (
        <>
          <div className="soc-tabs">
            {["Cards", "Packs"].map((tab, i) => (
              <button
                key={tab}
                aria-pressed={packs === !!i}
                onClick={() => {
                  setPacks(!!i);
                  choose("");
                  search("");
                }}
              >
                {tab}
              </button>
            ))}
          </div>
          <Search
            label={packs ? "Search packs" : "Search cards"}
            value={q}
            set={search}
          />
          {variant === "empty" || !choices.length ? (
            <p className="soc-quiet">
              {q
                ? "No matching items"
                : "No available " + (packs ? "packs" : "cards")}
            </p>
          ) : (
            <OwnedItemPicker
              mode="select"
              label="Owned items"
              selectedId={choice}
              onSelect={choose}
              items={choices.map((o) => ({
                id: o[0],
                name: o[1],
                kind: o[2] ? "card" : "pack",
                detail: o[2] ? o[2] + " · " + o[3] : "",
                available: o[5],
                artwork: listingArt(o[0]),
              }))}
            />
          )}
          <div className="soc-actions">
            <Btn
              disabled={!choice || variant === "empty"}
              onClick={() => {
                sessionStorage.setItem("sw-review-listing", choice);
                go("ML-07");
              }}
            >
              Next
            </Btn>
          </div>
        </>
      ) : (
        <>
          {mode === "create" && (
            <button
              className="soc-link"
              disabled={frozen}
              onClick={() => go("ML-06")}
            >
              ‹ Back
            </button>
          )}
          <ListingItemSummary
            item={item(offer)}
            layout={mode === "cancel" ? "inline" : "detail"}
            {...(mode === "buy" ? { seller: offer[4] } : {})}
            {...(mode === "cancel"
              ? { price: { amount: offer[6], iconSrc: coin } }
              : {})}
          />
          {mode === "buy" ? (
            <div className="soc-line">
              Total <Coins n={offer[6]} />
            </div>
          ) : mode === "cancel" ? (
            <p>The items will return to your available collection.</p>
          ) : (
            <>
              <label className="soc-line">
                Quantity{" "}
                <Stepper
                  qty={qty}
                  set={setQty}
                  max={offer[5]}
                  disabled={frozen}
                />
              </label>
              <label className="soc-line">
                Total price{" "}
                <input
                  aria-label="Total price"
                  value={price}
                  onChange={(e) => setPrice(e.target.value)}
                  disabled={frozen}
                />
              </label>
              {incorrect && (
                <small className="soc-error">
                  Only {offer[5]} available. Enter a whole number above 0.
                </small>
              )}
            </>
          )}
          {variant === "unknown" && (
            <p>
              {mode === "buy"
                ? "Purchase"
                : mode === "create"
                  ? "Listing"
                  : "Cancellation"}{" "}
              not confirmed.
            </p>
          )}
          {variant === "rejected" && (
            <p className="soc-error">Not enough available coins.</p>
          )}
          {notice && <Notice>{notice}</Notice>}
          <div className="soc-actions">
            {mode !== "create" &&
              !(mode === "cancel" && variant === "unknown") && (
                <Btn onClick={close} disabled={frozen}>
                  {mode === "cancel" ? "Keep listing" : "Cancel"}
                </Btn>
              )}
            <Btn
              danger={mode === "cancel"}
              disabled={
                variant === "pending" ||
                variant === "rejected" ||
                (mode === "create" && incorrect)
              }
              onClick={() => {
                if (variant === "unknown") {
                  notify("Checking status…");
                  return;
                }
                if (mode === "cancel") {
                  sessionStorage.setItem("sw-cancelled-" + offer[0], "1");
                  sessionStorage.setItem("sw-lot-notice", "Listing cancelled.");
                } else if (mode === "create") {
                  sessionStorage.setItem(
                    "sw-own-lot-" + offer[0],
                    JSON.stringify({
                      quantity: Number(qty),
                      total: Number(price),
                    }),
                  );
                  sessionStorage.removeItem("sw-cancelled-" + offer[0]);
                  sessionStorage.setItem("sw-lot-notice", "Listing created.");
                } else {
                  sessionStorage.setItem("sw-bought-lot", offer[0]);
                  sessionStorage.setItem(
                    "sw-buy-notice",
                    "Purchased " +
                      offer[5] +
                      " " +
                      (offer[2] ? "copies of " : "packs of ") +
                      offer[1] +
                      ".",
                  );
                }
                go(
                  mode === "buy"
                    ? "ML-05"
                    : mode === "create"
                      ? "ML-02"
                      : "ML-02",
                );
              }}
            >
              {variant === "unknown"
                ? "Check status"
                : mode === "buy"
                  ? "Buy"
                  : mode === "cancel"
                    ? "Cancel listing"
                    : "Create listing"}
            </Btn>
          </div>
        </>
      )}
    </Window>
  );
}
export function Listings({
  id,
  go,
  data,
}: {
  id: string;
  go: (id: string, data?: unknown) => void;
  data?: unknown;
}) {
  const [q, search] = useState(""),
    [type, setType] = useState("All items"),
    [sort, setSort] = useState("Newest"),
    [buying, setBuying] = useState(id === "ML-04"),
    [mine, setMine] = useState(id === "ML-02" || id === "ML-09"),
    [sel, select] = useState(
      id === "ML-05"
        ? "counter"
        : sessionStorage.getItem("sw-review-listing") || "bolt",
    );
  const inspectButton = useRef<HTMLButtonElement>(null);
  useLayoutEffect(() => {
    if (id === "ML-01" && data === "focus-listing-action")
      inspectButton.current?.focus({ preventScroll: true });
  }, [id, data]);
  const [facets, setFacets] = useState<Facets>(allFacets);
  const purchases = purchasedListings();
  const balance = remainingBalance(purchases);
  const [purchaseMessage, notifyPurchase] = useState("");
  function purchase(chosen: Offer) {
    const settled = purchasedListings();
    if (
      settled.some((row) => row[0] === chosen[0]) ||
      remainingBalance(settled) < chosen[6]
    ) {
      notifyPurchase(
        settled.some((row) => row[0] === chosen[0])
          ? "Listing unavailable."
          : "Not enough coins.",
      );
      setBuying(false);
      return;
    }
    sessionStorage.setItem(
      "sw-market-purchased-ids",
      [...settled, chosen].map((row) => row[0]).join(","),
    );
    sessionStorage.setItem("sw-bought-lot", chosen[0]);
    sessionStorage.setItem(
      "sw-buy-notice",
      `Purchased ${chosen[5]} ${chosen[2] ? "copies" : "packs"} of ${chosen[1]}.`,
    );
    setBuying(false);
    go("ML-05");
  }
  const available = mine ? entries.map(ownOffer) : entries;
  const offer = available.find((o) => o[0] === sel) ?? available[0]!;
  const rows = available.filter(
    (o) =>
      (mine || !purchases.some((p) => p[0] === o[0])) &&
      matchesFacets(o, facets) &&
      (!mine ||
        o[0] === "bolt" ||
        o[0] === "ring" ||
        sessionStorage.getItem("sw-own-lot-" + o[0])) &&
      !(mine && sessionStorage.getItem("sw-cancelled-" + o[0])) &&
      (mine ||
        id !== "ML-05" ||
        o[0] !== (sessionStorage.getItem("sw-bought-lot") || "bolt")) &&
      o[1].toLowerCase().includes(q.toLowerCase()) &&
      (type === "All items" || Boolean(o[2]) === (type === "Cards")),
  );
  if (sort === "Price: Low to high") rows.sort((a, b) => a[6] - b[6]);
  if (sort === "Price: High to low") rows.sort((a, b) => b[6] - a[6]);
  if (sort === "Name: A–Z") rows.sort((a, b) => a[1].localeCompare(b[1]));
  const selected = rows.includes(offer) ? offer : rows[0];
  function openTerms(target: string) {
    if (selected) sessionStorage.setItem("sw-review-listing", selected[0]);
    go(target);
  }
  if (id === "ML-03")
    return (
      <Board
        title=""
        panels={[
          [
            "A Cards filters",
            <Filter
              initial={{
                ...allFacets,
                sets: ["M11"],
                colors: ["Red"],
                maxMana: "3",
              }}
            />,
          ],
          [
            "B Packs filters",
            <Filter
              packs
              initial={{ ...allFacets, sets: ["Foundations", "Bloomburrow"] }}
            />,
          ],
        ]}
      />
    );
  if (id === "ML-08")
    return (
      <Board
        title=""
        panels={[
          [
            "A Pack inspector",
            <ListingItemSummary
              item={item(entries[4])}
              layout="detail"
              seller="Finch"
            />,
          ],
          [
            "B Choose a pack",
            <Terms
              mode="pick"
              offer={entries[4]}
              variant="packs"
              go={go}
              mini
            />,
          ],
          [
            "C Create validation",
            <Terms
              mode="create"
              offer={entries[0]}
              variant="invalid"
              go={go}
              mini
            />,
          ],
          [
            "D No available items",
            <Terms mode="pick" offer={offer} variant="empty" go={go} mini />,
          ],
        ]}
      />
    );
  if (id === "ML-10")
    return (
      <Board
        title="Market listing controls"
        panels={[
          [
            "Item type, sort and finish",
            <div className="soc-column">
              {[
                ["All items", "Cards", "Packs"],
                [
                  "Newest",
                  "Price: Low to high",
                  "Price: High to low",
                  "Name: A–Z",
                ],
                ["Any", "Nonfoil", "Foil"],
              ].map((opts) => (
                <select aria-label={opts[0]} key={opts[0]} size={opts.length}>
                  {opts.map((v) => (
                    <option key={v}>{v}</option>
                  ))}
                </select>
              ))}
            </div>,
          ],
          [
            "Matching listings",
            <>
              <Search
                label="Search listings"
                value={q || "bolt"}
                set={search}
              />
              <ListingRow
                item={item(entries[0])}
                seller="Mira"
                selected
                price={{ amount: 500, iconSrc: coin }}
                onSelect={() => go("ML-01")}
              />
            </>,
          ],
          [
            "Empty listings",
            <>
              <p>No matching listings</p>
              <p>No listings yet</p>
              <p>No active listings</p>
            </>,
          ],
          [
            "Read feedback",
            <>
              <Skeleton />
              <Notice>Couldn’t load listings.</Notice>
            </>,
          ],
        ]}
      />
    );
  if (id === "ML-11") {
    return (
      <Artboard width={786} height={860}>
        <Box area={[22, 4, 742, 37]}>
          <h1>Market · Operation states</h1>
        </Box>
        {[
          "Pending (original attempt in progress, no other actions)",
          "Original attempt recovery (same dialog, outcome unknown → check status)",
        ].map((label, r) => (
          <div key={label}>
            <Box area={[22, 46 + r * 261, 742, 32]}>
              <h3>
                {r === 0 ? "Ⓐ" : "Ⓑ"} {label}
              </h3>
            </Box>
            {(["buy", "create", "cancel"] as const).map((mode, c) => (
              <Box
                key={mode}
                area={[21 + c * 251, 80 + r * 261, 240, 212]}
                scale={0.54}
                className="soc-lot-excerpt"
              >
                <Terms
                  mode={mode}
                  offer={offer}
                  go={go}
                  mini
                  variant={r === 0 ? "pending" : "unknown"}
                />
              </Box>
            ))}
          </div>
        ))}
        <Box area={[22, 574, 385, 34]}>
          <h3>Ⓒ Authoritative rejection</h3>
        </Box>
        <Box
          area={[21, 610, 211, 197]}
          scale={0.52}
          className="soc-lot-excerpt"
        >
          <Terms mode="buy" offer={offer} go={go} mini variant="rejected" />
        </Box>
        <Box area={[239, 611, 164, 188]} scale={0.68}>
          <p>Toast copy alternatives</p>
          <Notice>Listing no longer available.</Notice>
          <Notice>This purchase exceeds your account limit.</Notice>
        </Box>
        <Box area={[422, 574, 342, 34]}>
          <h3>Ⓓ Resolved cancellation</h3>
        </Box>
        <Box
          area={[423, 609, 342, 195]}
          scale={0.65}
          className="soc-resolved-board"
        >
          <div>
            <Search label="Search listings" value={q} set={search} />
            <Btn onClick={() => go("ML-03")}>Filters</Btn>
            <Btn onClick={() => go("ML-06")}>Create listing</Btn>
          </div>
          <label>
            <input type="checkbox" defaultChecked /> Only my listings
          </label>
          <p className="soc-quiet">No active listings</p>
          <Notice>Listing cancelled.</Notice>
        </Box>
      </Artboard>
    );
  }
  return (
    <Shell
      area="Market"
      current="ML-01"
      go={go}
      balance={id === "ML-05" && !purchases.length ? 7500 : balance}
    >
      <MarketListingsPage
        listings={rows.map((o) => listing(o, mine))}
        selectedId={selected?.[0]}
        coinIcon={coin}
        search={q}
        onSearch={search}
        type={type}
        onType={(value) => {
          setType(value);
          if (value === "Packs")
            setFacets((f) => ({
              ...allFacets,
              min: f.min,
              max: f.max,
              sets: f.sets.filter((s) =>
                ["Foundations", "Bloomburrow"].includes(s),
              ),
            }));
        }}
        sort={sort}
        onSort={setSort}
        mine={mine}
        onMine={setMine}
        renderFilters={(close) => (
          <Filter
            packs={type === "Packs"}
            initial={facets}
            change={setFacets}
            close={close}
          />
        )}
        inspectButton={inspectButton}
        onSelect={select}
        onCreate={() => go("ML-06")}
        onInspectAction={(row) => {
          select(row.id);
          if (mine) openTerms("ML-09");
          else setBuying(true);
        }}
      />
      {selected && (
        <BuyListingDialog
          listing={listing(selected)}
          coinIcon={coin}
          returnFocusRef={inspectButton}
          open={buying}
          onCancel={() => {
            setBuying(false);
            if (id === "ML-04") go("ML-01", "focus-listing-action");
          }}
          onConfirm={() => purchase(selected)}
        />
      )}
      {purchaseMessage && (
        <div className="soc-toast">
          <Notice>{purchaseMessage}</Notice>
        </div>
      )}
      {["ML-06", "ML-07", "ML-09"].includes(id) && (
        <Terms
          mode={id === "ML-06" ? "pick" : id === "ML-07" ? "create" : "cancel"}
          offer={offer}
          go={go}
        />
      )}
      {(id === "ML-05" || (mine && id === "ML-02")) && (
        <div className="soc-toast">
          <Notice>
            {id === "ML-05"
              ? sessionStorage.getItem("sw-buy-notice") ||
                "Purchased 4 copies of Lightning Bolt."
              : sessionStorage.getItem("sw-lot-notice") || "Listing created."}
          </Notice>
        </div>
      )}
    </Shell>
  );
}

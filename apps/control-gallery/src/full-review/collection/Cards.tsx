import { SectionRail } from "../SectionRail";
import {
  useCallback,
  useEffect,
  useLayoutEffect,
  useRef,
  useState,
} from "react";
import {
  ActionButton,
  Checkbox,
  EmptyState,
  FilterChip,
  SelectMenu,
} from "@spellwake/ui";
import { ChevronDown } from "lucide-react";
import { ReviewHeader } from "../ReviewHeader";
import {
  Art,
  Board,
  Btn,
  CardArt,
  Choice,
  Example,
  Search,
  Tabs,
  cardNames,
  type Jump,
  type Rect,
} from "./common";
import { AnimatePresence } from "motion/react";
import {
  activeChips,
  boltFilters,
  browseCards,
  copies,
  emptyFilters,
  inventory,
  printLabel,
  printingMatches,
  sortOrders,
  type Filters,
} from "./card-inventory";
import { CardFilters, options } from "./CardFilters";
export function CardInspector({
  initial = 2,
  alternate = false,
  chosen,
  change,
}: {
  initial?: number;
  alternate?: boolean;
  chosen?: number;
  change?: (index: number) => void;
}) {
  const [local, setLocal] = useState(alternate && initial === 2 ? 2 : 0);
  const card = inventory[initial]!;
  const printing = chosen ?? local;
  const count = (n: number) => n + " " + (n === 1 ? "copy" : "copies");
  return (
    <aside className="cs-card-inspector" aria-label="Card details">
      <div className="cs-inspect-face">
        {initial === 2 ? (
          <Art
            source={printing === 0 ? "CLC-01" : "CLC-02"}
            rect={[1248, 126, 259, 350]}
            alt={card.name}
          />
        ) : (
          <CardArt index={initial} />
        )}
      </div>
      <h2>{card.name}</h2>
      <p>{count(copies[initial]!)}</p>
      <h4>Owned printings</h4>
      {card.printings.map((p, i) => (
        <button
          key={p.set + p.number + p.foil}
          aria-pressed={printing === i}
          onClick={() => {
            setLocal(i);
            change?.(i);
          }}
        >
          <span aria-hidden="true">{i === printing ? "✓" : ""}</span>
          <span className="cs-print-thumb">
            {initial === 2 ? (
              <Art
                source={i === 0 ? "CLC-01" : "CLC-02"}
                rect={[1260, 170, 218, 120]}
              />
            ) : (
              <CardArt index={initial} />
            )}
          </span>
          <span>
            {printLabel(p)}
            <small>{p.foil ? "Foil" : "Nonfoil"}</small>
          </span>
          <small>{count(p.count)}</small>
        </button>
      ))}
    </aside>
  );
}
type BrowseState = {
  id: string;
  query: string;
  filters: Filters;
  sort: string;
  selected: number;
  printing: number;
  scroll: number;
  open: boolean;
};
export function CardLibrary({ id, go }: { id: string; go: Jump }) {
  const [state, setState] = useState<BrowseState>(() => {
    const saved = window.parent.history.state?.cardBrowser as
      BrowseState | undefined;
    return saved?.id === id
      ? saved
      : {
          id,
          query: "",
          filters: id === "CLC-03" ? boltFilters : emptyFilters,
          sort: sortOrders[0],
          selected: 2,
          printing: id === "CLC-02" ? 2 : 0,
          scroll: 0,
          open: id === "CLC-03",
        };
  });
  const open = state.open;
  const trigger = useRef<HTMLButtonElement>(null);
  const grid = useRef<HTMLDivElement>(null);
  const initialScroll = useRef(state.scroll);
  const close = useCallback((restore: boolean) => {
    setState((prev) => ({ ...prev, open: false }));
    if (restore) trigger.current?.focus();
  }, []);
  const entries = browseCards(state.query, state.filters, state.sort);
  const current = entries.find((e) => e.index === state.selected);
  const chips = activeChips(state.filters);
  function update(patch: Partial<BrowseState>) {
    setState((prev) => {
      const next = { ...prev, ...patch };
      const results = browseCards(next.query, next.filters, next.sort);
      const selected =
        results.find((e) => e.index === next.selected) ?? results[0];
      const printing = selected?.card.printings[next.printing];
      if (
        selected?.index !== prev.selected ||
        !printing ||
        (patch.filters && !printingMatches(printing, next.filters))
      )
        next.printing = selected?.printing ?? 0;
      next.selected = selected?.index ?? -1;
      return next;
    });
  }
  useLayoutEffect(() => {
    if (grid.current) grid.current.scrollTop = initialScroll.current;
  }, []);
  useEffect(() => {
    const history = window.parent.history;
    history.replaceState({ ...history.state, cardBrowser: state }, "");
  }, [state]);
  function clearAll() {
    update({ filters: emptyFilters, query: "" });
    document
      .querySelector<HTMLInputElement>('.cs-card-browser input[type="search"]')
      ?.focus();
  }
  function removeChip(index: number) {
    const next = chips[index + 1] ?? chips[index - 1];
    const buttons = trigger.current
      ?.closest("section")
      ?.querySelectorAll<HTMLButtonElement>(".cs-library-chips button");
    if (next) buttons?.[chips.indexOf(next)]?.focus();
    else trigger.current?.focus();
    update({ filters: chips[index]!.remove() });
  }
  return (
    <>
      <ReviewHeader active="Collection" go={go} coins={0} />
      <div className="cs-workspace cs-cards-page">
        <SectionRail area="Collection" current="CLC-01" go={go} />
        <section className="cs-card-browser" aria-label="Owned cards">
          <div className="cs-toolbar">
            <Search
              label="Search cards"
              value={state.query}
              onChange={(query) => update({ query })}
            />
            <ActionButton
              ref={trigger}
              aria-expanded={open}
              aria-controls="card-filters"
              treatment="compact-form"
              onClick={() =>
                setState((prev) => ({ ...prev, open: !prev.open }))
              }
            >
              Filters{chips.length ? " (" + chips.length + ")" : ""}
              <ChevronDown />
            </ActionButton>
            <SelectMenu
              label="Sort cards"
              valuePrefix="Sort: "
              value={state.sort}
              options={options(sortOrders)}
              onValueChange={(sort) => {
                update({ sort });
                requestAnimationFrame(() =>
                  grid.current
                    ?.querySelector('[aria-pressed="true"]')
                    ?.scrollIntoView({ block: "nearest" }),
                );
              }}
            />
          </div>
          {!!chips.length && (
            <div className="cs-library-chips">
              <AnimatePresence initial={false}>
                {chips.map((chip, i) => (
                  <FilterChip
                    key={chip.label}
                    label={chip.label}
                    onRemove={() => removeChip(i)}
                  />
                ))}
              </AnimatePresence>
              {!open && (
                <Btn
                  onClick={() => {
                    update({ filters: emptyFilters });
                    trigger.current?.focus();
                  }}
                >
                  Clear filters
                </Btn>
              )}
            </div>
          )}
          <div
            className="cs-card-grid"
            ref={grid}
            onScroll={(e) => {
              const scroll = e.currentTarget.scrollTop;
              setState((prev) => ({ ...prev, scroll }));
            }}
          >
            {entries.map(({ card, index, printing }) => (
              <button
                key={index}
                onClick={() => update({ selected: index, printing })}
                aria-pressed={index === state.selected}
              >
                <div>
                  {printing === 0 ? (
                    <CardArt index={index} />
                  ) : (
                    <Art
                      source="CLC-02"
                      rect={[1248, 126, 259, 350]}
                      alt={card.name}
                    />
                  )}
                  <small>×{copies[index]}</small>
                </div>
                {card.name}
                <small>{printLabel(card.printings[printing]!)}</small>
              </button>
            ))}
          </div>
          {!entries.length && (
            <EmptyState
              title="No matching cards"
              action={<Btn onClick={clearAll}>Clear search and filters</Btn>}
            />
          )}
        </section>
        <div className="cs-inspector-slot">
          {!open && current && (
            <CardInspector
              initial={current.index}
              chosen={state.printing}
              change={(printing) => update({ printing })}
            />
          )}
        </div>
        {open && (
          <CardFilters
            filters={state.filters}
            change={(filters) => update({ filters })}
            close={close}
            trigger={trigger}
          />
        )}
      </div>
    </>
  );
}
const boards: Record<string, string[]> = {
  "CLC-04": [
    "Search matches",
    "Active filters",
    "Sort menu",
    "Owned lists",
    "Printing changed",
    "Another card selected",
  ],
  "CLC-05": [
    "Empty collection",
    "No matches",
    "Initial loading",
    "Loading more",
    "Read unavailable",
    "Append failed",
  ],
  "CLC-06": [
    "Two-faced card · front",
    "Two-faced card · back",
    "Long name",
    "Artwork unavailable",
    "Many printings",
    "Keyboard focus",
  ],
  "CLC-07": [
    "Format menu",
    "Set search",
    "Card type",
    "Color identity",
    "Mana value",
    "Live results",
  ],
  "CLC-08": [
    "Loading artwork",
    "Artwork unavailable",
    "Artwork recovered",
    "Range cleared",
  ],
};
export function CardBoard({ id, go }: { id: string; go: Jump }) {
  return (
    <Board
      title={
        id === "CLC-07"
          ? "Filter control details"
          : "Spellwake Collection Cards"
      }
    >
      {(boards[id] ?? []).map((v, i) => (
        <Example
          key={v}
          title={v}
          width={800}
          height={id === "CLC-07" ? 400 : 330}
        >
          <CardBoardItem id={id} i={i} go={go} />
        </Example>
      ))}
    </Board>
  );
}
function CardBoardItem({ id, i, go }: { id: string; i: number; go: Jump }) {
  const details = id === "CLC-08",
    feedback = id === "CLC-05";
  const [q, search] = useState(
      !feedback && !details && i === 0
        ? "counter"
        : feedback && i === 1
          ? "zzzz"
          : "",
    ),
    [idx, select] = useState(
      feedback || details ? 0 : i === 0 || i === 5 ? 0 : 2,
    ),
    [sort, setSort] = useState(
      id === "CLC-04" && i === 3 ? "Most copies" : "Name A–Z",
    ),
    [filter, setFilter] = useState(
      id === "CLC-04" && i === 1
        ? { ...emptyFilters, colors: ["Red"], types: ["Instant"] }
        : emptyFilters,
    );
  const chips = activeChips(filter);
  if (id === "CLC-07") return <FilterExample i={i} />;
  if (id === "CLC-06") return <InspectorExample i={i} />;
  const initial = feedback && i < 3,
    failed = feedback && i === 4,
    unavailable = details && i === 1,
    loading = details && i === 0,
    items = browseCards(q, filter, sort)
      .slice(0, id === "CLC-04" && i === 3 ? 3 : 4)
      .map((entry) => entry.index);
  const shown = items.includes(idx) ? idx : items[0];
  return (
    <div className="cs-board-browser">
      <div className="cs-toolbar">
        <Search label="Search cards" value={q} onChange={search} />
        <Btn onClick={() => go("CLC-03")}>
          Filters{chips.length ? " (" + chips.length + ")" : ""}
        </Btn>
        <Choice
          value={sort}
          items={sortOrders}
          change={setSort}
          open={!details && !feedback && i === 2}
        />
      </div>
      {!!chips.length && (
        <div className="cs-chips">
          {chips.map((chip) => (
            <FilterChip
              key={chip.label}
              label={chip.label}
              onRemove={() => setFilter(chip.remove())}
            />
          ))}
        </div>
      )}
      {initial ? (
        <div className="cs-center">
          <h2>{["No cards yet", "No matching cards", "Loading cards…"][i]}</h2>
          <small>
            {i === 0
              ? "Cards that you own will appear here."
              : i === 1
                ? "Try another search or filter."
                : ""}
          </small>
        </div>
      ) : (
        !failed && (
          <div className="cs-mini-card-list">
            {items.map((j) => (
              <button key={j} onClick={() => select(j)}>
                {id === "CLC-04" && i === 3 && j === 2 ? (
                  <CardArt index={2} alternate />
                ) : (
                  <CardArt index={j} />
                )}
                <span>{cardNames[j]}</span>
              </button>
            ))}
            {unavailable || loading ? (
              <aside className="cs-art-status">
                <div>{unavailable ? "Artwork unavailable" : "◌"}</div>
                <h3>Counterspell</h3>
                <p>2 copies</p>
                <small>Owned printings</small>
              </aside>
            ) : (
              shown !== undefined && (
                <CardInspector
                  key={shown}
                  initial={shown}
                  alternate={id === "CLC-04" && i === 4}
                />
              )
            )}
          </div>
        )
      )}
      {feedback && i > 2 && (
        <span className="cs-toast">
          {
            [
              "Loading more cards…",
              "Couldn’t load your cards.",
              "Couldn’t load more cards.",
            ][i - 3]
          }
        </span>
      )}
    </div>
  );
}
const inspectorNames = [
  "Delver of Secrets",
  "Insectile Aberration",
  "Asmoranomardicadaistinaculdacar",
  "Opt",
  "Lightning Bolt",
  "Counterspell",
];
const inspectorArts: readonly Rect[] = [
  [204, 84, 205, 257],
  [961, 85, 203, 255],
  [191, 379, 221, 263],
  [971, 380, 202, 265],
  [193, 684, 215, 270],
  [970, 692, 202, 252],
];
function InspectorExample({ i }: { i: number }) {
  const [flip, setFlip] = useState(false),
    [selected, setSelected] = useState(0);
  const which = i < 2 && flip ? 1 - i : i;
  const list =
    which === 4
      ? [
          "M11 · 146 · Nonfoil",
          "2X2 · 117 · Nonfoil",
          "2X2 · 117 · Foil",
          "A25 · 141 · Nonfoil",
          "A25 · 141 · Foil",
          "STA · 105 · Nonfoil",
        ]
      : [
          [
            "ISD · 51 · Nonfoil",
            "ISD · 51 · Nonfoil",
            "MH2 · 186 · Nonfoil",
            "M21 · 59 · Nonfoil",
            "",
            "MH2 · 267 · Nonfoil",
          ][which] ?? "",
        ];
  return (
    <div className="cs-board-inspect cs-printing-example">
      <Art
        source="CLC-06"
        rect={inspectorArts[which] ?? inspectorArts[0]!}
        alt={inspectorNames[which] ?? ""}
      />
      <div>
        <h2>{inspectorNames[which]}</h2>
        <i>
          {[3, 3, 1, 6, 30, 2][which]} {which === 2 ? "copy" : "copies"}
        </i>
        <h4>Owned printings</h4>
        <div className="cs-printings">
          {list.map((line, j) => (
            <button
              key={line}
              aria-pressed={j === selected}
              onClick={() => setSelected(j)}
            >
              {j === selected ? "✓ " : " "}
              {line}
              <small>
                {which === 4
                  ? [7, 6, 4, 5, 4, 4][j]
                  : [3, 3, 1, 6, 30, 2][which]}{" "}
                {which === 2 ? "copy" : "copies"}
              </small>
            </button>
          ))}
        </div>
        {which < 2 && <Btn onClick={() => setFlip(!flip)}>⟳ Flip card</Btn>}
      </div>
    </div>
  );
}
function FilterExample({ i }: { i: number }) {
  const [value, setValue] = useState("Modern"),
    [q, search] = useState("modern"),
    [q2, search2] = useState("zzzz"),
    [checked, setChecked] = useState(
      i === 1 ? ["Modern Horizons 2 (MH2)"] : ["Instant", "Creature"],
    ),
    [color, setColor] = useState("Color identity"),
    [colorOn, setColorOn] = useState(3),
    [min, setMin] = useState("5"),
    [max, setMax] = useState("2"),
    [chips, setChips] = useState(["M11", "Red", "Instant", "Common", "MV 0–3"]);
  const checks = (labels: string[]) =>
    labels.map((v) => (
      <Checkbox
        key={v}
        checked={checked.includes(v)}
        label={v}
        onCheckedChange={(on) =>
          setChecked((prev) =>
            on ? [...prev, v] : prev.filter((x) => x !== v),
          )
        }
      />
    ));
  if (i === 0)
    return (
      <div className="cs-board-filter">
        <Choice
          label="Format"
          value={value}
          items={[
            "All formats",
            "Standard",
            "Modern",
            "Pioneer",
            "Legacy",
            "Vintage",
            "Commander",
          ]}
          change={setValue}
          open
        />
      </div>
    );
  if (i === 1)
    return (
      <div className="cs-board-filter">
        {[q, q2].map((query, k) => (
          <div className="cs-filter-line" key={k}>
            <Search
              label="Search sets"
              value={query}
              onChange={k === 0 ? search : search2}
            />
            <div>
              {[
                "Modern Horizons 2 (MH2)",
                "Modern Horizons 3 (MH3)",
                "Modern Masters (MMA)",
              ].some((n) => n.toLowerCase().includes(query.toLowerCase())) ? (
                checks(
                  [
                    "Modern Horizons 2 (MH2)",
                    "Modern Horizons 3 (MH3)",
                    "Modern Masters (MMA)",
                  ].filter((n) =>
                    n.toLowerCase().includes(query.toLowerCase()),
                  ),
                )
              ) : (
                <i>No sets found</i>
              )}
            </div>
          </div>
        ))}
      </div>
    );
  if (i === 2)
    return (
      <div className="cs-board-filter cs-filter-line">
        <div className="cs-chips">
          {checked.map((v) => (
            <button
              onClick={() => setChecked((prev) => prev.filter((x) => x !== v))}
              key={v}
            >
              {v} ×
            </button>
          ))}
        </div>
        <div>
          {checks([
            "Instant",
            "Creature",
            "Artifact",
            "Enchantment",
            "Land",
            "Planeswalker",
            "Sorcery",
            "Battle",
          ])}
        </div>
      </div>
    );
  if (i === 3)
    return (
      <div className="cs-board-filter cs-center">
        <Tabs
          labels={["Color", "Color identity"]}
          value={color}
          change={setColor}
        />
        <div className="cs-mana">
          {["☀", "◉", "☠", "♦", "♣"].map((v, j) => (
            <button
              aria-pressed={colorOn === j}
              key={v}
              onClick={() => setColorOn(j)}
            >
              {v}
            </button>
          ))}
        </div>
        <i>Contains any selected color</i>
        {checks(["Colorless"])}
      </div>
    );
  if (i === 4)
    return (
      <div className="cs-board-filter cs-center">
        <div className="cs-filter-line">
          Min
          <input type="number" defaultValue="0" aria-label="Valid minimum" />
          Max
          <input type="number" defaultValue="3" aria-label="Valid maximum" />
        </div>
        <div className="cs-filter-line">
          Min
          <input
            type="number"
            value={min}
            aria-label="Minimum"
            onChange={(e) => setMin(e.currentTarget.value)}
          />
          Max
          <input
            type="number"
            value={max}
            aria-label="Maximum"
            onChange={(e) => setMax(e.currentTarget.value)}
          />
        </div>
        {Number(min) > Number(max) && <i>Maximum must be at least minimum.</i>}
        <small>Invalid draft retains last valid results.</small>
      </div>
    );
  return (
    <div className="cs-board-filter">
      <div className="cs-chips">
        {chips.map((x) => (
          <button
            key={x}
            onClick={() => setChips((old) => old.filter((v) => v !== x))}
          >
            {x} ×
          </button>
        ))}
      </div>
      <div className="cs-printing-example cs-board-inspect">
        <CardArt index={2} />
        <div>
          <h2>Lightning Bolt</h2>
          <p>7 copies</p>
          <p>Owned printings</p>
          {["M11 · 146", "2X2 · 117", "2X2 · 117 · Foil"].map((x, j) => (
            <p key={x}>
              {x} {[4, 2, 1][j]}
            </p>
          ))}
        </div>
      </div>
    </div>
  );
}

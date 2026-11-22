import { useState } from "react";
import { motion, useReducedMotion } from "motion/react";
import { builderCard, deckGroups, searchPrintings } from "./builder-cards";
import {
  ActionMenu,
  OriginalCardFace,
  QuantityControl,
  SelectMenu,
  Tooltip,
} from "@spellwake/ui";
import {
  ChevronDown,
  Copy,
  EllipsisVertical,
  Palette,
  Pencil,
  Plus,
  Trash2,
  Upload,
} from "lucide-react";
import {
  Art,
  Board,
  Btn,
  CardArt,
  Choice,
  Example,
  Modal,
  Search,
  Tabs,
  type Jump,
} from "./common";
import type {
  DemoDeck,
  DemoDeckEntry,
} from "../../../../play/src/collection/data/demo-model";
import {
  savedDeck,
  updateSavedDeck,
  rememberFormat,
  formats,
  type DeckLaunch,
} from "./deck-library";
import { CosmeticsDialog } from "./Settings";
const mainRows = [
  "Skirk Prospector",
  "Mogg War Marshal",
  "Goblin Instigator",
  "Goblin Warchief",
  "Goblin Chieftain",
  "Goblin Matron",
  "Lightning Bolt",
  "Shock",
  "Krenko’s Command",
  "Mountain",
];
function BuilderArt({ name, full = false }: { name: string; full?: boolean }) {
  const card = builderCard(name);
  if (full && card?.face)
    return (
      <OriginalCardFace
        image={{ name: card.name, src: card.face, width: 488, height: 680 }}
      />
    );
  if (!full && card?.art)
    return <img className="cs-art" src={card.art} alt="" />;
  if (card?.inventoryIndex !== undefined)
    return <CardArt index={card.inventoryIndex} />;
  return full ? (
    <div className="cs-center">
      <strong>{name}</strong>
      <small>Artwork unavailable</small>
    </div>
  ) : (
    <span className="cs-art" aria-hidden="true" />
  );
}
const manaIcons = import.meta.glob<string>(
  "../../../../../output/imagegen/guildhall-screen-set/components/assets/mana/{1,2,3,4,R}.svg",
  { import: "default", query: "?inline", eager: true },
);
function RowCost({ name }: { name: string }) {
  const cost = builderCard(name)?.cost;
  if (!cost) return null;
  return (
    <span className="cs-row-cost" role="img" aria-label={"Mana cost " + cost}>
      {Array.from(cost.matchAll(/\{([^}]+)\}/g), ([token, symbol], i) => {
        const src =
          manaIcons[
            "../../../../../output/imagegen/guildhall-screen-set/components/assets/mana/" +
              symbol +
              ".svg"
          ];
        return src ? (
          <img alt="" key={i} src={src} />
        ) : (
          <span key={i}>{token}</span>
        );
      })}
    </span>
  );
}
const sectionNames = ["Main", "Sideboard", "Considering"] as const;
type TabSection = (typeof sectionNames)[number];
const sections = {
  Main: "main",
  Sideboard: "sideboard",
  Considering: "considering",
} as const;
function startingEntries(record?: DemoDeck): DemoDeckEntry[] {
  return (
    record?.entries ??
    mainRows.map((name) => ({
      id: crypto.randomUUID(),
      name,
      section: "main",
      quantity: name === "Mountain" ? "20" : "4",
    }))
  );
}

export function Builder({
  id,
  go,
  launch,
}: {
  id: string;
  go: Jump;
  launch?: DeckLaunch | undefined;
}) {
  const [record, setRecord] = useState(() =>
    launch ? savedDeck(launch.source, launch.deckId) : undefined,
  );
  if (launch && !record)
    return (
      <div className="cs-center">
        <p>Couldn’t open deck.</p>
        <Btn onClick={() => go(launch.source, launch)}>‹ Decks</Btn>
      </div>
    );
  return (
    <Workbench
      id={id}
      go={go}
      launch={launch}
      record={record}
      onSaved={setRecord}
    />
  );
}
function Workbench({
  id,
  go,
  launch,
  record,
  onSaved,
}: {
  id: string;
  go: Jump;
  launch: DeckLaunch | undefined;
  record: DemoDeck | undefined;
  onSaved: (deck: DemoDeck) => void;
}) {
  const [category, setCategory] = useState<TabSection>("Main"),
    [format, setFormat] = useState<string>(record?.format ?? "Modern"),
    [mode, setMode] = useState(id === "DB-02" ? "Cards" : "List"),
    [selected, select] = useState(
      record
        ? (record.entries.find((e) => e.section === "main")?.name ?? "")
        : "Lightning Bolt",
    ),
    [q, search] = useState(""),
    [group, setGroup] = useState("Group: Type"),
    [sort, setSort] = useState("Sort: Name"),
    [color, setColor] = useState("Color: All"),
    [type, setType] = useState("Type: All"),
    [deckName, rename] = useState(record?.name ?? "Goblins"),
    [dirty, setDirty] = useState(false),
    [modal, setModal] = useState(id === "DB-07" ? "Appearance" : ""),
    [notice, setNotice] = useState(""),
    [collapsed, collapse] = useState<string[]>([]);
  const [entries, setEntries] = useState(() => startingEntries(record));
  const leave = () => go(launch?.source ?? "DL-01", launch);
  function save() {
    if (record && launch) {
      const savedFormat = formats.find((f) => f === format);
      if (!savedFormat) return false;
      const result = updateSavedDeck(launch.source, record, {
        name: deckName,
        format: savedFormat,
        entries,
      });
      if (result?.status !== "success") {
        setNotice(
          result?.message ?? "Couldn’t save. Your edits are still here.",
        );
        setModal("");
        return false;
      }
      onSaved(result.deck);
      rename(result.deck.name);
      rememberFormat(launch.source, result.deck.format);
    }
    setDirty(false);
    setNotice("Saved.");
    return true;
  }
  const section = sections[category];
  const holdings: Record<string, bigint> = Object.create(null);
  const imported = new Map<string, NonNullable<DemoDeckEntry["catalogue"]>>();
  for (const e of entries) {
    if (e.section === section)
      holdings[e.name] = (holdings[e.name] ?? 0n) + BigInt(e.quantity);
    if (e.catalogue) imported.set(e.name, e.catalogue);
  }
  const metadata = (name: string) => imported.get(name) ?? builderCard(name);
  const quantity = holdings[selected] ?? 0n;
  const total = (tab: TabSection) =>
    entries.reduce(
      (n, e) => (e.section === sections[tab] ? n + BigInt(e.quantity) : n),
      0n,
    );
  const groups = deckGroups(
    holdings,
    group === "Group: Mana value",
    sort === "Sort: Mana value",
    metadata,
  );
  const reducedMotion = useReducedMotion();
  const [sortAnnouncement, announceSort] = useState("");
  function changeCount(delta: 1n | -1n, name = selected) {
    if (!name) return;
    setEntries((previous) => {
      const idx = previous.findLastIndex(
        (e) => e.name === name && e.section === section,
      );
      if (idx < 0)
        return delta > 0n
          ? [
              ...previous,
              { id: crypto.randomUUID(), name, section, quantity: "1" },
            ]
          : previous;
      const count = BigInt(previous[idx]!.quantity) + delta;
      if (!count) return previous.filter((_, i) => i !== idx);
      if (count.toString().length > 78) return previous;
      return previous.map((e, i) =>
        i === idx ? { ...e, quantity: String(count) } : e,
      );
    });
    setDirty(true);
  }
  function moveTo(destination: TabSection) {
    if (!quantity) return;
    setEntries((old) =>
      old.map((e) =>
        e.section === section && e.name === selected
          ? { ...e, section: sections[destination] }
          : e,
      ),
    );
    select("");
    setDirty(true);
  }
  const listText = (
    ["main", "sideboard", "considering", "commander", "companion"] as const
  )
    .flatMap((section) => {
      const lines = entries
        .filter((e) => e.section === section)
        .map(
          (e) =>
            e.quantity +
            " " +
            (e.sourceText?.replace(/^\s*\d+x?\s+/i, "").trim() || e.name),
        );
      return section !== "main" && lines.length ? [section, ...lines] : lines;
    })
    .join("\n");
  return (
    <div className="cs-builder">
      <header>
        <Btn onClick={() => (dirty ? setModal("Save changes?") : leave())}>
          ‹ Decks
        </Btn>
        <label>
          <input
            aria-label="Deck name"
            value={deckName}
            onChange={(e) => {
              rename(e.target.value);
              setDirty(true);
            }}
          />
          <Pencil size={17} />
        </label>
        <span />
        <Choice
          value={format}
          items={["Modern", "Standard", "Commander"]}
          change={(v) => {
            setFormat(v);
            setDirty(true);
          }}
        />
        <Btn onClick={() => setModal("Appearance")}>
          <Palette /> Appearance
        </Btn>
        <Btn disabled={!dirty} onClick={save}>
          Save
        </Btn>
        <ActionMenu
          label="Deck commands"
          defaultOpen={!launch && id === "DB-01"}
          className="workbench"
          trigger={
            <button aria-label="Deck commands">
              <EllipsisVertical />
            </button>
          }
          commands={[
            { id: "Duplicate", label: "Duplicate", leading: <Copy /> },
            { id: "Export", label: "Export", leading: <Upload /> },
            {
              id: "Delete",
              label: "Delete",
              leading: <Trash2 />,
              danger: true,
              dividerBefore: true,
            },
          ]}
          onCommand={(command) => {
            if (command === "Duplicate") setNotice("Deck duplicated.");
            else setModal(command);
          }}
        />
      </header>
      <div className="cs-builder-body">
        <aside className="cs-inspector cs-panel">
          <div className="cs-builder-face">
            {!selected ? (
              <div className="cs-center">Select a card</div>
            ) : (
              <BuilderArt name={selected} full />
            )}
          </div>
          {selected && (
            <>
              <QuantityControl
                label={selected + " quantity"}
                value={String(quantity)}
                canDecrease={quantity > 0}
                canIncrease
                onDecrease={() => changeCount(-1n)}
                onIncrease={() => changeCount(1n)}
              />
              {sectionNames
                .filter((v) => v !== category)
                .map((v) => (
                  <Btn
                    key={v}
                    disabled={quantity === 0n}
                    onClick={() => moveTo(v)}
                  >
                    {v}
                  </Btn>
                ))}
            </>
          )}
        </aside>
        <main className="cs-panel cs-deck-list">
          <Tabs
            labels={sectionNames.map((v) => v + " " + total(v))}
            value={category + " " + total(category)}
            change={(v) => {
              setCategory(
                sectionNames.find((tab) => v.startsWith(tab + " ")) ?? "Main",
              );
              select("");
            }}
          />
          <div className="cs-list-controls">
            <Btn onClick={() => setMode("List")}>☷ List</Btn>
            <Btn onClick={() => setMode("Cards")}>▧ Cards</Btn>
            <span />
            <SelectMenu
              label="Group cards"
              value={group}
              options={["Group: Type", "Group: Mana value"].map((value) => ({
                value,
                label: value,
              }))}
              onValueChange={setGroup}
            />
            <SelectMenu
              label="Sort cards"
              value={sort}
              options={["Sort: Name", "Sort: Mana value"].map((value) => ({
                value,
                label: value,
              }))}
              onValueChange={(v) => {
                setSort(v);
                announceSort(
                  v === "Sort: Mana value"
                    ? "Sorted by mana value, lowest first."
                    : "Sorted by name, A to Z.",
                );
              }}
            />
            <span className="cs-sort-status" role="status">
              {sortAnnouncement}
            </span>
          </div>
          {groups.length === 0 ? (
            <div className="cs-center">
              {category === "Main" ? "No cards yet" : `No cards in ${category}`}
            </div>
          ) : (
            <div
              className={"cs-build-groups " + (mode === "Cards" ? "tiles" : "")}
            >
              {groups.map(({ label: h, rows, count }) => (
                <section key={h}>
                  <button
                    className="cs-section-head"
                    aria-expanded={!collapsed.includes(h)}
                    onClick={() =>
                      collapse((c) =>
                        c.includes(h) ? c.filter((v) => v !== h) : [...c, h],
                      )
                    }
                  >
                    <ChevronDown size={16} />
                    {h}
                    <small>{count}</small>
                  </button>
                  {!collapsed.includes(h) &&
                    rows.map((v) => (
                      <motion.button
                        layout={reducedMotion ? false : "position"}
                        transition={{ duration: 0.16 }}
                        className="cs-deck-row"
                        aria-pressed={selected === v}
                        key={v}
                        onClick={() => select(v)}
                      >
                        <span className="cs-row-count">{holdings[v]}</span>
                        <BuilderArt name={v} />
                        <span className="cs-row-name">{v}</span>
                        <small>{metadata(v)?.type}</small>
                        <RowCost name={v} />
                      </motion.button>
                    ))}
                </section>
              ))}
            </div>
          )}
        </main>
        <aside className="cs-panel cs-builder-search">
          <Search label="Search cards" value={q} onChange={search} />
          <div className="cs-toolbar">
            <Choice
              value={color}
              items={["Color: All", "Red", "Blue", "Green"]}
              change={setColor}
            />
            <Choice
              value={type}
              items={["Type: All", "Creature", "Instant", "Land"]}
              change={setType}
            />
          </div>
          {searchPrintings
            .filter(
              (card) =>
                card.name.toLowerCase().includes(q.trim().toLowerCase()) &&
                (type === "Type: All" ||
                  card.type.split(" — ")[0]?.split(" ").includes(type)) &&
                (color === "Color: All" ||
                  card.colors.includes(
                    (
                      { Red: "R", Blue: "U", Green: "G" } as Record<
                        string,
                        string
                      >
                    )[color] ?? "",
                  )),
            )
            .map(({ name: v }) => (
              <div className="cs-search-result" key={v}>
                <button onClick={() => select(v)}>
                  <BuilderArt name={v} />
                  <span>
                    {v}
                    <small>{metadata(v)?.type}</small>
                  </span>
                </button>
                <Btn
                  label={`Add ${v} to ${category}`}
                  onClick={() => {
                    select(v);
                    changeCount(1n, v);
                  }}
                >
                  <Plus />
                </Btn>
              </div>
            ))}
        </aside>
      </div>
      {notice && (
        <button className="cs-toast" onClick={() => setNotice("")}>
          {notice}
        </button>
      )}
      {modal === "Appearance" ? (
        <CosmeticsDialog
          kind="appearance"
          go={go}
          onClose={() => setModal("")}
        />
      ) : modal ? (
        <Modal
          title={
            modal === "Export"
              ? "Export deck"
              : modal === "Delete"
                ? "Delete deck?"
                : modal
          }
          close={() => setModal("")}
          className="cs-compact-modal"
          footer={
            <>
              <Btn onClick={() => setModal("")}>
                {modal === "Export"
                  ? "Close"
                  : modal === "Delete"
                    ? "Cancel"
                    : "Keep editing"}
              </Btn>
              {modal === "Save changes?" && <Btn onClick={leave}>Discard</Btn>}
              <Btn
                onClick={() => {
                  if (modal === "Delete") leave();
                  else if (modal === "Save changes?") {
                    if (save()) leave();
                  } else {
                    navigator.clipboard?.writeText(listText);
                    setNotice("List copied.");
                  }
                  setModal("");
                }}
              >
                {modal === "Export"
                  ? "Copy list"
                  : modal === "Delete"
                    ? "Delete"
                    : "Save and leave"}
              </Btn>
            </>
          }
        >
          {modal === "Export" ? (
            <pre>{listText}</pre>
          ) : (
            <p>
              {modal === "Delete"
                ? "Delete " + deckName + "? This cannot be undone."
                : deckName + " has unsaved changes."}
            </p>
          )}
        </Modal>
      ) : null}
    </div>
  );
}
const boards: Record<string, string[]> = {
  "DB-03": [
    "Search Selection — Source (Search)",
    "Format",
    "Quantity Validation",
    "Role",
    "Eligible Score (Search)",
  ],
  "DB-04": ["Inline rename", "Quantity", "Group", "Sort", "Color", "Type"],
  "DB-05": [
    "Empty deck",
    "Empty section",
    "Search no results",
    "Format-change invalid card",
    "Loading / unavailable",
    "Save feedback",
  ],
  "DB-06": [
    "Eligible Commander example",
    "Assigned Commander example",
    "Eligible Companion example",
    "Assigned Companion example",
  ],
  "DB-08": ["Card backs", "Dice", "Playmats", "Pets"],
  "DB-09": ["Deck menu", "Export", "Delete", "Unsaved departure"],
  "DB-10": [
    "Save states",
    "Deck opening",
    "Addition guard",
    "Utility feedback alternatives",
  ],
  "DB-11": [
    "Mana-value grouping result",
    "Active search filters",
    "Invalid Cards tile",
    "Collapsed group",
  ],
};
export function BuilderBoard({ id, go }: { id: string; go: Jump }) {
  return (
    <Board>
      {(boards[id] ?? []).map((label, i) => (
        <Example
          title={label}
          key={label}
          width={id === "DB-08" ? 1150 : 610}
          height={id === "DB-08" ? 680 : 310}
        >
          {id === "DB-08" ? (
            <CosmeticsDialog kind="appearance" go={go} initial={label} />
          ) : (
            <BuilderExample id={id} i={i} label={label} go={go} />
          )}
        </Example>
      ))}
    </Board>
  );
}
function BuilderExample({
  id,
  i,
  label,
  go,
}: {
  id: string;
  i: number;
  label: string;
  go: Jump;
}) {
  const [value, change] = useState(
      id === "DB-04" && i === 0 ? "Goblins" : "Modern",
    ),
    [count, setCount] = useState(4),
    [open, setOpen] = useState(true),
    [assigned, assign] = useState(i % 2 === 1);
  const quantity = (
    <QuantityControl
      label="Lightning Bolt copies"
      value={count}
      onIncrease={() => setCount(count + 1)}
      onDecrease={() => setCount(count - 1)}
      canIncrease
      canDecrease={count > 0}
    />
  );
  if (id === "DB-06") {
    const role = i < 2 ? "Commander" : "Companion";
    return (
      <div className="cs-center">
        <strong>
          {i < 2 ? "Krenko, Mob Boss" : "Jegantha, the Wellspring"}
        </strong>
        <div className="cs-ability-art">
          {i < 2 ? (
            <BuilderArt name="Krenko, Mob Boss" />
          ) : (
            <Art
              source="DB-06"
              rect={[333, 615, 240, 161]}
              alt="Jegantha, the Wellspring"
            />
          )}
        </div>
        {!assigned && quantity}
        <Btn onClick={() => assign(!assigned)}>
          {assigned ? "Remove " + role.toLowerCase() : "Set as " + role}
        </Btn>
      </div>
    );
  }
  if (id === "DB-09") {
    return (
      <div className="cs-center">
        {i === 0 ? (
          <div className="cs-panel cs-account-entry">
            {["Duplicate", "Export", "Delete"].map((x) => (
              <Btn key={x} onClick={() => change(x)}>
                {x}
              </Btn>
            ))}
            {value !== "Modern" && <p>{value}</p>}
          </div>
        ) : (
          <div className="cs-panel cs-board-dialog">
            <h2>{["", "Export deck", "Delete deck?", "Save changes?"][i]}</h2>
            {i === 1 ? (
              <pre>
                4 Lightning Bolt (M11) 146
                <br />4 Shock (M21) 159
                <br />
                20 Mountain (M21) 312
              </pre>
            ) : (
              <p>
                {i === 2
                  ? "Delete Goblins? This cannot be undone."
                  : "Goblins has unsaved changes."}
              </p>
            )}
            <footer>
              {(i === 1
                ? ["Close", "Copy list", "Download .txt"]
                : i === 2
                  ? ["Cancel", "Delete"]
                  : ["Keep editing", "Discard", "Save and leave"]
              ).map((x) => (
                <Btn
                  key={x}
                  onClick={() => go(x === "Delete" ? "DL-01" : "DB-01")}
                >
                  {x}
                </Btn>
              ))}
            </footer>
          </div>
        )}
      </div>
    );
  }
  if (id === "DB-04")
    return (
      <div className="cs-center">
        {i === 0 ? (
          <input
            value={value}
            aria-label="Deck name"
            onChange={(e) => change(e.currentTarget.value)}
          />
        ) : i === 1 ? (
          quantity
        ) : (
          <Choice
            value={
              value === "Modern"
                ? (["Type", "Name", "All", "All"][i - 2] ?? "All")
                : value
            }
            items={
              i === 2
                ? ["Type", "Mana value"]
                : i === 3
                  ? ["Name", "Mana value"]
                  : i === 4
                    ? [
                        "All",
                        "White",
                        "Blue",
                        "Black",
                        "Red",
                        "Green",
                        "Colorless",
                      ]
                    : [
                        "All",
                        "Artifact",
                        "Creature",
                        "Instant",
                        "Land",
                        "Sorcery",
                      ]
            }
            change={change}
            open
          />
        )}
      </div>
    );
  if (id === "DB-05" && i < 3)
    return (
      <div className="cs-board-browser">
        <Tabs
          labels={["Main 0", "Sideboard 0", "Considering 0"]}
          value={i === 1 ? "Sideboard 0" : "Main 0"}
          change={() => go("DB-01")}
        />
        <div className="cs-center">
          <h2>
            {["No cards yet", "No cards in Sideboard", "No matching cards"][i]}
          </h2>
          {i === 0 && <i>Find cards to start building.</i>}
        </div>
      </div>
    );
  if ((id === "DB-10" && i === 3) || (id === "DB-05" && i === 5))
    return (
      <div className="cs-center">
        {(id === "DB-10"
          ? ["Deck duplicated.", "List copied.", "Couldn’t copy list."]
          : [
              "Saving…",
              "Saved.",
              "Couldn’t save. Your edits are still here.",
              "Couldn’t confirm save.",
            ]
        ).map((v) => (
          <p className="cs-notice" key={v}>
            {v}
          </p>
        ))}
      </div>
    );
  if (id === "DB-10" && i === 2)
    return (
      <div className="cs-board-guards">
        {[
          "Can’t verify format eligibility.",
          "Remove the current commander first.",
        ].map((message, j) => (
          <div key={message}>
            <div className="cs-filter-line">
              <BuilderArt
                name={j === 0 ? "Lightning Bolt" : "Krenko, Mob Boss"}
              />
              <span>{j === 0 ? "Lightning Bolt" : "Krenko, Mob Boss"}</span>
              <Tooltip
                text={message}
                placement="bottom-end"
                aria-disabled="true"
              >
                {j === 0 ? "+" : "Set as Commander"}
              </Tooltip>
            </div>
            <span className="cs-notice">{message}</span>
          </div>
        ))}
      </div>
    );
  if (id === "DB-11" && i === 1)
    return (
      <div className="cs-board-browser">
        <Search
          label="Search cards"
          value={value === "Modern" ? "goblin" : value}
          onChange={change}
        />
        <div className="cs-toolbar">
          <Choice
            label="Color"
            value="Red"
            items={["Red", "All"]}
            change={change}
          />
          <Choice
            label="Type"
            value="Creature"
            items={["Creature", "All"]}
            change={change}
          />
        </div>
        {mainRows.slice(0, 2).map((name) => (
          <div className="cs-search-result" key={name}>
            <BuilderArt name={name} />
            <span>
              {name}
              <small>Creature — Goblin</small>
            </span>
            <Btn onClick={() => setCount(count + 1)}>+</Btn>
          </div>
        ))}
      </div>
    );
  if (id === "DB-11" && (i === 0 || i === 3))
    return (
      <div className="cs-board-inspect">
        <Art source="DB-01" rect={[47, 153, 317, 464]} />
        <div>
          <Tabs
            labels={["Main 56", "Sideboard 0", "Considering 0"]}
            value={value === "Modern" ? "Main 56" : value}
            change={change}
          />
          <Btn onClick={() => setOpen(!open)}>
            {i === 0 ? "1" : "Creatures 24"}
          </Btn>
          {open && i === 0 && <p>4 Skirk Prospector</p>}
          <h4>{i === 0 ? "2" : "Instants 8"}</h4>
          {(i === 0
            ? ["Goblin Instigator", "Mogg War Marshal"]
            : ["Lightning Bolt", "Shock"]
          ).map((v) => (
            <div key={v} className="cs-deck-row">
              <BuilderArt name={v} />
              {v}
            </div>
          ))}
        </div>
      </div>
    );
  if (id === "DB-10" && i === 0)
    return (
      <div className="cs-center">
        {["Clean", "Edited", "Pending"].map((v, j) => (
          <div className="cs-toolbar" key={v}>
            <Btn disabled={j !== 1} onClick={() => change("Saved.")}>
              Save
            </Btn>
            <i>{v}</i>
          </div>
        ))}
        {value === "Saved." && <p>{value}</p>}
      </div>
    );
  if ((id === "DB-10" && i === 1) || (id === "DB-05" && i === 4))
    return (
      <div className="cs-center">
        {[1, 2, 3].map((x) => (
          <div key={x} className="cs-load-line" />
        ))}
        <p className="cs-notice">Couldn’t open deck.</p>
      </div>
    );
  if (id === "DB-03" && i === 1)
    return (
      <div className="cs-center">
        <Choice
          value={value}
          items={["Modern", "Standard", "Commander"]}
          change={change}
          open
        />
      </div>
    );
  if (id === "DB-03" && i === 2)
    return (
      <div className="cs-center">
        <p>Lightning Bolt</p>
        {quantity}
        <small>Enter a whole number.</small>
      </div>
    );
  return (
    <div className="cs-board-inspect">
      <div className="cs-board-illustration">
        <CardArt index={2} />
      </div>
      <div>
        <h3>
          {id === "DB-11" && i === 1
            ? "Search Red · Creature"
            : label.includes("Role")
              ? "Commander"
              : "Lightning Bolt"}
        </h3>
        {id === "DB-11" && i === 3 ? (
          <Btn onClick={() => setOpen(!open)}>Creatures ⌄</Btn>
        ) : (
          quantity
        )}
        {open && (
          <p>
            {id === "DB-10"
              ? "Can’t verify format eligibility."
              : id === "DB-05" || (id === "DB-11" && i === 2)
                ? "Not legal in Standard."
                : "Instant"}
          </p>
        )}
        {id === "DB-03" && (
          <>
            <Btn onClick={() => change("Main")}>Add to Main</Btn>
            <Btn onClick={() => change("Sideboard")}>Add to Sideboard</Btn>
          </>
        )}
      </div>
    </div>
  );
}

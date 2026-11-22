import { SectionRail } from "../SectionRail";
import { useLayoutEffect, useRef, useState } from "react";
import { ActionButton, EmptyState } from "@spellwake/ui";
import { Play, Plus, Upload } from "lucide-react";
import cardBack from "../../../../play/public/cards/magic-back.jpg?inline";
import { ReviewHeader } from "../ReviewHeader";
import {
  Board,
  Btn,
  CardArt,
  Choice,
  DeckArt,
  Example,
  PackArt,
  Search,
  cardNames,
  packNames,
  type Jump,
} from "./common";

import {
  decks,
  formats,
  longDeck,
  playDeck,
  editDeck,
  createDeck,
  openImport,
  type ShelfContext,
  rememberFormat,
  shelfFor,
  type DeckLaunch,
  type ShelfDeck,
} from "./deck-library";

export function DeckLibrary({
  id,
  go,
  launch,
}: {
  id: string;
  go: Jump;
  launch?:
    (ShelfContext & Partial<Pick<DeckLaunch, "deckId" | "target">>) | undefined;
}) {
  const origin = launch?.source === id ? launch : undefined;
  const [query, setQuery] = useState(
      origin?.query ?? (id === "DL-03" ? "Blue" : ""),
    ),
    [format, setFormat] = useState(
      origin?.formatFilter ?? (id === "DL-07" ? "Modern" : "All formats"),
    );
  const [createError, setCreateError] = useState(false);
  const navigating = useRef(false);
  const openPlay = id === "DL-05";
  const scroll = useRef<HTMLDivElement>(null);
  const importButton = useRef<HTMLButtonElement>(null);
  const deckId = origin?.deckId,
    scrollTop = origin?.scrollTop;
  useLayoutEffect(() => {
    if (!scroll.current || scrollTop === undefined) return;
    scroll.current.scrollTop = scrollTop ?? 0;
    if (!deckId) {
      importButton.current?.focus({ preventScroll: true });
      return;
    }
    const target = scroll.current.querySelector<HTMLButtonElement>(
      `[data-deck-id="${CSS.escape(deckId)}"] ${origin?.target ? ".cs-deck-open" : "button:last-child"}`,
    );
    (target ?? scroll.current).focus({ preventScroll: true });
  }, [deckId, scrollTop, origin?.target]);
  const context = () => ({
    source: id,
    query,
    formatFilter: format,
    scrollTop: scroll.current?.scrollTop ?? 0,
  });
  function create() {
    if (navigating.current) return;
    navigating.current = true;
    if (!createDeck(go, context())) {
      navigating.current = false;
      setCreateError(true);
    }
  }
  const cards = shelfFor(id).filter(
    (item) =>
      item.name.toLowerCase().includes(query.toLowerCase()) &&
      (format === "All formats" || item.format === format),
  );
  return (
    <>
      <ReviewHeader
        active={openPlay ? "Open Play" : "Collection"}
        go={go}
        coins={0}
      />
      <div className="cs-workspace">
        {!openPlay && <SectionRail area="Collection" current="DL-01" go={go} />}
        <main className={"cs-shelf-page " + (openPlay ? "open-play" : "")}>
          <div className="cs-toolbar">
            <Search label="Search decks" value={query} onChange={setQuery} />
            <Choice
              value={format}
              items={["All formats", ...formats]}
              change={(value) => {
                setFormat(value);
                const chosen = formats.find((f) => f === value);
                if (chosen) rememberFormat(id, chosen);
              }}
              open={id === "DL-02"}
            />
            <ActionButton
              ref={importButton}
              treatment="compact-form"
              onClick={() => openImport(go, context())}
            >
              <Upload /> Import deck
            </ActionButton>
            <Btn primary onClick={create}>
              <Plus /> Create deck
            </Btn>
          </div>
          {createError && (
            <p className="cs-toast" role="status">
              Couldn’t create deck.
            </p>
          )}
          <div className="cs-deck-scroll" ref={scroll} tabIndex={-1}>
            <div className="cs-deck-grid">
              {cards.map((item) => (
                <article
                  className="cs-deck"
                  key={item.id}
                  data-deck-id={item.id}
                >
                  <button
                    className="cs-deck-open"
                    aria-label={`Open ${item.name}`}
                    onClick={() =>
                      editDeck(go, { ...context(), deckId: item.id })
                    }
                  >
                    <DeckArt index={item.artIndex} name={item.name} />
                    <span>
                      {item.name}
                      <small>{item.format}</small>
                    </span>
                  </button>
                  <Btn
                    label={`Play ${item.name}`}
                    onClick={() =>
                      playDeck(go, { ...context(), deckId: item.id })
                    }
                  >
                    <Play /> Play
                  </Btn>
                </article>
              ))}
            </div>
            {!cards.length && (
              <EmptyState
                title="No matching decks"
                description="Try another name or format."
                action={
                  <Btn
                    onClick={() => {
                      setFormat("All formats");
                      setQuery("");
                    }}
                  >
                    Clear filters
                  </Btn>
                }
              />
            )}
          </div>
        </main>
      </div>
    </>
  );
}
const feedback = [
  "Loading more",
  "Load failure toast",
  "Long deck name",
  "Creating a deck",
  "Create failure toast",
  "Opening a lobby",
];
export function LibraryBoard({ id, go }: { id: string; go: Jump }) {
  const [error, setError] = useState(false);
  const busy = useRef(false);
  const context = {
    source: id,
    query: "",
    formatFilter: "All formats",
    scrollTop: 0,
  };
  const create = () => {
    if (busy.current) return;
    busy.current = true;
    if (!createDeck(go, context)) {
      busy.current = false;
      setError(true);
    }
  };
  const shelf = shelfFor(id);
  const previewDecks = (requested: ShelfDeck[]) =>
    requested.flatMap(
      (item) => shelf.find((deck) => deck.id === item.id) ?? [],
    );
  const list = id === "DL-04";
  const play = (deck: ShelfDeck) =>
    playDeck(go, { ...context, deckId: deck.id });
  return (
    <Board>
      {error && (
        <p className="cs-toast" role="status">
          Couldn’t create deck.
        </p>
      )}
      {(list
        ? ["Loading", "Empty", "No matches", "Load failed · toast"]
        : feedback
      ).map((label, i) => (
        <Example key={label} title={label}>
          <div className="cs-mini-library">
            {(list || i === 3 || i === 4) && (
              <div className="cs-toolbar">
                <Search
                  label="Search decks"
                  value={i === 2 && list ? "zzzz" : ""}
                  onChange={() => go("DL-03")}
                />
                <Choice
                  value="All formats"
                  items={["All formats", ...formats]}
                  change={() => go("DL-07")}
                />
                <Btn onClick={() => openImport(go, context)}>Import deck</Btn>
                <Btn onClick={create}>Create deck</Btn>
              </div>
            )}
            {list ? (
              <div className="cs-center">
                {i === 0 ? (
                  <p>◌ Loading your decks…</p>
                ) : i === 1 ? (
                  <EmptyState
                    title="No decks yet"
                    description="Create or import a deck."
                    action={<Btn onClick={create}>Create deck</Btn>}
                  />
                ) : i === 2 ? (
                  <EmptyState
                    title="No matching decks"
                    description="Try another name or format."
                    action={
                      <Btn onClick={() => go("DL-01")}>Clear filters</Btn>
                    }
                  />
                ) : (
                  <span className="cs-toast">Couldn’t load your decks.</span>
                )}
              </div>
            ) : (
              <>
                {i < 3 && (
                  <div className="cs-mini-decks">
                    {previewDecks(
                      i === 2 ? [decks[3]!, longDeck] : decks.slice(0, 3),
                    ).map((deck) => (
                      <div key={deck.id}>
                        <button
                          aria-label={`Open ${deck.name}`}
                          onClick={() =>
                            editDeck(go, { ...context, deckId: deck.id })
                          }
                        >
                          <DeckArt index={deck.artIndex} name={deck.name} />
                          <span className="cs-mini-deck-name">{deck.name}</span>
                          <small>{deck.format}</small>
                        </button>
                        <Btn
                          label={`Play ${deck.name}`}
                          onClick={() => play(deck)}
                        >
                          ▸ Play
                        </Btn>
                      </div>
                    ))}
                  </div>
                )}
                {i === 5 && (
                  <div className="cs-mini-lobby-links">
                    {["Opening lobby…", "Couldn’t open lobby."].map((text) => (
                      <div key={text}>
                        <DeckArt index={0} />
                        <span>
                          {shelf[0]?.name}
                          <small>{shelf[0]?.format}</small>
                        </span>
                        <Btn
                          label={`Play ${shelf[0]?.name}`}
                          onClick={() => play(decks[0]!)}
                        >
                          ▸ Play
                        </Btn>
                        <span className="cs-notice">{text}</span>
                      </div>
                    ))}
                  </div>
                )}
                {i !== 2 && i !== 5 && (
                  <span className="cs-toast">
                    {
                      [
                        "Loading more decks…",
                        "Couldn’t load more decks.",
                        "",
                        "Creating deck…",
                        "Couldn’t create deck.",
                        "Opening lobby…",
                      ][i]
                    }
                  </span>
                )}
              </>
            )}
          </div>
        </Example>
      ))}
    </Board>
  );
}
const packSetCodes: Record<string, string> = {
  Foundations: "FDN",
  "Modern Horizons 3": "MH3",
  Bloomburrow: "BLB",
  Duskmourn: "DSK",
};

export function PackLibrary({
  id,
  go,
  mini,
  initialSearch = "",
  initialSet = "All sets",
  menuOpen = false,
}: {
  id: string;
  go: Jump;
  mini?: boolean;
  initialSearch?: string;
  initialSet?: string;
  menuOpen?: boolean;
}) {
  const [query, setQuery] = useState(initialSearch),
    [set, setSet] = useState(initialSet);
  const toolbar = useRef<HTMLDivElement>(null);
  const searchTerm = query.trim().toLowerCase();
  const matching = packNames
    .map((name, i) => ({ name, i }))
    .filter(({ name }) => {
      const product = `${name} Play Booster`.toLowerCase();
      const code = (packSetCodes[name] ?? "").toLowerCase();
      return (
        (product.includes(searchTerm) || code.includes(searchTerm)) &&
        (set === "All sets" || set === name)
      );
    });
  return (
    <>
      {!mini && <ReviewHeader active="Collection" go={go} coins={0} />}
      <div className="cs-workspace">
        <SectionRail area="Collection" current="CP-01" go={go} />
        <div className="cs-shelf-page">
          <div className="cs-toolbar" ref={toolbar}>
            <Search label="Search packs" value={query} onChange={setQuery} />
            <Choice
              value={set}
              items={["All sets", ...packNames]}
              change={setSet}
              open={menuOpen}
            />
          </div>
          {id === "CP-05" || !matching.length ? (
            <div className="cs-center">
              <EmptyState
                treatment="panel"
                title={id === "CP-05" ? "No packs yet" : "No matching packs"}
                description={
                  id === "CP-05"
                    ? "Your packs will appear here."
                    : "Try another name or set."
                }
                action={
                  id !== "CP-05" && (
                    <Btn
                      onClick={() => {
                        setQuery("");
                        setSet("All sets");
                        toolbar.current?.querySelector("input")?.focus();
                      }}
                    >
                      Clear filters
                    </Btn>
                  )
                }
              />
            </div>
          ) : (
            <div className="cs-pack-grid">
              {matching.map(({ name, i }) => (
                <article key={name}>
                  <PackArt index={i} />
                  <h3>{name}</h3>
                  <small>
                    {[4, 2, 3, 1][i]} {i === 3 ? "pack" : "packs"}
                  </small>
                  <Btn onClick={() => go("CP-11")}>
                    <Play /> Open pack
                  </Btn>
                </article>
              ))}
            </div>
          )}
        </div>
      </div>
    </>
  );
}
export function PackOpening({ id, go }: { id: string; go: Jump }) {
  const [revealed, setRevealed] = useState<number[]>(
    id === "CP-13"
      ? Array.from({ length: 14 }, (_, i) => i)
      : id === "CP-12"
        ? [0, 1, 2]
        : [],
  );
  return (
    <div className="cs-opening">
      <button
        className="cs-close-corner"
        aria-label="Close opening"
        onClick={() => go("CP-01")}
      >
        ×
      </button>
      <h1>Foundations Play Booster</h1>
      <div className="cs-reveal-grid">
        {Array.from({ length: 14 }, (_, i) => {
          const faceUp = revealed.includes(i);
          const artIndex = 3 - (i % 4);
          return (
            <button
              key={i}
              aria-label={
                faceUp
                  ? `Card ${i + 1}: ${cardNames[artIndex]}`
                  : `Reveal card ${i + 1}`
              }
              aria-disabled={faceUp || undefined}
              onClick={
                faceUp
                  ? undefined
                  : () =>
                      setRevealed((prev) =>
                        prev.includes(i) ? prev : [...prev, i],
                      )
              }
            >
              {faceUp ? (
                <CardArt index={artIndex} />
              ) : (
                <img src={cardBack} alt="Card back" />
              )}
            </button>
          );
        })}
      </div>
      <footer>
        {revealed.length === 14 ? (
          <>
            <Btn onClick={() => go("CP-01")}>Done</Btn>
            <Btn onClick={() => go("CP-11")}>Open another</Btn>
          </>
        ) : (
          <Btn
            onClick={() => setRevealed(Array.from({ length: 14 }, (_, i) => i))}
          >
            Reveal all
          </Btn>
        )}
      </footer>
    </div>
  );
}
export function PackBoard({ id, go }: { id: string; go: Jump }) {
  const labels =
    id === "CP-02"
      ? ["Set menu", "Matching results", "No matches"]
      : id === "CP-09"
        ? ["Opening", "Checking opening", "Loading accepted contents"]
        : ["Loading", "Availability", "Toast feedback", "Last pack complete"];
  return (
    <Board columns={id === "CP-06" ? 2 : 1}>
      {labels.map((label, i) => (
        <Example
          key={label}
          title={label}
          width={id === "CP-02" ? 1460 : 850}
          height={id === "CP-02" ? 820 : 330}
        >
          {id === "CP-02" ? (
            <PackLibrary
              id="CP-01"
              go={go}
              mini
              menuOpen={i === 0}
              initialSearch={i === 2 ? "zzzz" : ""}
              initialSet={i === 1 ? "Bloomburrow" : "All sets"}
            />
          ) : id === "CP-09" ? (
            <div className="cs-center">
              <h2>Foundations Play Booster</h2>
              <p>◌</p>
              <p>
                {
                  [
                    "Opening pack…",
                    "Still checking this opening…",
                    "Loading your cards…",
                  ][i]
                }
              </p>
              {i === 1 && <Btn onClick={() => go("CP-11")}>Check opening</Btn>}
            </div>
          ) : i === 3 ? (
            <div className="cs-mini-decks">
              {[1, 2, 0, 3].map((x) => (
                <CardArt key={x} index={x} />
              ))}
              <Btn onClick={() => go("CP-05")}>Done</Btn>
            </div>
          ) : i === 2 ? (
            <div className="cs-center">
              {[
                "Couldn’t load packs.",
                "This pack is no longer available.",
                "Pack opening is unavailable.",
              ].map((x) => (
                <p className="cs-notice" key={x}>
                  {x}
                </p>
              ))}
            </div>
          ) : (
            <div className="cs-mini-decks">
              {[0, 2].map((j) => (
                <div key={j}>
                  {i === 0 ? (
                    <div className="cs-skeleton" />
                  ) : (
                    <PackArt index={j} />
                  )}
                  <p>{packNames[j]}</p>
                  <Btn
                    disabled={i === 0 || j === 2}
                    onClick={() => go("CP-11")}
                  >
                    Open pack
                  </Btn>
                </div>
              ))}
            </div>
          )}
        </Example>
      ))}
    </Board>
  );
}

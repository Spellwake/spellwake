import { useEffect, useState, type ReactElement } from "react";
import { ArrowLeft, Image, Play, Plus, Upload } from "lucide-react";
import {
  Button,
  DialogFooter,
  FieldStack,
  Modal,
  SearchInput,
  Select,
  SideNavigation,
  TextArea,
  TextInput,
  TopNavigation,
  type SelectGroup,
} from "@spellwake/ui";
import "../workshop.css";
import * as s from "./collection.css";

type BoxArt = { crop: string; polygons: string[] };
type Deck = { id: string; name: string; format: string; art: number };
const art: BoxArt[] = [
  {
    crop: "270 190 430 290",
    polygons: ["294,316 425,211 665,246 669,315 649,397 566,456 297,400"],
  },
  {
    crop: "690 190 430 290",
    polygons: ["714,312 847,207 1090,244 1092,355 1057,466 716,407"],
  },
  {
    crop: "1110 190 430 290",
    polygons: [
      "1134,317 1271,209 1492,254 1502,314 1472,417 1383,467 1138,405",
    ],
  },
  {
    crop: "270 500 430 320",
    polygons: [
      "250,595 383,508 600,549 562,655",
      "267,680 326,645 565,670 600,550 653,657 647,731 570,812 266,750",
    ],
  },
  {
    crop: "690 530 430 300",
    polygons: ["712,655 845,541 1090,578 1093,752 1007,830 716,739"],
  },
  {
    crop: "1110 530 430 300",
    polygons: ["1157,655 1291,543 1562,583 1563,745 1483,829 1160,734"],
  },
];
const initialDecks: Deck[] = [
  { id: "goblins", name: "Goblins", format: "Standard", art: 0 },
  { id: "blue-control", name: "Blue Control", format: "Modern", art: 1 },
  { id: "elves", name: "Elves", format: "Commander", art: 2 },
  { id: "angels", name: "Angels", format: "Standard", art: 3 },
  { id: "graveyard", name: "Graveyard", format: "Modern", art: 4 },
  { id: "dragons", name: "Dragons", format: "Commander", art: 5 },
];
const formats: SelectGroup[] = [
  { options: [{ value: "all", label: "All formats" }] },
  {
    label: "Constructed",
    options: [
      { value: "Standard", label: "Standard" },
      { value: "Modern", label: "Modern" },
    ],
  },
  { label: "Singleton", options: [{ value: "Commander", label: "Commander" }] },
];
const base = import.meta.env.BASE_URL;

/** Reference artwork only. Text and controls are separate, live UI elements. */
function ReferenceBox({
  index,
  id,
}: {
  index: number;
  id: string;
}): ReactElement {
  const source = art[index] ?? art[0];
  if (!source) return <></>;
  const clipId = `box-${id}`;
  return (
    <svg
      className={s.boxArt}
      viewBox={source.crop}
      aria-hidden="true"
      focusable="false"
    >
      <defs>
        <clipPath id={clipId}>
          {source.polygons.map((points, i) => (
            <polygon key={i} points={points} />
          ))}
        </clipPath>
      </defs>
      <image
        clipPath={`url(#${clipId})`}
        href={`${base}reference/hybrid.png`}
        width="1672"
        height="941"
      />
    </svg>
  );
}

export function Collection(): ReactElement {
  useEffect(() => {
    document.title = "Collection · Spellwake";
  }, []);
  const [decks, setDecks] = useState(initialDecks);
  const [query, setQuery] = useState("");
  const [format, setFormat] = useState("all");
  const [createOpen, setCreateOpen] = useState(false);
  const [importOpen, setImportOpen] = useState(false);
  const [name, setName] = useState("");
  const [newFormat, setNewFormat] = useState("Standard");
  const [error, setError] = useState("");
  const [listError, setListError] = useState("");
  const [deckText, setDeckText] = useState("");
  const [selected, setSelected] = useState<Deck | null>(null);
  const [notice, setNotice] = useState("");
  const [reference, setReference] = useState(false);
  const filtered = decks.filter(
    (deck) =>
      deck.name.toLowerCase().includes(query.trim().toLowerCase()) &&
      (format === "all" || deck.format === format),
  );
  function addDeck(importing: boolean): void {
    if (!name.trim()) {
      setError("Enter a deck name.");
      return;
    }
    setError("");
    if (importing && !deckText.trim()) {
      setListError("Paste a deck list to import.");
      return;
    }
    setDecks((current) => [
      ...current,
      { id: crypto.randomUUID(), name: name.trim(), format: newFormat, art: 0 },
    ]);
    setQuery("");
    setFormat("all");
    setCreateOpen(false);
    setImportOpen(false);
    setName("");
    setDeckText("");
    setError("");
  }
  function openCreate(importing: boolean): void {
    setError("");
    setListError("");
    setName("");
    setDeckText("");
    setNewFormat("Standard");
    if (importing) setImportOpen(true);
    else setCreateOpen(true);
  }
  const form = (importing: boolean): ReactElement => (
    <form
      onSubmit={(event) => {
        event.preventDefault();
        addDeck(importing);
      }}
    >
      <FieldStack>
        <TextInput
          label="Deck name"
          value={name}
          onChange={(event) => setName(event.target.value)}
          placeholder="For example, Goblins"
          {...(error ? { error } : {})}
        />
        <div>
          <label className={s.fieldLabel}>Format</label>
          <Select
            label="New deck format"
            value={newFormat}
            onValueChange={setNewFormat}
            groups={formats.slice(1)}
          />
        </div>
        {importing && (
          <TextArea
            label="Deck list"
            value={deckText}
            onChange={(event) => {
              setDeckText(event.target.value);
              setListError("");
            }}
            {...(listError ? { error: listError } : {})}
            placeholder="4 Lightning Bolt"
            help="This preview stores the deck name and format for this session. Card validation will be connected with the deck builder."
          />
        )}
      </FieldStack>
      <DialogFooter>
        <Button
          variant="transparent"
          onClick={() => {
            setCreateOpen(false);
            setImportOpen(false);
          }}
        >
          Cancel
        </Button>
        <Button type="submit" variant="primary">
          <Plus size={19} />
          {importing ? "Import deck" : "Create deck"}
        </Button>
      </DialogFooter>
    </form>
  );
  return (
    <div className={s.scene}>
      {reference ? (
        <div className={s.referenceFrame}>
          <img
            className={s.reference}
            src={`${base}reference/hybrid.png`}
            alt="Approved hybrid reference image"
          />
        </div>
      ) : (
        <>
          <TopNavigation
            logoUrl={`${base}logo.svg`}
            activeId="collection"
            coins={1300}
            onNavigate={(id) => {
              if (id !== "collection")
                setNotice(
                  `${id === "play" ? "Open Play" : id === "market" ? "Market" : "League"} is outside this Collection screen preview.`,
                );
            }}
            onAccountAction={(action) =>
              setNotice(`${action} is not connected in this visual preview.`)
            }
            onNotifications={() =>
              setNotice("You’re all caught up. This is a local UI preview.")
            }
          />
          <div className={s.shell}>
            <SideNavigation
              activeId="decks"
              onNavigate={(id) => {
                if (id !== "decks")
                  setNotice(
                    `${id === "cards" ? "Cards" : "Packs"} will use this same shell; this preview reconstructs Decks.`,
                  );
              }}
            />
            <main className={s.main} aria-label="Your decks">
              <h1 className={s.srOnly}>Your decks</h1>
              <div className={s.toolbar}>
                <SearchInput value={query} onValueChange={setQuery} />
                <Select
                  label="Filter decks by format"
                  value={format}
                  onValueChange={setFormat}
                  groups={formats}
                />
                <div className={s.toolbarActions}>
                  <Button onClick={() => openCreate(true)}>
                    <Upload size={21} />
                    Import deck
                  </Button>
                  <Button variant="primary" onClick={() => openCreate(false)}>
                    <Plus size={23} />
                    Create deck
                  </Button>
                </div>
              </div>
              <div className={s.grid}>
                {filtered.map((deck) => (
                  <article className={s.deck} key={deck.id}>
                    <button
                      className={s.boxButton}
                      aria-label={`Inspect ${deck.name}`}
                      onClick={() => setSelected(deck)}
                    >
                      <ReferenceBox index={deck.art} id={deck.id} />
                    </button>
                    <div className={s.deckInfo}>
                      <div className={s.deckTitle}>
                        <h2>{deck.name}</h2>
                        <p>{deck.format}</p>
                      </div>
                      <Button
                        variant="secondary"
                        size="sm"
                        pill
                        onClick={() => setSelected(deck)}
                      >
                        <Play size={15} fill="currentColor" />
                        Play
                      </Button>
                    </div>
                  </article>
                ))}
              </div>
              {filtered.length === 0 && (
                <div className={s.empty}>
                  <h2>No decks found</h2>
                  <p>Try a different name or format.</p>
                  <Button
                    onClick={() => {
                      setQuery("");
                      setFormat("all");
                    }}
                  >
                    Clear filters
                  </Button>
                </div>
              )}
            </main>
          </div>
        </>
      )}
      <div className={s.studyTools}>
        <a href={`${base}`}>
          <ArrowLeft size={14} />
          Components
        </a>
        <Button
          size="sm"
          variant="ghost"
          onClick={() => setReference(!reference)}
        >
          <Image size={15} />
          {reference ? "Show live UI" : "Compare reference"}
        </Button>
      </div>
      <Modal
        title="Create a deck"
        description="Give your deck a name and choose its format."
        open={createOpen}
        onOpenChange={setCreateOpen}
      >
        {form(false)}
      </Modal>
      <Modal
        title="Import a deck"
        description="Start a deck from a text list."
        open={importOpen}
        onOpenChange={setImportOpen}
      >
        {form(true)}
      </Modal>
      <Modal
        title={selected?.name ?? "Deck"}
        description={`${selected?.format ?? ""} · Collection preview`}
        open={selected !== null}
        onOpenChange={(open) => {
          if (!open) setSelected(null);
        }}
      >
        <p>
          This screen demonstrates the Collection UI. Match launch and deck
          editing are not connected yet.
        </p>
        <DialogFooter>
          <Button variant="primary" onClick={() => setSelected(null)}>
            Back to collection
          </Button>
        </DialogFooter>
      </Modal>
      <Modal
        title="Preview"
        description={notice}
        open={notice !== ""}
        onOpenChange={(open) => {
          if (!open) setNotice("");
        }}
      >
        <DialogFooter>
          <Button onClick={() => setNotice("")}>Close</Button>
        </DialogFooter>
      </Modal>
    </div>
  );
}

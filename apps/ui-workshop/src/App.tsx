import { useEffect, useState, type ReactElement, type ReactNode } from "react";
import {
  ArrowRight,
  Bell,
  Check,
  ChevronDown,
  Copy,
  Download,
  ExternalLink,
  Grid2X2,
  Info,
  List,
  MoreHorizontal,
  Play,
  Plus,
  Search,
  Settings,
  Trash2,
  Upload,
  X,
} from "lucide-react";
import {
  Accordion,
  Alert,
  Badge,
  Button,
  Checkbox,
  DialogFooter,
  FieldStack,
  IconButton,
  Menu,
  Modal,
  Quantity,
  RadioOptions,
  SearchInput,
  Select,
  SideNavigation,
  Slider,
  Switch,
  TabGroup,
  TextArea,
  TextInput,
  Tooltip,
  TopNavigation,
  vars,
  type ButtonVariant,
  type SelectGroup,
} from "@spellwake/ui";
import * as w from "./workshop.css";

const formats: SelectGroup[] = [
  { label: "Browse", options: [{ value: "all", label: "All formats" }] },
  {
    label: "Constructed",
    options: [
      { value: "standard", label: "Standard" },
      { value: "modern", label: "Modern" },
      { value: "legacy", label: "Legacy" },
    ],
  },
  {
    label: "Singleton",
    options: [
      { value: "commander", label: "Commander" },
      { value: "brawl", label: "Brawl", disabled: true },
    ],
  },
];
const decks = [
  { name: "Goblins", format: "standard" },
  { name: "Blue Control", format: "modern" },
  { name: "Elves", format: "commander" },
  { name: "Angels", format: "standard" },
];
const variants: ButtonVariant[] = [
  "primary",
  "secondary",
  "ghost",
  "transparent",
  "light",
  "text",
  "danger",
];

function Section({
  id,
  title,
  description,
  children,
}: {
  id: string;
  title: string;
  description: string;
  children: ReactNode;
}): ReactElement {
  return (
    <section id={id} className={w.section}>
      <div className={w.sectionHead}>
        <div>
          <h2>{title}</h2>
          <p>{description}</p>
        </div>
        <a href={`#${id}`} aria-label={`Link to ${title}`}>
          #
        </a>
      </div>
      {children}
    </section>
  );
}

export function App(): ReactElement {
  const [main, setMain] = useState("collection");
  const [side, setSide] = useState("decks");
  const [query, setQuery] = useState("");
  const [format, setFormat] = useState("all");
  const [checked, setChecked] = useState(true);
  const [switched, setSwitched] = useState(false);
  const [radio, setRadio] = useState("private");
  const [view, setView] = useState("list");
  const [quantity, setQuantity] = useState(4);
  const [removed, setRemoved] = useState(false);
  const [volume, setVolume] = useState(35);
  const [font, setFont] = useState("Lora");
  const [message, setMessage] = useState("");
  const [loading, setLoading] = useState(false);
  const [createOpen, setCreateOpen] = useState(false);
  const [deckName, setDeckName] = useState("");
  const [createFormat, setCreateFormat] = useState("modern");
  const [submitted, setSubmitted] = useState(false);
  const [created, setCreated] = useState<{ name: string; format: string }[]>(
    [],
  );
  useEffect(() => {
    const property = vars.font.display.slice(4, -1);
    document.documentElement.style.setProperty(
      property,
      `"${font}", Georgia, serif`,
    );
  }, [font]);
  useEffect(() => {
    if (!message) return;
    const timer = window.setTimeout(() => setMessage(""), 5000);
    return () => window.clearTimeout(timer);
  }, [message]);
  const filtered = [...decks, ...created].filter(
    (deck) =>
      deck.name.toLowerCase().includes(query.toLowerCase()) &&
      (format === "all" || deck.format === format),
  );
  function notify(text: string): void {
    setMessage(text);
  }
  function createDeck(event: React.FormEvent<HTMLFormElement>): void {
    event.preventDefault();
    setSubmitted(true);
    if (!deckName.trim()) return;
    setCreated((current) => [
      ...current,
      { name: deckName.trim(), format: createFormat },
    ]);
    setCreateOpen(false);
    setSubmitted(false);
    setDeckName("");
    notify("Deck added to this workshop session.");
  }
  return (
    <>
      <TopNavigation
        logoUrl="./logo.svg"
        activeId={main}
        coins={1300}
        onNavigate={(id) => {
          setMain(id);
          notify(`Navigation example: ${id === "play" ? "Open Play" : id}.`);
        }}
        onAccountAction={(action) =>
          notify(`${action} selected in the account-menu example.`)
        }
        onNotifications={() =>
          notify("You’re all caught up. This is sample notification feedback.")
        }
      />
      <div className={w.sampleBody}>
        <SideNavigation
          activeId={side}
          onNavigate={(id) => {
            setSide(id);
            notify(`Collection navigation: ${id}.`);
          }}
        />
        <main className={w.page} style={{ flex: 1, minWidth: 0 }}>
          <div className={w.intro}>
            <div>
              <h1>Component library</h1>
              <p>
                A fresh UI foundation built toward the approved hybrid. Real
                controls, shown over artwork. The original stays close so we can
                catch drift early.
              </p>
            </div>
            <div className={w.inline}>
              <Modal
                title="Our visual reference"
                description="Moonlit Atelier content and controls, with Astral Vault top and side navigation."
                trigger={
                  <Button variant="ghost">
                    <ExternalLink />
                    View reference
                  </Button>
                }
              >
                <div className={w.referenceDialog}>
                  <a
                    href="./reference/hybrid.png"
                    target="_blank"
                    rel="noreferrer"
                  >
                    <img
                      src="./reference/hybrid.png"
                      className={w.reference}
                      alt="Approved hybrid reference with dimensional deck boxes and dark navigation"
                    />
                  </a>
                </div>
              </Modal>
              <Button variant="primary" onClick={() => setCreateOpen(true)}>
                <Plus />
                Create deck
              </Button>
            </div>
          </div>
          <nav className={w.inline} aria-label="Workshop sections">
            {[
              ["buttons", "Buttons"],
              ["fields", "Fields & menus"],
              ["selection", "Selection"],
              ["overlays", "Overlays"],
              ["type", "Typography"],
              ["reference", "Reference"],
            ].map(([id, label]) => (
              <a
                key={id}
                href={`#${id}`}
                style={{ padding: "8px 12px", fontSize: 13 }}
              >
                {label}
              </a>
            ))}
          </nav>

          <Section
            id="buttons"
            title="Buttons and action states"
            description="Primary actions carry a restrained brass edge. Secondary actions stay quieter. Hover, press, keyboard focus and selection remain separate states."
          >
            <div className={w.horizontalScroll}>
              <table className={w.matrix}>
                <caption
                  className={w.exampleLabel}
                  style={{
                    textAlign: "left",
                    display: "table-caption",
                    captionSide: "top",
                    paddingBottom: 16,
                  }}
                >
                  State samples are pinned for comparison; every button also
                  responds to real pointer and keyboard input.
                </caption>
                <thead>
                  <tr>
                    <th scope="col">Variant</th>
                    <th scope="col">Rest</th>
                    <th scope="col">Hover</th>
                    <th scope="col">Pressed</th>
                    <th scope="col">Focus</th>
                    <th scope="col">Disabled</th>
                  </tr>
                </thead>
                <tbody>
                  {variants.map((variant) => (
                    <tr key={variant}>
                      <td>
                        {variant.charAt(0).toUpperCase() + variant.slice(1)}
                      </td>
                      <td>
                        <Button
                          variant={variant}
                          size="sm"
                          onClick={() => notify(`${variant} button activated.`)}
                        >
                          Continue
                        </Button>
                      </td>
                      <td>
                        <Button
                          variant={variant}
                          size="sm"
                          previewState="hover"
                        >
                          Continue
                        </Button>
                      </td>
                      <td>
                        <Button
                          variant={variant}
                          size="sm"
                          previewState="pressed"
                        >
                          Continue
                        </Button>
                      </td>
                      <td>
                        <Button
                          variant={variant}
                          size="sm"
                          previewState="focus"
                        >
                          Continue
                        </Button>
                      </td>
                      <td>
                        <Button variant={variant} size="sm" disabled>
                          Continue
                        </Button>
                      </td>
                    </tr>
                  ))}
                </tbody>
              </table>
            </div>
            <div className={w.two} style={{ marginTop: 28 }}>
              <div className={w.group}>
                <h3>Composition, not another variant</h3>
                <div className={w.inline}>
                  <Button variant="primary" size="sm">
                    <Plus />
                    Create
                  </Button>
                  <Button variant="primary">
                    <Plus />
                    Create deck
                  </Button>
                  <Button variant="secondary" pill>
                    <Play fill="currentColor" size={16} />
                    Play
                  </Button>
                  <Tooltip label="More deck actions">
                    <IconButton label="More deck actions" variant="ghost">
                      <MoreHorizontal />
                    </IconButton>
                  </Tooltip>
                </div>
                <p style={{ marginTop: 16 }}>
                  Size and shape are independent of the visual role. Play uses
                  the secondary finish with a pill shape.
                </p>
              </div>
              <div className={w.group}>
                <h3>Progress and selection</h3>
                <div className={w.inline}>
                  <Button
                    variant="primary"
                    loading={loading}
                    onClick={() => {
                      setLoading(true);
                      window.setTimeout(() => {
                        setLoading(false);
                        notify("Demo save completed.");
                      }, 1200);
                    }}
                  >
                    {loading ? "Saving…" : "Save deck"}
                  </Button>
                  <Button variant="secondary" loading>
                    Saving…
                  </Button>
                  <Button
                    variant="ghost"
                    selected={checked}
                    aria-pressed={checked}
                    onClick={() => setChecked(!checked)}
                  >
                    Selected option
                  </Button>
                </div>
                <p style={{ marginTop: 16 }}>
                  Loading prevents repeat activation. A selected option has a
                  persistent state, independent of its hover.
                </p>
              </div>
            </div>
          </Section>

          <Section
            id="fields"
            title="Fields and menus"
            description="Flat smoked surfaces, restrained placeholders, aligned icons and a soft gold focus. Menus support labels, dividers and disabled items."
          >
            <div className={w.two}>
              <div className={w.group}>
                <FieldStack>
                  <TextInput
                    label="Deck name"
                    placeholder="Give your deck a name"
                    help="Used in your library and when choosing a deck."
                  />
                  <TextInput
                    label="With an entered value"
                    defaultValue="Goblins"
                  />
                  <TextInput
                    label="Validation example"
                    defaultValue=""
                    placeholder="Deck name"
                    error="Enter a deck name before saving."
                    required
                  />
                  <TextInput
                    label="Disabled field"
                    placeholder="Unavailable"
                    disabled
                  />
                  <TextArea
                    label="Deck notes"
                    placeholder="A plan for the opening turns…"
                    help="Optional notes for this deck."
                  />
                </FieldStack>
              </div>
              <div className={w.group}>
                <FieldStack>
                  <div>
                    <span className={w.exampleLabel}>
                      Search, including a fully transparent clear action
                    </span>
                    <SearchInput value={query} onValueChange={setQuery} />
                  </div>
                  <div>
                    <span className={w.exampleLabel}>
                      Grouped format selector
                    </span>
                    <Select
                      label="Filter decks by format"
                      value={format}
                      onValueChange={setFormat}
                      groups={formats}
                    />
                  </div>
                  <div>
                    <span className={w.exampleLabel}>Action menu</span>
                    <Menu
                      label="Deck actions"
                      align="start"
                      trigger={
                        <Button>
                          <MoreHorizontal />
                          Deck actions
                          <ChevronDown />
                        </Button>
                      }
                      entries={[
                        { kind: "label", text: "This deck" },
                        {
                          kind: "item",
                          text: "Rename",
                          icon: <Settings size={17} />,
                          onSelect: () =>
                            notify("Rename selected in the menu example."),
                        },
                        {
                          kind: "item",
                          text: "Duplicate",
                          icon: <Copy size={17} />,
                          onSelect: () =>
                            notify("Duplicate selected in the menu example."),
                        },
                        { kind: "divider", id: "export" },
                        { kind: "label", text: "Share & export" },
                        {
                          kind: "item",
                          text: "Export deck",
                          icon: <Download size={17} />,
                          onSelect: () =>
                            notify("Export selected in the menu example."),
                        },
                        { kind: "item", text: "Publish deck", disabled: true },
                        { kind: "divider", id: "remove" },
                        {
                          kind: "item",
                          text: "Remove deck",
                          icon: <Trash2 size={17} />,
                          onSelect: () =>
                            notify(
                              "Remove selected. No saved deck was deleted.",
                            ),
                        },
                      ]}
                    />
                  </div>
                  <div>
                    <span className={w.exampleLabel}>
                      Quantity: inline edit without moving the surrounding
                      layout
                    </span>
                    {removed ? (
                      <Button
                        variant="text"
                        onClick={() => {
                          setRemoved(false);
                          setQuantity(1);
                        }}
                      >
                        Restore quantity example
                      </Button>
                    ) : (
                      <Quantity
                        value={quantity}
                        onValueChange={setQuantity}
                        onRemove={() => setRemoved(true)}
                      />
                    )}
                  </div>
                  <Alert>
                    <Info size={17} style={{ marginRight: 8 }} />
                    Try the search and format selector together in the
                    composition below.
                  </Alert>
                </FieldStack>
              </div>
            </div>
          </Section>

          <Section
            id="selection"
            title="Choices and disclosure"
            description="Shared selection treatment across tabs, toggles and grouped choices. The accordion chevron turns from right to down as the content opens."
          >
            <div className={w.two}>
              <div className={w.group}>
                <FieldStack>
                  <div>
                    <span className={w.exampleLabel}>View selection</span>
                    <TabGroup
                      label="Deck view"
                      value={view}
                      onValueChange={setView}
                      items={[
                        {
                          value: "list",
                          label: (
                            <>
                              <List size={16} /> List
                            </>
                          ),
                        },
                        {
                          value: "cards",
                          label: (
                            <>
                              <Grid2X2 size={16} /> Cards
                            </>
                          ),
                        },
                        { value: "inspector", label: "Inspector" },
                      ]}
                    />
                  </div>
                  <Checkbox
                    label="Show card names"
                    checked={checked}
                    onCheckedChange={setChecked}
                  />
                  <Checkbox
                    label="Unavailable option"
                    checked={false}
                    onCheckedChange={() => undefined}
                    disabled
                  />
                  <Switch
                    label="Sound effects"
                    checked={switched}
                    onCheckedChange={setSwitched}
                  />
                  <div>
                    <span className={w.exampleLabel}>Deck visibility</span>
                    <RadioOptions
                      label="Deck visibility"
                      value={radio}
                      onValueChange={setRadio}
                      options={[
                        { value: "private", label: "Private" },
                        { value: "unlisted", label: "Unlisted" },
                        { value: "public", label: "Public" },
                      ]}
                    />
                  </div>
                  <div>
                    <span className={w.exampleLabel}>Volume · {volume}%</span>
                    <Slider
                      label="Volume"
                      value={volume}
                      onValueChange={setVolume}
                    />
                  </div>
                </FieldStack>
              </div>
              <div className={w.group}>
                <Accordion
                  items={[
                    {
                      id: "creatures",
                      title: "Creatures · 24",
                      children: (
                        <div className={w.inline}>
                          <span>Goblin Warchief</span>
                          <Badge>3 copies</Badge>
                        </div>
                      ),
                    },
                    {
                      id: "lands",
                      title: "Lands · 22",
                      children: (
                        <p>
                          Collapsed groups keep their children outside the
                          keyboard tab order. Sections open independently.
                        </p>
                      ),
                    },
                    {
                      id: "preferences",
                      title: "Preferences",
                      children: (
                        <Switch
                          label="Show artwork previews"
                          checked={switched}
                          onCheckedChange={setSwitched}
                        />
                      ),
                    },
                  ]}
                />
                <div className={w.inline} style={{ marginTop: 24 }}>
                  <Badge>
                    <Check size={13} />
                    Valid
                  </Badge>
                  <Badge tone="danger">3 issues</Badge>
                  <Badge>Modern</Badge>
                  <Badge>Draft</Badge>
                </div>
              </div>
            </div>
          </Section>

          <Section
            id="overlays"
            title="Feedback and focused tasks"
            description="Consistent menus, tooltips and dialogs. Keyboard focus returns to the trigger; Escape, the close button and outside clicks dismiss dialogs."
          >
            <div className={w.two}>
              <div className={w.group}>
                <h3>Create a deck</h3>
                <p>
                  A working form example using the same fields, select and
                  action variants. New decks exist only in this workshop
                  session.
                </p>
                <div className={w.inline}>
                  <Button variant="primary" onClick={() => setCreateOpen(true)}>
                    <Plus />
                    Create deck
                  </Button>
                  <Tooltip label="Tooltips clarify small actions without adding another surface.">
                    <IconButton label="About tooltips">
                      <Info />
                    </IconButton>
                  </Tooltip>
                  <Button
                    variant="ghost"
                    onClick={() =>
                      notify(
                        "Your changes have been saved. Sample feedback only.",
                      )
                    }
                  >
                    <Bell />
                    Show feedback
                  </Button>
                </div>
              </div>
              <div className={w.group}>
                <h3>Inline recovery</h3>
                <Alert role="alert">
                  The deck couldn’t be loaded. Your changes are still here.
                  <div style={{ marginTop: 12 }}>
                    <Button
                      variant="secondary"
                      size="sm"
                      onClick={() =>
                        notify("Retry activated in the error-state example.")
                      }
                    >
                      Try again
                      <ArrowRight size={15} />
                    </Button>
                  </div>
                </Alert>
              </div>
            </div>
          </Section>

          <Section
            id="composition"
            title="Built from the same pieces"
            description="A small library composition proves the controls work together. The deck-box renderer stays separate from these primitives."
          >
            <div className={w.surface}>
              <div className={w.toolbar}>
                <SearchInput value={query} onValueChange={setQuery} />
                <Select
                  label="Composition format"
                  value={format}
                  onValueChange={setFormat}
                  groups={formats}
                />
                <div className={w.inline}>
                  <Button
                    variant="ghost"
                    onClick={() =>
                      notify(
                        "Import is a separate integration; this demonstrates its control.",
                      )
                    }
                  >
                    <Upload />
                    Import deck
                  </Button>
                  <Button variant="primary" onClick={() => setCreateOpen(true)}>
                    <Plus />
                    Create deck
                  </Button>
                </div>
              </div>
              {filtered.length ? (
                <div className={w.cards}>
                  {filtered.map((deck, index) => (
                    <div key={`${deck.name}-${index}`} className={w.deckRow}>
                      <div>
                        <h3 style={{ marginBottom: 2 }}>{deck.name}</h3>
                        <span className={w.muted}>
                          {deck.format.charAt(0).toUpperCase() +
                            deck.format.slice(1)}
                        </span>
                      </div>
                      <Button
                        variant="secondary"
                        pill
                        size="sm"
                        onClick={() =>
                          notify(
                            `${deck.name} selected. Match launch is outside this component workshop.`,
                          )
                        }
                      >
                        <Play fill="currentColor" />
                        Play
                      </Button>
                    </div>
                  ))}
                </div>
              ) : (
                <div className={w.empty}>
                  <Search size={28} />
                  <h3>No decks found</h3>
                  <p>Try another name or clear your filters.</p>
                  <Button
                    variant="text"
                    onClick={() => {
                      setQuery("");
                      setFormat("all");
                    }}
                  >
                    Clear filters
                  </Button>
                </div>
              )}
            </div>
          </Section>

          <Section
            id="type"
            title="Typography, chosen by comparison"
            description="Lora is our chosen serif for headings, navigation and controls. The other locally hosted faces remain here for comparison."
          >
            <div className={w.two}>
              <div className={w.group}>
                {["Source Serif 4", "Libre Baskerville", "Lora"].map((name) => (
                  <div className={w.fontCard} key={name}>
                    <div className={w.inline}>
                      <strong className={w.muted}>{name}</strong>
                      <Button
                        variant={font === name ? "primary" : "text"}
                        size="sm"
                        onClick={() => setFont(name)}
                      >
                        {font === name
                          ? "Using this face"
                          : "Try across the UI"}
                      </Button>
                    </div>
                    <div style={{ fontFamily: `"${name}",serif` }}>
                      <div className={w.serifSample}>
                        Goblins · Blue Control
                      </div>
                      <div style={{ fontSize: 21 }}>
                        Open Play &nbsp; Collection &nbsp; Market &nbsp; League
                      </div>
                      <div
                        style={{
                          fontSize: 18,
                          marginTop: 12,
                          color: "#c5bbac",
                        }}
                      >
                        Search decks… &nbsp; Create deck &nbsp; Play
                      </div>
                    </div>
                  </div>
                ))}
              </div>
              <div className={w.group}>
                <h3>Roles and saved sources</h3>
                <p>
                  <strong>Display, navigation, controls:</strong> {font}. The
                  reference has a classical serif voice, including its buttons
                  and fields.
                </p>
                <p>
                  <strong>Utility:</strong> Albert Sans for helper text, labels,
                  status and dense settings. Body is 14–16px; small labels
                  12–13px.
                </p>
                <p>
                  The exact font in the raster remains unknown. These names are
                  implementation choices, not a claim that the generator used
                  them.
                </p>
                <a href="./fonts/README.md">Font source and license notes</a>
                <h3 style={{ marginTop: 32 }}>Palette</h3>
                <div className={w.swatches}>
                  {[
                    ["Ivory", "#eee7db"],
                    ["Secondary", "#bdb5a7"],
                    ["Brass", "#d7b77c"],
                    ["Soft edge", "#a98a59"],
                    ["Surface", "#171c25"],
                    ["Error", "#e3a39a"],
                  ].map(([label, color]) => (
                    <div key={label} className={w.swatch}>
                      <span
                        className={w.swatchDot}
                        style={{ background: color }}
                      />
                      <span>
                        {label}
                        <br />
                        {color}
                      </span>
                    </div>
                  ))}
                </div>
              </div>
            </div>
          </Section>

          <Section
            id="reference"
            title="The image we build toward"
            description="The approved hybrid remains unchanged. This is the authority for surface depth, navigation, control proportions and the direction of future screen work."
          >
            <a href="./reference/hybrid.png" target="_blank" rel="noreferrer">
              <img
                className={w.reference}
                src="./reference/hybrid.png"
                alt="Approved Moonlit Atelier content with Astral Vault navigation"
                loading="lazy"
              />
            </a>
            <p style={{ marginTop: 20 }}>
              The old control skins are retired as visual direction. Existing
              deck-box and candle effects remain separate integration pieces.
              This workshop implements UI primitives and illustrative local
              behavior, not account, match or economy services.
            </p>
          </Section>
          <p className={w.tiny}>
            Spellwake component workshop · hybrid reference · self-hosted fonts
            · shared React primitives
          </p>
        </main>
      </div>
      <Modal
        open={createOpen}
        onOpenChange={(open) => {
          setCreateOpen(open);
          if (!open) setSubmitted(false);
        }}
        title="Create a deck"
        description="Give your deck a name and choose its format."
      >
        <form onSubmit={createDeck} noValidate>
          <FieldStack>
            <TextInput
              label="Deck name"

              value={deckName}
              onChange={(event) => setDeckName(event.target.value)}
              placeholder="For example, Goblins"
              required
              {...(submitted && !deckName.trim()
                ? { error: "Enter a deck name to continue." }
                : {})}
            />
            <div>
              <label className={w.exampleLabel} id="create-format-label">
                Format
              </label>
              <Select
                label="New deck format"
                value={createFormat}
                onValueChange={setCreateFormat}
                groups={formats.slice(1)}
              />
            </div>
          </FieldStack>
          <DialogFooter>
            <Button variant="ghost" onClick={() => setCreateOpen(false)}>
              Cancel
            </Button>
            <Button type="submit" variant="primary">
              <Plus />
              Create deck
            </Button>
          </DialogFooter>
        </form>
      </Modal>
      {message && (
        <div className={w.status} role="status">
          <Check size={18} />
          <span>{message}</span>
          <IconButton
            variant="transparent"
            label="Dismiss feedback"
            onClick={() => setMessage("")}
          >
            <X size={16} />
          </IconButton>
        </div>
      )}
    </>
  );
}

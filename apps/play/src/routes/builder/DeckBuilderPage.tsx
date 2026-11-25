import { useEffect, useMemo, useRef, useState, type ReactElement } from "react";
import { useBlocker, useLocation, useNavigate } from "react-router-dom";
import {
  ChevronDown,
  Copy,
  LayoutList,
  MoreVertical,
  Palette,
  Plus,
  RectangleVertical,
  Trash2,
  Upload,
  Check,
} from "lucide-react";
import {
  ActionButton,
  ActionMenu,
  ConfirmationDialog,
  Dialog,
  EmptyState,
  FeedbackMessages,
  PanelTabs,
  QuantityControl,
  SelectMenu,
  TextField,
  Tooltip,
} from "@spellwake/ui";
import { TaskHeader } from "../../components/shell/TaskHeader";
import { useShellLayout } from "../../lib/display-scale";
import { AppearanceDialog } from "../cosmetics/DefaultSettingsDialog";
import { saveTextFile, textFileName } from "../../lib/save-text-file";
import { manaSymbolUrl, type BuilderCard } from "../../data/builder-catalogue";
import { faceCardOf } from "../../data/deck-face";
import {
  succeeded,
  type DeckBuilderSource,
  type DeckDraft,
  type SourceGap,
} from "../../decks/deck-sources";
import {
  addCopy,
  assignRole,
  BUILDER_SECTIONS,
  clearRole,
  copiesOf,
  exportDeckText,
  groupSection,
  moveAll,
  roleFor,
  sectionCount,
  setCopies,
  type BuilderSection,
  type CardRef,
  type DeckLine,
  type DeckGroup,
  type DeckRole,
} from "../../data/deck-draft";
import * as s from "./builder.css";

const SECTION_LABEL: Record<BuilderSection, string> = {
  main: "Main",
  sideboard: "Sideboard",
  considering: "Considering",
};
const isBuilderSection = (value: string): value is BuilderSection =>
  BUILDER_SECTIONS.some((section) => section === value);
const ROLE_LABEL: Record<DeckRole, string> = {
  commander: "Commander",
  companion: "Companion",
};
const isRole = (section: string): section is DeckRole =>
  section === "commander" || section === "companion";
const newId = (): string => crypto.randomUUID();
const options = (values: readonly string[]) =>
  values.map((value) => ({ value, label: value }));
const COLORS = ["All", "White", "Blue", "Black", "Red", "Green", "Colorless"];
const TYPES = [
  "All",
  "Creature",
  "Artifact",
  "Enchantment",
  "Instant",
  "Sorcery",
  "Planeswalker",
  "Land",
];

type Pane = "deck" | "search" | "inspector";
const PANES: readonly { id: Pane; label: string }[] = [
  { id: "deck", label: "Deck" },
  { id: "search", label: "Search" },
  { id: "inspector", label: "Card" },
];
type ReadySource = Extract<DeckBuilderSource, { status: "ready" }>;
/** What the builder says when its source has no deck to give. Loading shows nothing. */
const GAP_COPY: Record<
  Exclude<SourceGap, "loading"> | "missing",
  { title: string; description: string }
> = {
  missing: {
    title: "Deck not found",
    description: "It may have been deleted.",
  },
  "signed-out": {
    title: "Sign in to open this deck",
    description: "Your decks are saved to your account.",
  },
  unregistered: {
    title: "Finish creating your account",
    description: "Choose a username to start building decks.",
  },
  unavailable: {
    title: "Couldn’t load this deck",
    description: "Check your connection, then reload.",
  },
};
/** A deck row and a search result are inspected separately, so the origin travels with the selection. */
type Selection =
  | { origin: "deck"; entryId: string }
  | { origin: "search"; name: string; card?: CardRef }
  | null;

function CardCost({
  card,
}: {
  card: BuilderCard | undefined;
}): ReactElement | null {
  if (!card || card.cost.length === 0) return <span className={s.cost} />;
  return (
    <span
      className={s.cost}
      role="img"
      aria-label={`Mana cost ${card.cost.map((symbol) => `{${symbol}}`).join("")}`}
    >
      {card.cost.map((symbol, index) => (
        <img key={index} src={manaSymbolUrl(symbol)} alt="" />
      ))}
    </span>
  );
}

/**
 * The deck builder: a focused task with a selected-card inspector, the deck
 * and a card search. The deck and its cards come from `source`, which is the
 * account or the local preview. The screen offers only what its source can do.
 */
export function DeckBuilderPage({
  source,
}: {
  source: DeckBuilderSource;
}): ReactElement {
  const navigate = useNavigate();
  if (source.status !== "ready")
    return (
      <section className={s.page} aria-label="Deck builder">
        <TaskHeader
          back={{
            label: "Decks",
            onActivate: () => void navigate("/collection/decks"),
          }}
          title="Deck builder"
        />
        <div className={s.quiet}>
          {source.status !== "loading" && (
            <EmptyState {...GAP_COPY[source.status]} />
          )}
        </div>
      </section>
    );
  // A different deck is a different editing session.
  return <Workbench key={source.deck.id} source={source} />;
}

function Workbench({ source }: { source: ReadySource }): ReactElement {
  const navigate = useNavigate();
  const location = useLocation();
  const { deck, search } = source;
  /** The deck as this session last saved or opened it. Unsaved changes are measured against it. */
  const [saved, setSaved] = useState<DeckDraft>(() => ({
    name: deck.name,
    formatId: deck.formatId,
    lines: [...deck.lines],
    appearance: deck.appearance,
    faceCard: deck.faceCard,
  }));
  const [draft, setDraft] = useState<DeckDraft>(saved);
  const [isSaving, setIsSaving] = useState(false);
  const [section, setSection] = useState<BuilderSection>("main");
  const [view, setView] = useState<"list" | "cards">("list");
  const [groupBy, setGroupBy] = useState<"type" | "mana">("type");
  const [sortBy, setSortBy] = useState<"name" | "mana">("name");
  const [collapsed, setCollapsed] = useState<readonly string[]>([]);
  const [selection, setSelection] = useState<Selection>(null);
  const [pane, setPane] = useState<Pane>("deck");
  const [notice, setNotice] = useState<string | null>(null);
  const [dialog, setDialog] = useState<
    "export" | "delete" | "appearance" | null
  >(null);
  const isCompact = useShellLayout() === "compact";
  /**
   * What the last export action did. It is shown inside the Export dialog,
   * because a toast behind an open dialog cannot be relied on in every webview.
   */
  const [exportNotice, setExportNotice] = useState<string | null>(null);
  const [quantityDraft, setQuantityDraft] = useState<string | null>(null);
  const [quantityError, setQuantityError] = useState<string | null>(null);
  /** Set once the deck is being left on purpose, so the unsaved-changes dialog stays out of the way. */
  const isLeaving = useRef(false);

  const isDirty =
    draft.name !== saved.name ||
    draft.formatId !== saved.formatId ||
    JSON.stringify(draft.lines) !== JSON.stringify(saved.lines) ||
    JSON.stringify(draft.appearance) !== JSON.stringify(saved.appearance) ||
    draft.faceCard !== saved.faceCard;
  const format = source.formats.find(
    (choice) => choice.id === draft.formatId,
  ) ?? { id: draft.formatId, name: "Unknown format" };
  const blocker = useBlocker(
    ({ currentLocation, nextLocation }) =>
      isDirty &&
      !isLeaving.current &&
      currentLocation.pathname !== nextLocation.pathname,
  );
  useEffect(() => {
    if (!isDirty) return;
    const warn = (event: BeforeUnloadEvent): void => event.preventDefault();
    window.addEventListener("beforeunload", warn);
    return () => window.removeEventListener("beforeunload", warn);
  }, [isDirty]);
  useEffect(() => {
    if (notice === null) return;
    const timer = window.setTimeout(() => setNotice(null), 4000);
    return () => window.clearTimeout(timer);
  }, [notice]);

  const find = source.facts;
  const editEntries = (
    change: (entries: readonly DeckLine[]) => DeckLine[],
  ): void =>
    setDraft((current) => ({ ...current, lines: change(current.lines) }));
  const selectedEntry =
    selection?.origin === "deck"
      ? draft.lines.find((entry) => entry.id === selection.entryId)
      : undefined;
  const selectedCard = find(
    selectedEntry?.name ??
      (selection?.origin === "search" ? selection.name : ""),
  );
  /** Why a card cannot be added in this format, or null when it can. */
  const addBlock = (name: string): string | null => {
    // A source that cannot check legality leaves the decision to the server at play time.
    const legality = source.legality?.(name, format) ?? "legal";
    if (legality === "legal") return null;
    return legality === "unknown"
      ? "Can’t verify format eligibility."
      : `Not legal in ${format.name}.`;
  };
  const add = (
    name: string,
    to: BuilderSection,
    card?: CardRef,
    customCardRevisionId?: string,
  ): void =>
    editEntries((entries) =>
      addCopy(entries, name, to, newId, card, customCardRevisionId),
    );
  const select = (next: Selection): void => {
    setSelection(next);
    setQuantityDraft(null);
    setQuantityError(null);
    // On a one-pane layout, choosing a card is asking to see it.
    if (next) setPane("inspector");
  };
  async function save(): Promise<boolean> {
    // One save at a time: a second press waits for the first answer.
    if (isSaving) return false;
    setIsSaving(true);
    const attempt = draft;
    const result = await source.save(attempt);
    setIsSaving(false);
    if (result.status === "success") setSaved(attempt);
    else setNotice(result.message);
    return result.status === "success";
  }
  const shelfPath = deck.mode === "open" ? "/open-play/decks" : "/collection/decks";
  const leave = (): void => {
    if (location.key === "default") void navigate(shelfPath);
    else void navigate(-1);
  };
  /** Commits a typed quantity for a deck row. Zero removes the row. */
  function commitQuantity(entry: DeckLine): void {
    if (quantityDraft === null) return;
    const text = quantityDraft.trim();
    if (!/^[0-9]{1,78}$/.test(text)) {
      setQuantityError("Enter a whole number.");
      return;
    }
    const next = BigInt(text);
    const block = next > copiesOf(entry) ? addBlock(entry.name) : null;
    if (block) {
      setQuantityError(block);
      return;
    }
    editEntries((entries) => setCopies(entries, entry.id, next));
    setQuantityDraft(null);
    setQuantityError(null);
    if (next === 0n) setSelection(null);
  }
  /** The role button for a card, or nothing when the card has no role to offer here. */
  const roleAction = (
    card: BuilderCard | undefined,
    source: { entryId: string } | { name: string; card?: CardRef },
  ): ReactElement | null => {
    const role = roleFor(card, format.name);
    if (!role) return null;
    const label = `Set as ${ROLE_LABEL[role]}`;
    const block = draft.lines.some((entry) => entry.section === role)
      ? `Remove the current ${role} first.`
      : "name" in source
        ? addBlock(source.name)
        : null;
    return block ? (
      <Tooltip text={block} aria-disabled="true" className={s.pendingAction}>
        {label}
      </Tooltip>
    ) : (
      <ActionButton
        treatment="compact-form"
        emphasis="primary"
        onClick={() => {
          editEntries((entries) => assignRole(entries, source, role, newId));
          setSelection(null);
        }}
      >
        {label}
      </ActionButton>
    );
  };
  /**
   * The deck box shows one card from the deck, printed from its official
   * art: the one chosen here, or else the first card added. Offered for
   * cards with art, where the source can keep the choice.
   */
  const faceCard = faceCardOf(
    draft.faceCard,
    draft.lines
      .map((line) => line.name)
      .filter((name) => source.showsOnBox(name)),
  );
  const faceCardAction = (name: string): ReactElement | null => {
    if (!source.canChooseFaceCard || !source.showsOnBox(name)) return null;
    return (
      <div className={s.moves}>
        {faceCard === name ? (
          <p className={s.faceCard}>
            <Check aria-hidden="true" /> Face card
          </p>
        ) : (
          <ActionButton
            treatment="compact-form"
            onClick={() =>
              setDraft((current) => ({ ...current, faceCard: name }))
            }
          >
            Set as face card
          </ActionButton>
        )}
      </div>
    );
  };
  const exportText = exportDeckText(draft.lines, source.printingOf);

  const groups = useMemo(
    () =>
      groupSection(draft.lines, section, { groupBy, sortBy }, (name) => {
        const card = find(name);
        return card && { type: card.type, manaValue: card.manaValue };
      }),
    [draft.lines, section, groupBy, sortBy, find],
  );
  const shownGroups: DeckGroup[] =
    section === "main"
      ? [
          ...(["commander", "companion"] as const).flatMap((role) => {
            const entries = draft.lines.filter(
              (entry) => entry.section === role,
            );
            return entries.length > 0
              ? [
                  {
                    label: ROLE_LABEL[role],
                    count: entries.reduce(
                      (sum, entry) => sum + copiesOf(entry),
                      0n,
                    ),
                    entries,
                  },
                ]
              : [];
          }),
          ...groups,
        ]
      : groups;
  const invalidMark = (entry: DeckLine): ReactElement | null =>
    source.legality?.(entry.name, format) === "not-legal" ? (
      <Tooltip
        className={s.invalid}
        text={`Not legal in ${format.name}.`}
        aria-label={`${entry.name} is not legal in ${format.name}`}
      >
        !
      </Tooltip>
    ) : null;

  return (
    <section className={s.page} aria-label="Deck builder">
      <TaskHeader
        back={{ label: "Decks", onActivate: leave }}
        title={draft.name}
        onRename={(name) => setDraft((current) => ({ ...current, name }))}
      >
        <span className={s.wideOnly}>
          <span className={s.formatBox}>
            <SelectMenu
              label="Format"
              value={format.id}
              options={source.formats.map(({ id, name }) => ({
                value: id,
                label: name,
              }))}
              onValueChange={(formatId) =>
                // Cards stay. Entries that are no longer legal are marked, not removed.
                setDraft((current) => ({ ...current, formatId }))
              }
            />
          </span>
          {source.appearanceOptions ? (
            <ActionButton
              treatment="compact-form"
              icon={<Palette />}
              onClick={() => setDialog("appearance")}
            >
              Appearance
            </ActionButton>
          ) : (
            <Tooltip
              text="This deck’s appearance can’t be changed here"
              placement="bottom"
              aria-disabled="true"
              className={s.pendingAction}
            >
              <Palette aria-hidden="true" /> Appearance
            </Tooltip>
          )}
        </span>
        <ActionButton
          treatment="compact-form"
          emphasis="primary"
          disabled={!isDirty || isSaving}
          onClick={() => void save()}
        >
          Save
        </ActionButton>
        <ActionMenu
          label="Deck actions"
          commands={[
            // A command the source cannot carry out is left off the menu.
            ...(source.duplicate
              ? [{ id: "duplicate", label: "Duplicate", leading: <Copy /> }]
              : []),
            // The header hides Appearance on a narrow screen, so the menu carries it there.
            ...(isCompact && source.appearanceOptions
              ? [
                  {
                    id: "appearance",
                    label: "Appearance",
                    leading: <Palette />,
                  },
                ]
              : []),
            { id: "export", label: "Export", leading: <Upload /> },
            ...(source.remove
              ? [
                  {
                    id: "delete",
                    label: "Delete",
                    leading: <Trash2 />,
                    danger: true,
                    dividerBefore: true,
                  },
                ]
              : []),
          ]}
          onCommand={(command) => {
            if (command === "appearance") {
              setDialog("appearance");
              return;
            }
            if (command === "export" || command === "delete") {
              setExportNotice(null);
              setDialog(command);
              return;
            }
            void source
              .duplicate?.({
                ...draft,
                name: `${draft.name} (copy)`.slice(0, 120),
              })
              .then((result) =>
                setNotice(
                  result.status === "success"
                    ? "Deck duplicated."
                    : result.message,
                ),
              );
          }}
          trigger={
            <ActionButton
              treatment="compact-form"
              className={s.iconOnly}
              aria-label="Deck actions"
              icon={<MoreVertical />}
            />
          }
        />
      </TaskHeader>

      <div className={s.workbench}>
        <div className={s.panes} data-pane={pane}>
          <section
            className={`${s.pane} ${s.inspectorPane}`}
            aria-label="Selected card"
          >
            <div className={s.inspector}>
              {!selection || (selection.origin === "deck" && !selectedEntry) ? (
                <EmptyState treatment="quiet" title="Select a card" />
              ) : (
                <>
                  {selectedCard ? (
                    <img
                      className={s.face}
                      src={selectedCard.faceUrl}
                      alt={selectedCard.name}
                    />
                  ) : (
                    <EmptyState
                      treatment="quiet"
                      title={
                        selectedEntry?.name ??
                        (selection.origin === "search" ? selection.name : "")
                      }
                    />
                  )}
                  {selectedEntry ? (
                    isRole(selectedEntry.section) ? (
                      <div className={s.moves}>
                        <ActionButton
                          treatment="compact-form"
                          onClick={() => {
                            editEntries((entries) =>
                              clearRole(entries, selectedEntry.id, newId),
                            );
                            setSelection(null);
                          }}
                        >
                          Remove {selectedEntry.section}
                        </ActionButton>
                      </div>
                    ) : (
                      <>
                        <QuantityControl
                          label={`${selectedEntry.name} in ${SECTION_LABEL[section]}`}
                          treatment="joined"
                          value={selectedEntry.quantity}
                          canDecrease
                          canIncrease={addBlock(selectedEntry.name) === null}
                          removeOnDecrease={copiesOf(selectedEntry) === 1n}
                          edit={{
                            draft: quantityDraft ?? selectedEntry.quantity,
                            onChange: (text) => {
                              setQuantityDraft(text);
                              setQuantityError(null);
                            },
                            onCommit: () => commitQuantity(selectedEntry),
                            onCancel: () => {
                              setQuantityDraft(null);
                              setQuantityError(null);
                            },
                            ...(quantityError ? { error: quantityError } : {}),
                          }}
                          onDecrease={() => {
                            setQuantityDraft(null);
                            editEntries((entries) =>
                              setCopies(
                                entries,
                                selectedEntry.id,
                                copiesOf(selectedEntry) - 1n,
                              ),
                            );
                          }}
                          onIncrease={() => {
                            setQuantityDraft(null);
                            editEntries((entries) =>
                              setCopies(
                                entries,
                                selectedEntry.id,
                                copiesOf(selectedEntry) + 1n,
                              ),
                            );
                          }}
                        />
                        {quantityError && (
                          <p className={s.fieldError} role="alert">
                            {quantityError}
                          </p>
                        )}
                        <div className={s.moves}>
                          <div className={s.movePair}>
                            {BUILDER_SECTIONS.filter(
                              (other) => other !== selectedEntry.section,
                            ).map((other) => (
                              <ActionButton
                                key={other}
                                treatment="compact-form"
                                aria-label={`Move every copy to ${SECTION_LABEL[other]}`}
                                onClick={() => {
                                  editEntries((entries) =>
                                    moveAll(entries, selectedEntry.id, other),
                                  );
                                  // The row left this section, so nothing here is selected any more.
                                  setSelection(null);
                                }}
                              >
                                {SECTION_LABEL[other]}
                              </ActionButton>
                            ))}
                          </div>
                          {roleAction(selectedCard, {
                            entryId: selectedEntry.id,
                          })}
                        </div>
                      </>
                    )
                  ) : (
                    selection.origin === "search" && (
                      <div className={s.moves}>
                        {BUILDER_SECTIONS.map((target) => {
                          const block = addBlock(selection.name);
                          return block ? (
                            <Tooltip
                              key={target}
                              text={block}
                              aria-disabled="true"
                              className={s.pendingAction}
                            >
                              Add to {SECTION_LABEL[target]}
                            </Tooltip>
                          ) : (
                            <ActionButton
                              key={target}
                              treatment="compact-form"
                              onClick={() =>
                                add(selection.name, target, selection.card)
                              }
                            >
                              Add to {SECTION_LABEL[target]}
                            </ActionButton>
                          );
                        })}
                        {roleAction(selectedCard, selection)}
                      </div>
                    )
                  )}
                  {selectedEntry && faceCardAction(selectedEntry.name)}
                </>
              )}
            </div>
          </section>

          <section className={`${s.pane} ${s.deckPane}`} aria-label="Deck">
            <div className={s.deckHead}>
              <PanelTabs
                label="Deck sections"
                treatment="segmented"
                value={section}
                onValueChange={(value) => {
                  if (!isBuilderSection(value)) return;
                  setSection(value);
                  // A deck row belongs to its section, so it stops being the selection.
                  setSelection((current) =>
                    current?.origin === "deck" ? null : current,
                  );
                }}
                tabs={BUILDER_SECTIONS.map((value) => ({
                  value,
                  label: `${SECTION_LABEL[value]} ${sectionCount(draft.lines, value)}`,
                  content: null,
                }))}
              />
              <div className={s.deckControls}>
                <div
                  className={s.viewToggle}
                  role="group"
                  aria-label="Deck view"
                >
                  <button
                    type="button"
                    className={s.viewChoice}
                    aria-pressed={view === "list"}
                    onClick={() => setView("list")}
                  >
                    <LayoutList aria-hidden="true" />
                    <span className={s.viewLabel}>List</span>
                  </button>
                  <button
                    type="button"
                    className={s.viewChoice}
                    aria-pressed={view === "cards"}
                    onClick={() => setView("cards")}
                  >
                    <RectangleVertical aria-hidden="true" />
                    <span className={s.viewLabel}>Cards</span>
                  </button>
                </div>
                <span className={s.sortBox}>
                  <SelectMenu
                    label="Group"
                    valuePrefix="Group "
                    value={groupBy === "type" ? "Type" : "Mana value"}
                    options={options(["Type", "Mana value"])}
                    onValueChange={(value) =>
                      setGroupBy(value === "Type" ? "type" : "mana")
                    }
                  />
                </span>
                <span className={s.sortBox}>
                  <SelectMenu
                    label="Sort"
                    valuePrefix="Sort "
                    value={sortBy === "name" ? "Name" : "Mana value"}
                    options={options(["Name", "Mana value"])}
                    onValueChange={(value) =>
                      setSortBy(value === "Name" ? "name" : "mana")
                    }
                  />
                </span>
              </div>
            </div>
            <div
              className={s.deckScroll}
              role="region"
              aria-label={`${SECTION_LABEL[section]} cards`}
              tabIndex={0}
            >
              {shownGroups.length === 0 && (
                <div className={s.quiet}>
                  <EmptyState
                    treatment="quiet"
                    title={`No cards in ${SECTION_LABEL[section]}`}
                  />
                </div>
              )}
              <div className={view === "cards" ? s.tiles : undefined}>
                {shownGroups.map((group) => {
                  const isOpen = !collapsed.includes(group.label);
                  return (
                    <div
                      key={group.label}
                      className={view === "cards" ? s.tileGroup : s.group}
                    >
                      <button
                        type="button"
                        className={s.groupHead}
                        aria-expanded={isOpen}
                        onClick={() =>
                          setCollapsed((current) =>
                            isOpen
                              ? [...current, group.label]
                              : current.filter(
                                  (label) => label !== group.label,
                                ),
                          )
                        }
                      >
                        <ChevronDown aria-hidden="true" />
                        {group.label}
                        <span className={s.groupCount}>
                          {String(group.count)}
                        </span>
                      </button>
                      {isOpen &&
                        group.entries.map((entry) => {
                          const card = find(entry.name);
                          const isSelected =
                            selection?.origin === "deck" &&
                            selection.entryId === entry.id;
                          const pick = (
                            <button
                              type="button"
                              className={s.rowSelect}
                              aria-label={`Inspect ${entry.name}, ${entry.quantity} in ${isRole(entry.section) ? ROLE_LABEL[entry.section] : SECTION_LABEL[section]}`}
                              aria-pressed={isSelected}
                              onClick={() =>
                                select({ origin: "deck", entryId: entry.id })
                              }
                            />
                          );
                          return view === "cards" ? (
                            <div
                              key={entry.id}
                              className={s.tile}
                              data-selected={isSelected}
                              style={
                                card
                                  ? { backgroundImage: `url("${card.artUrl}")` }
                                  : undefined
                              }
                            >
                              {pick}
                              <div className={s.tileBar}>
                                <span className={s.cardName}>{entry.name}</span>
                                {invalidMark(entry)}
                                <span className={s.tileCount}>
                                  {entry.quantity}
                                </span>
                              </div>
                            </div>
                          ) : (
                            <div
                              key={entry.id}
                              className={s.listRow}
                              data-selected={isSelected}
                            >
                              {pick}
                              <span className={s.quantity}>
                                {entry.quantity}
                              </span>
                              {card ? (
                                <img
                                  className={s.thumb}
                                  src={card.artUrl}
                                  alt=""
                                />
                              ) : (
                                <span className={s.thumb} />
                              )}
                              <span className={s.cardName}>{entry.name}</span>
                              <span className={s.cardType}>{card?.type}</span>
                              <span className={s.rowEnd}>
                                <CardCost card={card} />
                                {invalidMark(entry)}
                              </span>
                            </div>
                          );
                        })}
                    </div>
                  );
                })}
              </div>
            </div>
          </section>

          <section
            className={`${s.pane} ${s.searchPane}`}
            aria-label="Card search"
          >
            <div className={s.searchHead}>
              <TextField
                type="search"
                label="Search cards"
                placeholder="Search cards…"
                value={search.query}
                onValueChange={search.setQuery}
                hideLabel
                treatment="compact-form"
              />
              {search.filters && (
                <div className={s.filters}>
                  <span className={s.filterBox}>
                    <SelectMenu
                      label="Color"
                      valuePrefix="Color "
                      value={search.filters.color}
                      options={options(COLORS)}
                      onValueChange={search.filters.setColor}
                    />
                  </span>
                  <span className={s.filterBox}>
                    <SelectMenu
                      label="Type"
                      valuePrefix="Type "
                      value={search.filters.type}
                      options={options(TYPES)}
                      onValueChange={search.filters.setType}
                    />
                  </span>
                </div>
              )}
            </div>
            <div
              className={s.searchScroll}
              role="region"
              aria-label="Search results"
              tabIndex={0}
            >
              {search.state === "idle" && (
                <div className={s.quiet}>
                  <EmptyState treatment="quiet" title="Search by card name" />
                </div>
              )}
              {search.state === "ready" && search.results.length === 0 && (
                <div className={s.quiet}>
                  <EmptyState
                    title="No matching cards"
                    description={
                      search.filters
                        ? "Try another name or filter."
                        : "Try another name."
                    }
                  />
                </div>
              )}
              {search.results.map((found) => {
                const block = addBlock(found.name);
                const isSelected =
                  selection?.origin === "search" &&
                  selection.name === found.name;
                return (
                  <div
                    key={
                      found.card?.definitionId ??
                      found.customCardRevisionId ??
                      found.name
                    }
                    className={s.result}
                    data-selected={isSelected}
                  >
                    <button
                      type="button"
                      className={s.rowSelect}
                      aria-label={`Inspect ${found.name}`}
                      aria-pressed={isSelected}
                      onClick={() =>
                        select({
                          origin: "search",
                          name: found.name,
                          ...(found.card ? { card: found.card } : {}),
                        })
                      }
                    />
                    {found.facts ? (
                      <img
                        className={s.resultArt}
                        src={found.facts.artUrl}
                        alt=""
                      />
                    ) : (
                      <span className={s.resultArt} aria-hidden="true" />
                    )}
                    <span>
                      <span className={s.resultName}>{found.name}</span>
                      {found.facts && (
                        <>
                          <br />
                          <span className={s.resultType}>
                            {found.facts.type}
                          </span>
                        </>
                      )}
                    </span>
                    <CardCost card={found.facts} />
                    {block ? (
                      <Tooltip
                        className={s.add}
                        text={block}
                        placement="left"
                        aria-disabled="true"
                        aria-label={`Add ${found.name} to ${SECTION_LABEL[section]}`}
                      >
                        <Plus aria-hidden="true" />
                      </Tooltip>
                    ) : (
                      <button
                        type="button"
                        className={s.add}
                        aria-label={`Add ${found.name} to ${SECTION_LABEL[section]}`}
                        onClick={() =>
                          add(
                            found.name,
                            section,
                            found.card,
                            found.customCardRevisionId,
                          )
                        }
                      >
                        <Plus aria-hidden="true" />
                      </button>
                    )}
                  </div>
                );
              })}
            </div>
          </section>
        </div>
      </div>

      <div
        className={s.paneSwitch}
        role="group"
        aria-label="Builder panes"
        data-frame="pane-switch"
      >
        {PANES.map((choice) => (
          <button
            key={choice.id}
            type="button"
            className={s.paneChoice}
            aria-pressed={choice.id === pane}
            onClick={() => setPane(choice.id)}
          >
            {choice.label}
          </button>
        ))}
      </div>

      <Dialog
        open={blocker.state === "blocked"}
        size="compact"
        title="Unsaved changes"
        busy={isSaving}
        onClose={() => blocker.reset?.()}
        actions={
          <>
            <ActionButton
              treatment="compact-form"
              onClick={() => blocker.proceed?.()}
            >
              Discard
            </ActionButton>
            <ActionButton
              treatment="compact-form"
              emphasis="primary"
              disabled={isSaving}
              onClick={() =>
                // Leave only once the save is accepted. A failed save keeps the draft and this screen.
                void save().then((isSaved) =>
                  isSaved ? blocker.proceed?.() : blocker.reset?.(),
                )
              }
            >
              Save
            </ActionButton>
          </>
        }
      >
        Save your changes to {draft.name}?
      </Dialog>
      <Dialog
        open={dialog === "export"}
        title="Export deck"
        onClose={() => setDialog(null)}
        actions={
          <>
            <ActionButton
              treatment="compact-form"
              onClick={() => setDialog(null)}
            >
              Close
            </ActionButton>
            <ActionButton
              treatment="compact-form"
              disabled={exportText === ""}
              onClick={() => {
                navigator.clipboard.writeText(exportText).then(
                  () => setExportNotice("List copied."),
                  () => setExportNotice("Couldn’t copy list."),
                );
              }}
            >
              Copy list
            </ActionButton>
            <ActionButton
              treatment="compact-form"
              emphasis="primary"
              // An empty deck has nothing to copy or save.
              disabled={exportText === ""}
              onClick={() =>
                void saveTextFile(
                  textFileName(draft.name, "deck"),
                  exportText,
                ).then((outcome) => {
                  // A closed save dialog needs no message, and a browser shows its own download.
                  if (outcome === "saved") setExportNotice("Deck list saved.");
                  if (outcome === "failed")
                    setExportNotice("Couldn’t save the file.");
                  if (outcome === "cancelled" || outcome === "started")
                    setExportNotice(null);
                })
              }
            >
              Download .txt
            </ActionButton>
          </>
        }
      >
        <textarea
          className={s.exportText}
          aria-label="Deck list"
          readOnly
          value={exportText}
        />
        <div className={s.exportNotice}>
          <FeedbackMessages
            label="Export messages"
            presentation="inline"
            items={
              exportNotice === null
                ? []
                : [{ id: exportNotice, text: exportNotice }]
            }
          />
        </div>
      </Dialog>
      {dialog === "appearance" && source.appearanceOptions && (
        <AppearanceDialog
          title="Deck appearance"
          subtitle={draft.name}
          confirmLabel="Apply"
          working={null}
          options={source.appearanceOptions}
          initial={draft.appearance}
          failure={
            source.appearanceOptions.items.state === "failed"
              ? "Couldn’t load your cosmetics."
              : null
          }
          // Apply changes the deck being edited. Save writes it with everything else.
          onConfirm={(appearance) => {
            setDraft((current) => ({ ...current, appearance }));
            return Promise.resolve(succeeded(undefined));
          }}
          onClose={() => setDialog(null)}
        />
      )}
      <ConfirmationDialog
        open={dialog === "delete"}
        appearance="lobby"
        tone="danger"
        title="Delete deck?"
        description={`Delete ${draft.name}? This cannot be undone.`}
        cancelLabel="Cancel"
        confirmLabel="Delete"
        onCancel={() => setDialog(null)}
        onConfirm={() =>
          void source.remove?.().then((result) => {
            if (result.status !== "success") {
              setDialog(null);
              setNotice(result.message);
              return;
            }
            isLeaving.current = true;
            void navigate(shelfPath, { replace: true });
          })
        }
      />
      <FeedbackMessages
        label="Deck builder messages"
        presentation="toast"
        items={[
          ...(source.hasCardsFailed
            ? [{ id: "catalogue", text: "Couldn’t load cards." }]
            : []),
          ...(notice === null
            ? []
            : [{ id: notice, text: notice, onDismiss: () => setNotice(null) }]),
        ]}
      />
    </section>
  );
}

import { useRef, useState, type ReactElement } from "react";
import { useNavigate } from "react-router-dom";
import { parseAsBoolean, parseAsString, useQueryState } from "nuqs";
import { motion } from "motion/react";
import { Play, Plus, Upload } from "lucide-react";
import {
  ActionButton,
  EmptyState,
  FeedbackMessages,
  SelectMenu,
  TextField,
} from "@spellwake/ui";
import { DeckBox } from "@spellwake/deck-box";
import {
  failed,
  succeeded,
  type DeckShelfSource,
  type PlayMode,
  type SourceGap,
  type Outcome,
} from "../../decks/deck-sources";
import { ImportDeckDialog } from "../import/ImportDeckDialog";
import { lobbyPath } from "../../lobby/LobbyRoutes";
import { motionLanguage as m } from "../../lib/motion";
import * as s from "./deck-shelf.css";

const ALL_FORMATS = "All formats";
/** What the shelf says when its source has no decks to give. Loading shows nothing. */
const GAP_COPY: Record<
  Exclude<SourceGap, "loading">,
  { title: string; description: string }
> = {
  "signed-out": {
    title: "Sign in to see your decks",
    description: "Your decks are saved to your account.",
  },
  unregistered: {
    title: "Finish creating your account",
    description: "Choose a username to start building decks.",
  },
  unavailable: {
    title: "Couldn’t load your decks",
    description: "Check your connection, then reload.",
  },
};
/** The deck Play was pressed for, and the two ways out of what it opens. */
export type PlayProps = {
  deck: { id: string; name: string; format: string };
  /** Opens the lobby with this deck. */
  onFriends: () => void;
  onClose: () => void;
};
const MODEL_URL = `${import.meta.env.BASE_URL}models/deck-box-leather.glb?bend=1`;
/** Tiles beyond this arrive together so a long shelf never keeps the eye waiting. */
const STAGGERED_TILES = 9;

/**
 * A mode's deck shelf. Collection and Open Play share this body inside
 * different navigation frames, as approved in the shared deck-library set.
 * The decks come from `source`, which is the account or the local preview.
 */
export function DeckShelfPage({
  mode,
  source,
  play,
}: {
  mode: PlayMode;
  source: DeckShelfSource;
  /** What Play on a deck opens. It comes from the account or the preview, as the decks do. */
  play: (props: PlayProps) => ReactElement;
}): ReactElement {
  const navigate = useNavigate();
  const [query, setQuery] = useQueryState("q", parseAsString.withDefault(""));
  // The address carries the format's name, which reads the same from either source.
  const [formatName, setFormatName] = useQueryState("format", parseAsString);
  const [createFailure, setCreateFailure] = useState<string | null>(null);
  const [playing, setPlaying] = useState<{
    id: string;
    name: string;
    format: string;
  } | null>(null);
  const [isImporting, setIsImporting] = useQueryState("import", parseAsBoolean);
  const isCreating = useRef(false);
  const searchRegion = useRef<HTMLDivElement>(null);

  const shelf = source.status === "ready" ? source : null;
  const formats = shelf?.formats ?? [];
  const format = formats.find((choice) => choice.name === formatName) ?? null;
  const needle = query.trim().toLowerCase();
  const visible = (shelf?.decks ?? []).filter(
    (deck) =>
      deck.name.toLowerCase().includes(needle) &&
      (format === null || deck.format.id === format.id),
  );
  const isFiltered = needle !== "" || format !== null;

  async function create(): Promise<void> {
    const formatId = newDeckFormat();
    // The original command wins. A second press cannot make a second deck.
    if (!shelf || formatId === undefined || isCreating.current) return;
    isCreating.current = true;
    const result = await shelf.createDeck(formatId);
    if (result.status !== "success") {
      isCreating.current = false;
      setCreateFailure(result.message);
      return;
    }
    shelf.rememberFormat(formatId);
    void navigate(`/decks/${result.value.deckId}`);
  }
  /** The format a new deck starts in: the shelf's filter, then the last used, then the first. */
  const newDeckFormat = (): string | undefined =>
    format?.id ?? shelf?.lastFormatId ?? formats[0]?.id;
  async function importList(text: string): Promise<Outcome> {
    const formatId = newDeckFormat();
    if (!shelf || formatId === undefined)
      return failed("Couldn’t import deck.");
    const result = await shelf.importDeck(formatId, text);
    if (result.status !== "success") return result;
    shelf.rememberFormat(formatId);
    void navigate(`/decks/${result.value.deckId}`);
    return succeeded(undefined);
  }
  function clearFilters(): void {
    void setQuery(null);
    void setFormatName(null);
    searchRegion.current?.querySelector("input")?.focus();
  }

  return (
    <section className={s.page} data-mode={mode} aria-label="Decks">
      <div className={s.toolbar}>
        <div className={s.search} ref={searchRegion}>
          <TextField
            type="search"
            label="Search decks"
            placeholder="Search decks…"
            value={query}
            onValueChange={(value) => void setQuery(value || null)}
            hideLabel
            treatment="compact-form"
          />
        </div>
        <div className={`${s.format} ${s.formatWide}`}>
          <SelectMenu
            label="Format"
            value={format?.name ?? ALL_FORMATS}
            options={[ALL_FORMATS, ...formats.map(({ name }) => name)].map(
              (value) => ({ value, label: value }),
            )}
            onValueChange={(value) => {
              const chosen = formats.find((choice) => choice.name === value);
              void setFormatName(chosen?.name ?? null);
              if (chosen) shelf?.rememberFormat(chosen.id);
            }}
          />
        </div>
        <ActionButton
          treatment="compact-form"
          icon={<Upload />}
          disabled={!shelf}
          onClick={() => void setIsImporting(true)}
        >
          Import deck
        </ActionButton>
        <ActionButton
          treatment="compact-form"
          emphasis="primary"
          icon={<Plus />}
          disabled={!shelf}
          onClick={() => void create()}
        >
          Create deck
        </ActionButton>
      </div>

      {visible.length > 0 ? (
        <div
          className={s.scroll}
          role="region"
          aria-label="Deck library"
          tabIndex={0}
        >
          <div className={s.grid}>
            {visible.map((deck, index) => (
              <motion.article
                key={deck.id}
                className={s.deck}
                initial={{ opacity: 0, y: m.travel }}
                animate={{ opacity: 1, y: 0 }}
                transition={{
                  duration: m.duration.hero,
                  ease: m.ease.arrive,
                  delay:
                    m.duration.settle +
                    Math.min(index, STAGGERED_TILES) * m.stagger,
                }}
              >
                <span className={s.floor} aria-hidden="true" />
                <DeckBox
                  name={deck.name}
                  appearance={deck.appearance}
                  modelUrl={MODEL_URL}
                  artworkUrl={import.meta.env.BASE_URL}
                  className={s.open}
                  stageClassName={s.boxStage}
                  statusClassName={s.boxStatus}
                  onInspect={() => void navigate(`/decks/${deck.id}`)}
                />
                <div className={s.caption}>
                  <div className={s.title}>
                    <h2 className={s.name} title={deck.name}>
                      {deck.name}
                    </h2>
                    <p className={s.formatLine}>{deck.format.name}</p>
                  </div>
                  <span className={s.play}>
                    <ActionButton
                      treatment="compact-form"
                      icon={<Play fill="currentColor" />}
                      aria-label={`Play ${deck.name}`}
                      onClick={() =>
                        setPlaying({
                          id: deck.id,
                          name: deck.name,
                          format: deck.format.name,
                        })
                      }
                    >
                      Play
                    </ActionButton>
                  </span>
                </div>
              </motion.article>
            ))}
          </div>
        </div>
      ) : (
        <div className={s.center}>
          {source.status === "loading" ? null : source.status !== "ready" ? (
            <EmptyState {...GAP_COPY[source.status]} />
          ) : isFiltered ? (
            <EmptyState
              title="No matching decks"
              description="Try another name or format."
              action={
                <ActionButton treatment="compact-form" onClick={clearFilters}>
                  Clear filters
                </ActionButton>
              }
            />
          ) : (
            <EmptyState
              title="No decks yet"
              description="Create or import a deck."
              action={
                <ActionButton
                  treatment="compact-form"
                  onClick={() => void create()}
                >
                  Create deck
                </ActionButton>
              }
            />
          )}
        </div>
      )}

      <FeedbackMessages
        label="Deck library messages"
        presentation="toast"
        items={
          createFailure === null
            ? []
            : [
                {
                  id: "create-failed",
                  text: createFailure,
                  onDismiss: () => setCreateFailure(null),
                },
              ]
        }
      />
      {playing &&
        play({
          deck: playing,
          onFriends: () =>
            void navigate(`${lobbyPath(mode)}?deck=${playing.id}`),
          onClose: () => setPlaying(null),
        })}
      {isImporting && shelf && (
        <ImportDeckDialog
          onImport={importList}
          onClose={() => void setIsImporting(null)}
        />
      )}
    </section>
  );
}

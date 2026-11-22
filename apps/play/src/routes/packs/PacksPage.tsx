import { useEffect, useRef, type ReactElement } from "react";
import { parseAsString, useQueryState } from "nuqs";
import { motion, useReducedMotion } from "motion/react";
import { preloadPackStage } from "@spellwake/booster-pack";
import { Play } from "lucide-react";
import {
  ActionButton,
  EmptyState,
  FeedbackMessages,
  SelectMenu,
  TextField,
} from "@spellwake/ui";
import { packsLabel, type PacksSource } from "../../collection/pack-sources";
import { motionLanguage as m } from "../../lib/motion";
import { PackOpening } from "./PackOpening";
import { PackWrapper } from "./PackWrapper";
import * as s from "./packs.css";
import * as cards from "../cards/cards.css";

const ALL_SETS = "All sets";
const STAGGERED_TILES = 8;
const LOADING_TILES = 4;
const GAP_COPY = {
  "signed-out": {
    title: "Sign in to see your packs",
    description: "Your packs are saved to your account.",
  },
  unregistered: {
    title: "Finish creating your account",
    description: "Choose a username to start collecting.",
  },
} as const;

/**
 * Collection Packs: the owned packs, searched and narrowed by set, each with
 * Open pack. Opening starts at once, one pack at a time, on a stage over the
 * library that stays until the opening is resolved.
 */
export function PacksPage({ source }: { source: PacksSource }): ReactElement {
  const [query, setQuery] = useQueryState("q", parseAsString.withDefault(""));
  const [setName, setSetName] = useQueryState("set", parseAsString);
  const searchRegion = useRef<HTMLDivElement>(null);
  const opener = useRef<HTMLElement | null>(null);
  const ready = source.status === "ready" ? source : null;
  // Only packs that can be opened now: those in a trade or listed are not counted.
  const packs = (ready?.packs ?? []).filter((pack) => pack.available > 0);
  // The set menu lists the sets owned, and stays the same while searching.
  const sets = [...new Set(packs.map((pack) => pack.setName))];
  const chosenSet = setName && sets.includes(setName) ? setName : null;
  const needle = query.trim().toLowerCase();
  const visible = packs.filter(
    (pack) =>
      (chosenSet === null || pack.setName === chosenSet) &&
      [pack.setName, pack.product, pack.setCode ?? ""].some((text) =>
        text.toLowerCase().includes(needle),
      ),
  );
  const isFiltered = needle !== "" || chosenSet !== null;
  const opening = ready?.opening ?? null;
  // Fetch the 3D opening once the library has settled, so Open pack starts at once.
  const reduce = useReducedMotion() ?? false;
  const canOpen = !!ready?.canOpen && packs.some((pack) => pack.available > 0);
  useEffect(() => {
    if (reduce || !canOpen) return;
    const timer = window.setTimeout(preloadPackStage, 1200);
    return () => window.clearTimeout(timer);
  }, [reduce, canOpen]);
  const openingPack = packs.find((pack) => pack.id === opening?.packId);

  function clearFilters(): void {
    void setQuery(null);
    void setSetName(null);
    searchRegion.current?.querySelector("input")?.focus();
  }

  return (
    <section className={s.page} aria-labelledby="packs-heading">
      <h1
        id="packs-heading"
        className={s.srOnly}
        tabIndex={-1}
        data-packs-heading=""
      >
        Packs
      </h1>
      <div className={s.toolbar}>
        <div className={s.search} ref={searchRegion}>
          <TextField
            type="search"
            label="Search packs"
            placeholder="Search packs…"
            value={query}
            onValueChange={(value) => void setQuery(value || null)}
            hideLabel
            treatment="compact-form"
          />
        </div>
        <SelectMenu
          label="Set"
          value={chosenSet ?? ALL_SETS}
          options={[ALL_SETS, ...sets].map((value) => ({
            value,
            label: value,
          }))}
          onValueChange={(value) =>
            void setSetName(value === ALL_SETS ? null : value)
          }
          disabled={!ready}
        />
      </div>

      {source.status === "loading" ? (
        <div className={s.scroll} role="status" aria-label="Loading packs">
          <ul className={s.grid} aria-hidden="true">
            {Array.from({ length: LOADING_TILES }, (_, index) => (
              <li key={index} className={s.tile}>
                <span className={s.wrapperSlot}>
                  <span className={s.blank} />
                </span>
              </li>
            ))}
          </ul>
        </div>
      ) : visible.length > 0 ? (
        <div
          className={s.scroll}
          role="region"
          aria-label="Pack library"
          tabIndex={-1}
        >
          <ul className={s.grid}>
            {visible.map((pack, index) => {
              return (
                <motion.li
                  key={pack.id}
                  className={s.tile}
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
                  <span className={s.wrapperSlot}>
                    <PackWrapper
                      setName={pack.setName}
                      product={pack.product}
                      setCode={pack.setCode}
                    />
                    {/* The same count badge the Cards screen puts on a card. */}
                    <span className={cards.count} aria-hidden="true">
                      ×{pack.available}
                    </span>
                  </span>
                  <h2 className={s.setName}>{pack.setName}</h2>
                  <p className={s.srOnly}>{packsLabel(pack.available)}</p>
                  <span className={s.openButton}>
                    <ActionButton
                      treatment="compact-form"
                      icon={<Play fill="currentColor" />}
                      aria-label={`Open a ${pack.product}`}
                      disabled={
                        !ready?.canOpen || pack.available < 1 || !!opening
                      }
                      onClick={(event) => {
                        opener.current = event.currentTarget;
                        ready?.open(pack.id);
                      }}
                    >
                      Open pack
                    </ActionButton>
                  </span>
                </motion.li>
              );
            })}
          </ul>
        </div>
      ) : (
        <div className={s.center}>
          {source.status === "signed-out" ||
          source.status === "unregistered" ? (
            <EmptyState {...GAP_COPY[source.status]} />
          ) : source.status === "unavailable" ||
            ready?.isUnread ? null : isFiltered && packs.length > 0 ? (
            <div>
              <p className={s.quietTitle}>No matching packs</p>
              <button
                type="button"
                className={s.textLink}
                onClick={clearFilters}
              >
                Clear filters
              </button>
            </div>
          ) : (
            <EmptyState
              title="No packs yet"
              description="Packs you own will appear here."
            />
          )}
        </div>
      )}

      <FeedbackMessages
        label="Packs messages"
        presentation="toast"
        items={
          ready?.notice
            ? [
                {
                  id: "notice",
                  text: ready.notice,
                  onDismiss: ready.dismissNotice,
                },
              ]
            : source.status === "unavailable"
              ? [{ id: "unavailable", text: "Couldn’t load packs." }]
              : []
        }
      />
      {opening && ready && (
        <PackOpening
          opening={opening}
          setName={openingPack?.setName}
          setCode={openingPack?.setCode ?? null}
          canOpenAnother={
            !!openingPack && openingPack.available > 0 && ready.canOpen
          }
          onCheck={ready.checkOpening}
          onClose={ready.closeOpening}
          onOpenAnother={() => {
            const packId = opening.packId;
            ready.closeOpening();
            ready.open(packId);
          }}
          returnFocusRef={opener}
        />
      )}
    </section>
  );
}

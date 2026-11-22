import {
  useEffect,
  useId,
  useState,
  type ReactElement,
  type RefObject,
} from "react";
import {
  ActionButton,
  Dialog,
  FeedbackMessages,
  LoadingShape,
  LoadingState,
} from "@spellwake/ui";
import {
  KIND_NOUN,
  sameSelection,
  type AppearanceOptions,
  type CosmeticKind,
  type DefaultSelection,
  type DefaultSettingsSource,
} from "../../cosmetics/cosmetic-sources";
import type { Outcome } from "../../decks/deck-sources";
import { useCommand, useNotice } from "../account/operation";
import {
  CategoryList,
  ItemPreview,
  TileGrid,
  type Tile,
} from "./CosmeticPicker";
import * as s from "./cosmetics.css";

/** The choices being made: one per category, from what the account owns. */
function Picker({
  options,
  draft,
  setDraft,
}: {
  options: AppearanceOptions;
  draft: DefaultSelection;
  setDraft: (next: DefaultSelection) => void;
}): ReactElement {
  const panelId = useId();
  const [kind, setKind] = useState<CosmeticKind>(
    options.categories[0] ?? "sleeve",
  );
  const owned = options.items.items.filter((item) => item.kind === kind);
  const selectedId = draft[kind];
  const tiles: Tile[] = [
    ...(options.optional.includes(kind) ? [{ item: null }] : []),
    ...owned.map((item) => ({ item })),
  ];
  const selected = owned.find((item) => item.id === selectedId) ?? null;
  return (
    <div className={s.columns}>
      <CategoryList
        categories={options.categories}
        current={kind}
        onChange={setKind}
        panelId={panelId}
      />
      <span className={s.divider} />
      <div
        id={panelId}
        role="tabpanel"
        aria-label={KIND_NOUN[kind]}
        className={s.gallery}
      >
        <TileGrid
          kind={kind}
          tiles={tiles}
          selectedId={selectedId}
          showTick
          state={options.items.state}
          emptyText="No cosmetics found."
          onSelect={(itemId) => setDraft({ ...draft, [kind]: itemId })}
        />
      </div>
      <span className={s.divider} />
      <div className={s.preview}>
        <ItemPreview item={selected} kind={kind} />
      </div>
    </div>
  );
}

/**
 * Choosing an appearance: sleeves, card backs, dice, playmat and pet. Shared
 * by Default settings, which saves for new decks, and a deck's Appearance,
 * which applies to the deck being edited. The confirm action is enabled only
 * once something differs from `initial`. Closing any other way discards the
 * choices, with no question asked.
 */
export function AppearanceDialog({
  title,
  subtitle,
  confirmLabel,
  working,
  options,
  initial,
  failure,
  onConfirm,
  onClose,
  returnFocusRef,
}: {
  title: string;
  subtitle: string;
  confirmLabel: string;
  /** The passive line shown while `onConfirm` runs, or null where it is instant. */
  working: string | null;
  /** Null while what the account owns is not yet known. */
  options: AppearanceOptions | null;
  /** The choices the dialog opens with. Null while they are not yet known. */
  initial: DefaultSelection | null;
  /** A read failure to say once, such as cosmetics that could not be loaded. */
  failure: string | null;
  /** Resolves to failure with a message to show; on success the dialog closes. */
  onConfirm: (selection: DefaultSelection) => Promise<Outcome>;
  onClose: () => void;
  returnFocusRef?: RefObject<HTMLElement | null>;
}): ReactElement {
  const [draft, setDraft] = useState<DefaultSelection | null>(null);
  const command = useCommand();
  const notice = useNotice();
  const { say } = notice;
  // The first time the starting choices are known, the dialog starts from them.
  useEffect(() => {
    if (initial) setDraft((current) => current ?? initial);
  }, [initial]);
  useEffect(() => {
    if (failure) say(failure);
  }, [failure, say]);

  const isChanged =
    options !== null &&
    initial !== null &&
    draft !== null &&
    !sameSelection(draft, initial, options.categories);
  const confirm = (): void => {
    if (!draft || !isChanged) return;
    if (working) say(working, "working");
    void command
      .run(() => onConfirm(draft))
      .then((outcome) => {
        if (outcome?.status === "success") onClose();
        else if (outcome) say(outcome.message);
      });
  };

  return (
    <Dialog
      open
      title={title}
      subtitle={subtitle}
      size="wide"
      className={s.shell}
      busy={command.isBusy}
      onClose={onClose}
      {...(returnFocusRef ? { returnFocusRef } : {})}
      actions={
        <>
          <ActionButton
            className={s.action}
            disabled={command.isBusy}
            onClick={onClose}
          >
            Cancel
          </ActionButton>
          <ActionButton
            className={s.action}
            emphasis="primary"
            disabled={!isChanged}
            pending={command.isBusy}
            onClick={confirm}
          >
            {confirmLabel}
          </ActionButton>
        </>
      }
    >
      <div className={s.layout} data-cosmetic-picker="appearance">
        {options && draft ? (
          <Picker options={options} draft={draft} setDraft={setDraft} />
        ) : (
          <LoadingState label="Loading your cosmetics">
            <div className={s.columns}>
              <LoadingShape width="100%" height="280px" />
              <span className={s.divider} />
              <LoadingShape width="100%" height="280px" />
              <span className={s.divider} />
              <LoadingShape width="100%" height="280px" />
            </div>
          </LoadingState>
        )}
        <div className={s.notice}>
          <FeedbackMessages
            label={`${title} messages`}
            presentation="toast"
            items={notice.items}
          />
        </div>
      </div>
    </Dialog>
  );
}

/**
 * Default settings: the appearance every new deck starts with. Save writes it
 * and closes.
 */
export function DefaultSettingsDialog({
  source,
  onClose,
  returnFocusRef,
}: {
  source: DefaultSettingsSource;
  onClose: () => void;
  returnFocusRef?: RefObject<HTMLElement | null>;
}): ReactElement {
  const ready = source.status === "ready" ? source : null;
  const isUnreadable =
    ready?.options.state === "failed" ||
    (source.status !== "ready" && source.status !== "loading");
  return (
    <AppearanceDialog
      title="Default settings"
      subtitle="Automatically applies to all new decks."
      confirmLabel="Save"
      working="Saving defaults…"
      options={
        ready
          ? {
              categories: ready.categories,
              optional: ready.optional,
              items: ready.options,
            }
          : null
      }
      initial={ready?.saved ?? null}
      failure={isUnreadable ? "Couldn’t load your cosmetics." : null}
      onConfirm={(selection) =>
        ready ? ready.save(selection) : Promise.reject(new Error("Not ready"))
      }
      onClose={onClose}
      {...(returnFocusRef ? { returnFocusRef } : {})}
    />
  );
}

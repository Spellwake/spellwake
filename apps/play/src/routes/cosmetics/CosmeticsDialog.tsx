import {
  useEffect,
  useId,
  useMemo,
  useState,
  type ReactElement,
  type RefObject,
} from "react";
import {
  ActionButton,
  Dialog,
  FeedbackMessages,
  TextField,
} from "@spellwake/ui";
import {
  KIND_NOUN,
  matching,
  type CheckoutNotice,
  type CosmeticKind,
  type CosmeticsSource,
} from "../../cosmetics/cosmetic-sources";
import { CategoryList, ItemPreview, TileGrid } from "./CosmeticPicker";
import * as s from "./cosmetics.css";

export const COSMETIC_VIEWS = ["owned", "shop"] as const;
export type CosmeticView = (typeof COSMETIC_VIEWS)[number];
const VIEW_LABEL: Readonly<Record<CosmeticView, string>> = {
  owned: "Your cosmetics",
  shop: "Shop",
};
const RESULT_MS = 4000;

/** The source's status line. A result leaves by itself; a working line stays. */
function useShownNotice(notice: CheckoutNotice | null): CheckoutNotice | null {
  const [hidden, setHidden] = useState<CheckoutNotice | null>(null);
  useEffect(() => {
    if (!notice || notice.kind !== "result") return;
    const timer = window.setTimeout(() => setHidden(notice), RESULT_MS);
    return () => window.clearTimeout(timer);
  }, [notice]);
  return notice && notice !== hidden ? notice : null;
}

/**
 * Cosmetics: what the account owns, and a shop of items sold one at a time.
 * Buying opens hosted checkout. Nothing here changes a deck or the defaults.
 */
export function CosmeticsDialog({
  source,
  view,
  onViewChange,
  onClose,
  returnFocusRef,
}: {
  source: CosmeticsSource;
  view: CosmeticView;
  onViewChange: (view: CosmeticView) => void;
  onClose: () => void;
  returnFocusRef?: RefObject<HTMLElement | null>;
}): ReactElement {
  const panelId = useId();
  const ready = source.status === "ready" ? source : null;
  const [kind, setKind] = useState<CosmeticKind>(
    ready?.categories[0] ?? "sleeve",
  );
  const [query, setQuery] = useState("");
  const [selectedId, setSelectedId] = useState<string | null>(null);
  const inView = (ready?.items ?? []).filter((item) =>
    view === "owned" ? item.isOwned : item.isSold,
  );
  const shown = matching(inView, kind, query);
  // The selection follows the gallery: when it leaves the list, the first
  // match takes its place, and no match clears the preview.
  const selected =
    shown.find((item) => item.id === selectedId) ?? shown[0] ?? null;
  const checkout = ready?.checkout ?? null;
  const choose = checkout?.choose;
  useEffect(() => {
    choose?.(selected?.id ?? null);
  }, [selected?.id, choose]);
  const hasFailed =
    ready?.state === "failed" ||
    (source.status !== "ready" && source.status !== "loading");
  const failure = useMemo<CheckoutNotice | null>(
    () =>
      hasFailed ? { text: "Couldn’t load cosmetics.", kind: "result" } : null,
    [hasFailed],
  );
  const shownNotice = useShownNotice(checkout?.notice ?? failure);

  return (
    <Dialog
      open
      title="Cosmetics"
      size="wide"
      className={s.shell}
      onClose={onClose}
      {...(returnFocusRef ? { returnFocusRef } : {})}
    >
      <div className={s.layout} data-cosmetic-picker={view}>
        <div className={s.tabs}>
          <div role="tablist" aria-label="Cosmetics" className={s.viewTabs}>
            {COSMETIC_VIEWS.map((value) => (
              <button
                key={value}
                type="button"
                role="tab"
                className={s.viewTab}
                aria-selected={value === view}
                tabIndex={value === view ? 0 : -1}
                onClick={() => onViewChange(value)}
                onKeyDown={(event) => {
                  if (event.key !== "ArrowLeft" && event.key !== "ArrowRight")
                    return;
                  event.preventDefault();
                  const next = value === "owned" ? "shop" : "owned";
                  onViewChange(next);
                  event.currentTarget.parentElement
                    ?.querySelector<HTMLElement>(`[data-view="${next}"]`)
                    ?.focus();
                }}
                data-view={value}
              >
                {VIEW_LABEL[value]}
              </button>
            ))}
          </div>
        </div>
        <div className={s.columns}>
          <CategoryList
            categories={ready?.categories ?? []}
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
            <TextField
              label="Search cosmetics"
              hideLabel
              type="search"
              placeholder="Search cosmetics…"
              autoComplete="off"
              spellCheck={false}
              value={query}
              onValueChange={setQuery}
            />
            <TileGrid
              kind={kind}
              tiles={shown.map((item) => ({
                item,
                detail: view === "shop" ? item.price : null,
              }))}
              selectedId={selected?.id ?? null}
              showTick={false}
              state={ready?.state ?? "loading"}
              emptyText="No cosmetics found."
              onSelect={(itemId) => setSelectedId(itemId)}
            />
          </div>
          <span className={s.divider} />
          <div className={s.preview}>
            <ItemPreview item={selected} kind={kind} />
            {selected && (
              <div className={s.details}>
                <h3 className={s.itemName}>{selected.name}</h3>
                <p className={s.itemKind}>{KIND_NOUN[selected.kind]}</p>
                {view === "shop" && selected.price && !selected.isOwned && (
                  <p className={s.itemPrice}>{selected.price} USD</p>
                )}
                {selected.isOwned ? (
                  <span className={s.owned}>Owned</span>
                ) : (
                  view === "shop" &&
                  checkout && (
                    <ActionButton
                      className={s.buy}
                      emphasis="primary"
                      pending={checkout.isBusy}
                      onClick={checkout.buy}
                    >
                      Buy
                    </ActionButton>
                  )
                )}
              </div>
            )}
          </div>
        </div>
        <div className={s.notice}>
          <FeedbackMessages
            label="Cosmetics messages"
            presentation="toast"
            items={
              shownNotice
                ? [{ id: shownNotice.text, text: shownNotice.text }]
                : []
            }
          />
        </div>
      </div>
    </Dialog>
  );
}

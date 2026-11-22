import {
  useId,
  useLayoutEffect,
  useRef,
  useState,
  type RefObject,
} from "react";
import { motion } from "motion/react";
import { X } from "lucide-react";
import { ActionButton } from "../button/ActionButton";
import { EmptyState } from "../feedback/EmptyState";
import { TextField } from "../field/TextField";
import {
  OwnedItemPicker,
  type OwnedPickerItem,
} from "../inventory/OwnedItemPicker";
import { PanelTabs } from "../navigation/PanelTabs";
import { controlMotion } from "../control-motion";
import { useAllowsControlMotion } from "../useAllowsControlMotion";
import * as s from "./itemPicker.css";
import { designPixels } from "../design-pixels";

const labels = { card: "Cards", pack: "Packs", coin: "Coins" } as const;
type Kind = OwnedPickerItem["kind"];
export interface TradeItemPickerProps {
  items: readonly OwnedPickerItem[];
  onAdd: (id: string) => void;
  onClose: () => void;
  initialTab?: Kind;
  /** Prefill only the initially open tab. */
  initialSearch?: string;
  returnFocusRef?: RefObject<HTMLButtonElement | null>;
}

function Results({
  kind,
  items,
  query,
  onQuery,
  onAdd,
  positions,
}: {
  kind: Kind;
  items: readonly OwnedPickerItem[];
  query: string;
  onQuery: (value: string) => void;
  onAdd: TradeItemPickerProps["onAdd"];
  positions: RefObject<Record<Kind, number>>;
}) {
  const list = useRef<HTMLDivElement>(null);
  useLayoutEffect(() => {
    if (list.current) list.current.scrollTop = positions.current[kind];
  }, [kind, positions]);
  const plural = labels[kind].toLowerCase();
  const needle = query.trim().toLowerCase();
  const visible = items.filter((row) =>
    (row.name + " " + (row.detail ?? "")).toLowerCase().includes(needle),
  );
  const total = items.reduce((n, row) => n + (row.added ?? 0), 0);
  return (
    <>
      {kind !== "coin" && (
        <TextField
          className={s.search}
          label={`Search ${plural}`}
          type="search"
          hideLabel
          placeholder={
            items.length ? `Search ${plural}` : "Search your collection…"
          }
          value={query}
          onValueChange={(text) => {
            if (list.current) list.current.scrollTop = 0;
            positions.current[kind] = 0;
            onQuery(text);
          }}
        />
      )}
      <div
        className={s.list}
        data-kind={kind}
        data-empty={visible.length === 0 || undefined}
        ref={list}
        onScroll={(e) => {
          positions.current[kind] = e.currentTarget.scrollTop;
        }}
      >
        {visible.length > 0 && (
          <OwnedItemPicker
            label={`Owned ${plural}`}
            mode="add"
            items={visible}
            onAdd={onAdd}
          />
        )}
        <div className={s.empty} role="status">
          {visible.length === 0 && (
            <EmptyState
              treatment="contained"
              title={
                items.length ? `No ${plural} found.` : `No ${plural} available.`
              }
            />
          )}
        </div>
      </div>
      <div role="status" aria-atomic="true" className={s.total}>
        {total > 0 && `${total} ${total === 1 ? kind : plural} added`}
      </div>
    </>
  );
}

/** Mount for one opening. The owner supplies available remainders and applied quantities. */
export function TradeItemPicker({
  items,
  onAdd,
  onClose,
  initialTab = "card",
  initialSearch = "",
  returnFocusRef,
}: TradeItemPickerProps) {
  const dialog = useRef<HTMLDialogElement>(null),
    titleId = useId();
  const [tab, setTab] = useState<Kind>(initialTab),
    [queries, setQueries] = useState({
      card: "",
      pack: "",
      coin: "",
      [initialTab]: initialSearch,
    });
  const positions = useRef({ card: 0, pack: 0, coin: 0 }),
    animate = useAllowsControlMotion();
  useLayoutEffect(() => {
    const node = dialog.current;
    if (node && !node.open) {
      node.showModal();
      node.querySelector<HTMLElement>("h2")?.focus({ preventScroll: true });
    }
  }, []);
  const tabs = (["card", "pack", "coin"] as const).map((kind) => ({
    value: kind,
    label: labels[kind],
    content: (
      <Results
        kind={kind}
        items={items.filter((row) => row.kind === kind)}
        query={queries[kind]}
        onQuery={(text) => setQueries((prev) => ({ ...prev, [kind]: text }))}
        onAdd={onAdd}
        positions={positions}
      />
    ),
  }));
  return (
    <dialog
      ref={dialog}
      className={s.dialog}
      aria-labelledby={titleId}
      onKeyDown={(event) => {
        // A search field can otherwise consume Escape to clear its value.
        if (event.key === "Escape" && !event.nativeEvent.isComposing) {
          event.preventDefault();
          event.stopPropagation();
          event.currentTarget.close();
        }
      }}
      onClose={() => {
        onClose();
        returnFocusRef?.current?.focus({ preventScroll: true });
      }}
    >
      <motion.div
        className={s.frame}
        initial={animate ? { opacity: 0.5, scale: 0.99 } : false}
        animate={{ opacity: 1, scale: 1 }}
        transition={{
          duration: animate ? controlMotion.settle : 0,
          ease: controlMotion.ease,
        }}
      >
        <header className={s.header}>
          <h2 id={titleId} tabIndex={-1}>
            Add items
          </h2>
          <ActionButton
            className={s.close}
            aria-label="Close"
            onClick={() => dialog.current?.close()}
          >
            <X aria-hidden size={designPixels(22)} />
          </ActionButton>
        </header>
        <PanelTabs
          label="Item type"
          tabs={tabs}
          value={tab}
          onValueChange={(kind) => {
            if (kind === "card" || kind === "pack" || kind === "coin")
              setTab(kind);
          }}
        />
      </motion.div>
    </dialog>
  );
}

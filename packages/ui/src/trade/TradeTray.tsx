import {
  useLayoutEffect,
  useRef,
  useState,
  type ReactNode,
  type Ref,
} from "react";
import { EmptyState } from "../feedback/EmptyState";
import { Pencil, Plus } from "lucide-react";
import { ActionButton } from "../button/ActionButton";
import { AnimatePresence, motion } from "motion/react";
import { PlayerAvatar } from "../identity/PlayerAvatar";
import { PlayerIdentity } from "../identity/PlayerIdentity";
import { controlMotion } from "../control-motion";
import { useAllowsControlMotion } from "../useAllowsControlMotion";
import { TradeItemRow, type TradeItemRowProps } from "./TradeItemRow";
import {
  TradeReadinessBadge,
  type TradeParticipantStatus,
} from "./TradeReadiness";
import * as s from "./tray.css";
import { designPixels } from "../design-pixels";

export type TradeTrayItem = Omit<TradeItemRowProps, "editor"> & { id: string };
export type TradeTrayProps = {
  portrait: ReactNode;
} & (
  | {
      kind: "offer";
      participant: string;
      status: TradeParticipantStatus;
      items: readonly TradeTrayItem[];
    }
  | {
      kind: "locked";
      status: "ready" | "confirmed";
      items: readonly TradeTrayItem[];
      onEditItems: () => void;
      editButtonRef?: Ref<HTMLButtonElement>;
    }
  | {
      kind: "receipt";
      direction: "gave" | "received";
      items: readonly TradeTrayItem[];
    }
  | {
      kind: "editable";
      compact?: boolean;
      items: readonly (TradeTrayItem & { editor: ReactNode })[];
      onAddItems: () => void;
      addButtonRef?: Ref<HTMLButtonElement>;
      ready?: { disabled: boolean; onReady: () => void };
    }
);

/** Participant offer or settlement receipt. The parent sets the tray height.
 * Stable holding IDs preserve row identity when a count or a readiness badge changes. */
export function TradeTray(props: TradeTrayProps) {
  const { portrait } = props;
  const items: readonly (TradeTrayItem & { editor?: ReactNode })[] =
    props.items.filter((item) => item.quantity > 0);
  const emptyOffer = props.kind !== "receipt" && items.length === 0;
  const title =
    props.kind === "offer"
      ? props.participant
      : props.kind === "editable" || props.kind === "locked"
        ? "You"
        : props.direction === "gave"
          ? "You gave"
          : "You received";
  const scroll = useRef<HTMLDivElement>(null),
    list = useRef<HTMLUListElement>(null);
  const [overflow, setOverflow] = useState(false);
  const animate = useAllowsControlMotion();
  useLayoutEffect(() => {
    const region = scroll.current,
      content = list.current;
    if (!region || !content) return;
    const measure = () =>
      setOverflow(region.scrollHeight > region.clientHeight + 1);
    measure();
    const observer = new ResizeObserver(measure);
    observer.observe(region);
    observer.observe(content);
    return () => observer.disconnect();
  }, []);
  return (
    <section
      className={s.tray}
      data-editable={props.kind === "editable" || undefined}
      data-locked={props.kind === "locked" || undefined}
      data-ready-action={
        (props.kind === "editable" && !!props.ready) || undefined
      }
      data-compact={(props.kind === "editable" && props.compact) || undefined}
      aria-label={
        props.kind === "editable" || props.kind === "locked"
          ? "Your offer"
          : props.kind === "offer"
            ? `Offer from ${title}`
            : title
      }
    >
      <header className={s.header}>
        <PlayerIdentity
          name={title}
          presentation="tray"
          portrait={
            <PlayerAvatar diameter={76} ring="gold">
              {portrait}
            </PlayerAvatar>
          }
        />
        {props.kind !== "receipt" && (
          <TradeReadinessBadge
            status={props.kind === "editable" ? "editing" : props.status}
          />
        )}
      </header>
      <div
        ref={scroll}
        className={s.scroll}
        tabIndex={overflow ? 0 : undefined}
        role={overflow ? "region" : undefined}
        aria-label={
          overflow
            ? props.kind === "editable"
              ? "Your items"
              : `${title} items`
            : undefined
        }
      >
        {props.kind !== "receipt" && (
          <div
            role="status"
            className={s.empty}
            data-space={emptyOffer || undefined}
          >
            {emptyOffer && (
              <EmptyState treatment="contained" title="No items added." />
            )}
          </div>
        )}
        <ul ref={list} className={s.items} aria-label="Trade items">
          <AnimatePresence initial={false}>
            {items.map((item) => (
              <motion.li
                key={item.id}
                initial={animate ? { opacity: 0.6, y: 2 } : false}
                animate={{ opacity: 1, y: 0 }}
                transition={{
                  duration: animate ? controlMotion.settle : 0,
                  ease: controlMotion.ease,
                }}
              >
                <TradeItemRow
                  {...item}
                  editor={props.kind === "editable" ? item.editor : undefined}
                />
              </motion.li>
            ))}
          </AnimatePresence>
        </ul>
      </div>
      {props.kind === "locked" && (
        <footer className={s.footer}>
          <ActionButton
            ref={props.editButtonRef ?? null}
            className={s.edit}
            aria-haspopup="dialog"
            icon={<Pencil size={designPixels(22)} aria-hidden />}
            onClick={props.onEditItems}
          >
            Edit items
          </ActionButton>
        </footer>
      )}
      {props.kind === "editable" && (
        <footer className={s.footer}>
          <ActionButton
            ref={props.addButtonRef ?? null}
            aria-haspopup="dialog"
            icon={<Plus aria-hidden size={designPixels(23)} />}
            className={s.add}
            onClick={props.onAddItems}
          >
            Add items
          </ActionButton>
          {props.ready && (
            <ActionButton
              disabled={props.ready.disabled}
              onClick={props.ready.onReady}
              className={s.ready}
              emphasis="primary"
            >
              Ready
            </ActionButton>
          )}
        </footer>
      )}
    </section>
  );
}

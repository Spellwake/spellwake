import { Check, Plus } from "lucide-react";
import { motion } from "motion/react";
import { useId, type ReactNode } from "react";
import { ActionButton } from "../button/ActionButton";
import { controlMotion } from "../control-motion";
import { useAllowsControlMotion } from "../useAllowsControlMotion";
import { ItemAvailability } from "./ItemAvailability";
import * as s from "./picker.css";
import { designPixels } from "../design-pixels";

export interface OwnedPickerItem {
  id: string;
  name: string;
  kind: "card" | "pack" | "coin";
  artwork: ReactNode;
  detail?: string;
  /** Copies still available to select or add. */
  available: number;
  added?: number;
}
export type OwnedItemPickerProps = {
  label: string;
  items: readonly OwnedPickerItem[];
} & (
  | {
      mode: "select";
      selectedId: string | null;
      onSelect: (id: string) => void;
    }
  | {
      mode: "add";
      onAdd: (id: string) => void;
    }
);

/** Single-choice listings and immediate trade additions share their row anatomy and item identities. */
export function OwnedItemPicker(props: OwnedItemPickerProps) {
  const group = useId(),
    animate = useAllowsControlMotion();
  return (
    <fieldset className={s.picker} data-mode={props.mode}>
      <legend className={s.srOnly}>{props.label}</legend>
      {props.items.map((item) => {
        const availabilityId = `${group}-availability-${encodeURIComponent(item.id)}`;
        const selected =
          props.mode === "select" &&
          props.selectedId === item.id &&
          item.available > 0;
        const content = (
          <>
            <span className={s.art} aria-hidden="true">
              {item.artwork}
            </span>
            <span className={s.identity}>
              <span className={s.name}>{item.name}</span>
              {item.detail && <span className={s.detail}>{item.detail}</span>}
            </span>
            <ItemAvailability
              id={availabilityId}
              available={item.available}
              added={props.mode === "add" ? (item.added ?? 0) : 0}
            />
          </>
        );
        if (props.mode === "add") {
          return (
            <div key={item.id} className={s.row} data-kind={item.kind}>
              {content}
              <ActionButton
                className={s.addButton}
                disabled={item.available <= 0}
                aria-describedby={availabilityId}
                aria-label={`Add ${item.name}${item.detail ? ", " + item.detail : ""}`}
                onClick={() => props.onAdd(item.id)}
              >
                <Plus
                  size={designPixels(25)}
                  strokeWidth={1.5}
                  aria-hidden="true"
                />
              </ActionButton>
            </div>
          );
        }
        return (
          <label
            className={s.row}
            key={item.id}
            data-kind={item.kind}
            data-disabled={item.available <= 0}
          >
            <input
              className={s.radio}
              type="radio"
              name={group}
              value={item.id}
              checked={selected}
              disabled={item.available <= 0}
              onChange={() => props.onSelect(item.id)}
            />
            <motion.span
              className={s.highlight}
              aria-hidden="true"
              initial={false}
              animate={{ opacity: selected ? 1 : 0 }}
              transition={{
                duration: animate ? controlMotion.settle : 0,
                ease: controlMotion.ease,
              }}
            />
            {content}
            <motion.span
              className={s.check}
              aria-hidden="true"
              initial={false}
              animate={{
                opacity: selected ? 1 : 0,
                scale: selected || !animate ? 1 : 0.9,
              }}
              transition={{
                duration: animate ? controlMotion.feedback : 0,
                ease: controlMotion.ease,
              }}
            >
              <Check size={designPixels(24)} strokeWidth={2} />
            </motion.span>
          </label>
        );
      })}
    </fieldset>
  );
}

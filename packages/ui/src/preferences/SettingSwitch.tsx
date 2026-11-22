import { motion } from "motion/react";
import { controlMotion } from "../control-motion";
import * as s from "./preferences.css";
export interface SettingSwitchProps {
  id?: string;
  label: string;
  checked: boolean;
  disabled?: boolean;
  onCheckedChange: (checked: boolean) => void;
}
/** Native two-state input for a setting whose owner applies the change. */
export function SettingSwitch({
  id,
  label,
  checked,
  disabled,
  onCheckedChange,
}: SettingSwitchProps) {
  return (
    <span className={s.switchBox}>
      <input
        id={id}
        className={s.toggle}
        type="checkbox"
        role="switch"
        aria-label={label}
        checked={checked}
        disabled={disabled}
        onChange={(event) => onCheckedChange(event.currentTarget.checked)}
      />
      <motion.span
        aria-hidden="true"
        className={s.thumbTravel}
        initial={false}
        animate={{ x: checked ? "100%" : "0%" }}
        transition={{
          type: "tween",
          duration: controlMotion.settle,
          ease: controlMotion.ease,
        }}
      />
    </span>
  );
}

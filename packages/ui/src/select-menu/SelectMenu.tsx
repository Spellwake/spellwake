import * as Select from "@radix-ui/react-select";
import { Check, ChevronDown } from "lucide-react";
import * as s from "../controls.css";
import { designPixels } from "../design-pixels";

export interface SelectOption {
  value: string;
  label: string;
  disabled?: boolean;
}
export interface SelectMenuProps {
  label: string;
  valuePrefix?: string;
  value: string;
  options: readonly SelectOption[];
  onValueChange: (value: string) => void;
  disabled?: boolean;
  defaultOpen?: boolean;
  name?: string;
}

export function SelectMenu({
  label,
  options,
  valuePrefix,
  ...props
}: SelectMenuProps) {
  return (
    <Select.Root {...props}>
      <Select.Trigger aria-label={label} className={s.trigger}>
        {valuePrefix ? (
          <span className={s.triggerLabel}>
            {valuePrefix}
            <Select.Value />
          </span>
        ) : (
          <Select.Value />
        )}
        <Select.Icon className={s.caret}>
          <ChevronDown size={designPixels(20)} aria-hidden="true" />
        </Select.Icon>
      </Select.Trigger>
      <Select.Portal>
        <Select.Content
          position="popper"
          align="start"
          sideOffset={5}
          collisionPadding={8}
          className={s.popup}
        >
          <Select.Viewport className={s.viewport}>
            {options.map((option) => (
              <Select.Item
                key={option.value}
                value={option.value}
                disabled={option.disabled ?? false}
                className={s.option}
              >
                <Select.ItemText>{option.label}</Select.ItemText>
                <Select.ItemIndicator className={s.caret}>
                  <Check size={designPixels(20)} aria-hidden="true" />
                </Select.ItemIndicator>
              </Select.Item>
            ))}
          </Select.Viewport>
        </Select.Content>
      </Select.Portal>
    </Select.Root>
  );
}

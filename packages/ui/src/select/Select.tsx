import { type ReactElement } from "react";
import { Select as RSelect } from "radix-ui";
import { Check, ChevronDown } from "lucide-react";
import * as s from "../controls.css";

export type SelectGroup = {
  label?: string;
  options: { value: string; label: string; disabled?: boolean }[];
};

export function Select({
  label,
  value,
  onValueChange,
  groups,
  placeholder = "Choose an option",
  disabled = false,
}: {
  label: string;
  value?: string;
  onValueChange?: (value: string) => void;
  groups: SelectGroup[];
  placeholder?: string;
  disabled?: boolean;
}): ReactElement {
  return (
    <RSelect.Root
      {...(value === undefined ? {} : { value })}
      {...(onValueChange ? { onValueChange } : {})}
      disabled={disabled}
    >
      <RSelect.Trigger aria-label={label} className={s.selectTrigger}>
        <RSelect.Value placeholder={placeholder} />
        <RSelect.Icon>
          <ChevronDown size={19} />
        </RSelect.Icon>
      </RSelect.Trigger>
      <RSelect.Portal>
        <RSelect.Content
          className={s.selectPopup}
          position="popper"
          sideOffset={7}
          collisionPadding={12}
        >
          <RSelect.Viewport>
            {groups.map((group, index) => (
              <RSelect.Group key={group.label ?? index}>
                {index > 0 && <RSelect.Separator className={s.separator} />}{" "}
                {group.label && (
                  <RSelect.Label className={s.menuLabel}>
                    {group.label}
                  </RSelect.Label>
                )}
                {group.options.map((option) => (
                  <RSelect.Item
                    key={option.value}
                    value={option.value}
                    disabled={option.disabled ?? false}
                    className={s.item}
                  >
                    <span className={s.itemCheck}>
                      <RSelect.ItemIndicator>
                        <Check size={16} />
                      </RSelect.ItemIndicator>
                    </span>
                    <RSelect.ItemText>{option.label}</RSelect.ItemText>
                  </RSelect.Item>
                ))}
              </RSelect.Group>
            ))}
          </RSelect.Viewport>
        </RSelect.Content>
      </RSelect.Portal>
    </RSelect.Root>
  );
}

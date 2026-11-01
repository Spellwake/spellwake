import { type ReactNode, type ReactElement } from "react";
import { Tabs } from "radix-ui";
import * as s from "../controls.css";

export function TabGroup({
  label,
  value,
  onValueChange,
  items,
}: {
  label: string;
  value: string;
  onValueChange: (value: string) => void;
  items: { value: string; label: ReactNode; content?: ReactNode }[];
}): ReactElement {
  return (
    <Tabs.Root value={value} onValueChange={onValueChange}>
      <Tabs.List aria-label={label} className={s.tabList}>
        {items.map((item) => (
          <Tabs.Trigger key={item.value} className={s.tab} value={item.value}>
            {item.label}
          </Tabs.Trigger>
        ))}
      </Tabs.List>
      {items.map(
        (item) =>
          item.content !== undefined && (
            <Tabs.Content key={item.value} value={item.value} tabIndex={0}>
              {item.content}
            </Tabs.Content>
          ),
      )}
    </Tabs.Root>
  );
}

import { type ReactNode, type ReactElement } from "react";
import { DropdownMenu } from "radix-ui";
import * as s from "../controls.css";

export type MenuEntry =
  | { kind: "label"; text: string }
  | { kind: "divider"; id: string }
  | {
      kind: "item";
      text: string;
      onSelect?: () => void;
      icon?: ReactNode;
      disabled?: boolean;
    };

export function Menu({
  trigger,
  entries,
  align = "end",
  label,
}: {
  trigger: ReactElement;
  entries: MenuEntry[];
  align?: "start" | "end";
  label: string;
}): ReactElement {
  return (
    <DropdownMenu.Root>
      <DropdownMenu.Trigger asChild>{trigger}</DropdownMenu.Trigger>
      <DropdownMenu.Portal>
        <DropdownMenu.Content
          aria-label={label}
          className={s.popup}
          align={align}
          sideOffset={8}
          collisionPadding={12}
        >
          {entries.map((entry, index) =>
            entry.kind === "label" ? (
              <DropdownMenu.Label key={index} className={s.menuLabel}>
                {entry.text}
              </DropdownMenu.Label>
            ) : entry.kind === "divider" ? (
              <DropdownMenu.Separator key={entry.id} className={s.separator} />
            ) : (
              <DropdownMenu.Item
                key={entry.text}
                disabled={entry.disabled ?? false}
                onSelect={entry.onSelect}
                className={s.item}
              >
                {entry.icon}
                {entry.text}
              </DropdownMenu.Item>
            ),
          )}
        </DropdownMenu.Content>
      </DropdownMenu.Portal>
    </DropdownMenu.Root>
  );
}

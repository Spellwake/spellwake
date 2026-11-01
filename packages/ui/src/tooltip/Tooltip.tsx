import { type ReactElement } from "react";
import { Tooltip as RTooltip } from "radix-ui";
import * as s from "../controls.css";

export function Tooltip({
  label,
  children,
}: {
  label: string;
  children: ReactElement;
}): ReactElement {
  return (
    <RTooltip.Provider delayDuration={400}>
      <RTooltip.Root>
        <RTooltip.Trigger asChild>{children}</RTooltip.Trigger>
        <RTooltip.Portal>
          <RTooltip.Content className={s.tooltip} sideOffset={7}>
            {label}
          </RTooltip.Content>
        </RTooltip.Portal>
      </RTooltip.Root>
    </RTooltip.Provider>
  );
}

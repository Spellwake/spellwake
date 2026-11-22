import { Fragment, useRef, type ReactElement, type ReactNode } from "react";
import * as Menu from "@radix-ui/react-dropdown-menu";
import { OverlayFrame, pointer } from "./OverlayFrame";
import * as s from "./overlays.css";
export interface MenuCommand {
  id: string;
  label: string;
  leading?: ReactNode;
  detail?: string;
  danger?: boolean;
  disabled?: boolean;
  current?: boolean;
  dividerBefore?: boolean;
}
export interface ActionMenuProps {
  label: string;
  trigger: ReactElement;
  commands: readonly MenuCommand[];
  onCommand: (id: string) => void;
  defaultOpen?: boolean;
  className?: string;
  alignOffset?: number;
}
/** The caller supplies commands for this context and owns their effects and confirmations. */
export function ActionMenu({
  label,
  trigger,
  commands,
  onCommand,
  defaultOpen = false,
  className = "",
  alignOffset = 0,
}: ActionMenuProps) {
  const marker = useRef<SVGSVGElement>(null);
  return (
    <Menu.Root defaultOpen={defaultOpen} modal={false}>
      <Menu.Trigger asChild>{trigger}</Menu.Trigger>
      <Menu.Portal>
        <Menu.Content
          aria-label={label}
          align="end"
          sideOffset={pointer.gap}
          alignOffset={alignOffset}
          collisionPadding={8}
          loop
          onPointerMoveCapture={(event) => {
            event.currentTarget.dataset.input = "pointer";
          }}
          onKeyDownCapture={(event) => {
            event.currentTarget.dataset.input = "keyboard";
          }}
          arrowPadding={pointer.corner}
          className={[s.menu, className].join(" ")}
        >
          <div className={s.scrollArea}>
            {commands.map((command) => (
              <Fragment key={command.id}>
                {command.dividerBefore && (
                  <Menu.Separator asChild>
                    <hr className={s.divider} />
                  </Menu.Separator>
                )}
                <Menu.Item
                  asChild
                  disabled={command.disabled}
                  textValue={command.label}
                  onSelect={() => onCommand(command.id)}
                >
                  <button
                    type="button"
                    aria-current={command.current ? "page" : undefined}
                    className={[s.item, command.danger ? s.danger : ""].join(
                      " ",
                    )}
                  >
                    {command.leading && (
                      <span aria-hidden className={s.leading}>
                        {command.leading}
                      </span>
                    )}
                    <span>
                      <span className={s.label}>{command.label}</span>
                      {command.detail && (
                        <small className={s.detail}>{command.detail}</small>
                      )}
                    </span>
                  </button>
                </Menu.Item>
              </Fragment>
            ))}
          </div>
          <Menu.Arrow asChild width={pointer.base} height={pointer.depth}>
            <svg aria-hidden="true" ref={marker} />
          </Menu.Arrow>
          <OverlayFrame marker={marker} />
        </Menu.Content>
      </Menu.Portal>
    </Menu.Root>
  );
}

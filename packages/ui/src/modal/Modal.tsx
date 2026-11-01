import { useRef, type ReactNode, type ReactElement } from "react";
import { Dialog } from "radix-ui";
import { X } from "lucide-react";
import * as s from "../controls.css";
import { IconButton } from "../icon-button/IconButton";

export function Modal({
  trigger,
  title,
  description,
  children,
  open,
  onOpenChange,
}: {
  trigger?: ReactElement;
  title: string;
  description: string;
  children: ReactNode;
  open?: boolean;
  onOpenChange?: (open: boolean) => void;
}): ReactElement {
  const content = useRef<HTMLDivElement>(null);
  const returnFocus = useRef<HTMLElement | null>(null);
  return (
    <Dialog.Root
      {...(open === undefined ? {} : { open })}
      {...(onOpenChange ? { onOpenChange } : {})}
    >
      {trigger && <Dialog.Trigger asChild>{trigger}</Dialog.Trigger>}
      <Dialog.Portal>
        <Dialog.Overlay className={s.overlay} />
        <Dialog.Content
          ref={content}
          className={s.dialog}
          onOpenAutoFocus={(event) => {
            if (document.activeElement instanceof HTMLElement)
              returnFocus.current = document.activeElement;
            const firstField = content.current?.querySelector<HTMLElement>(
              'input:not([disabled]), textarea:not([disabled]), [role="combobox"]',
            );
            if (firstField) {
              event.preventDefault();
              firstField.focus();
            }
          }}
          onCloseAutoFocus={(event) => {
            if (returnFocus.current?.isConnected) {
              event.preventDefault();
              returnFocus.current.focus();
            }
          }}
        >
          <Dialog.Title className={s.dialogTitle}>{title}</Dialog.Title>
          <Dialog.Description className={s.dialogDescription}>
            {description}
          </Dialog.Description>
          <Dialog.Close asChild>
            <IconButton
              label="Close dialog"
              variant="transparent"
              className={s.close}
            >
              <X />
            </IconButton>
          </Dialog.Close>
          {children}
        </Dialog.Content>
      </Dialog.Portal>
    </Dialog.Root>
  );
}

import {
  createContext,
  useContext,
  useEffect,
  useId,
  useRef,
  useState,
  type ReactNode,
  type RefObject,
} from "react";
import { X } from "lucide-react";
import { Drawer } from "vaul";
import * as s from "./dialog.css";

export interface DialogProps {
  open: boolean;
  title: string;
  subtitle?: string;
  /** Called for the close button, Escape, a press outside and, on a phone, a swipe down. It never chooses one of the actions. */
  onClose: () => void;
  /** Holds the dialog open while its command is in flight. */
  busy?: boolean;
  size?: "compact" | "standard" | "wide";
  /** Which end of the header holds the close button. A staged editor keeps it at the start. */
  closeSide?: "start" | "end";
  /**
   * "bare" drops the title bar: the title still names the dialog for assistive
   * technology, and the close button floats over the content's corner.
   */
  chrome?: "titled" | "bare";
  /** Sizes a dialog whose stages must share one fixed shell. */
  className?: string;
  /**
   * Where focus goes on closing when the control that opened the dialog is
   * gone, as a menu item is once its menu has closed.
   */
  returnFocusRef?: RefObject<HTMLElement | null>;
  children?: ReactNode;
  /** The dialog's actions, primary last. */
  actions?: ReactNode;
  /**
   * Dialogs this one opens, such as an editor or a confirmation. Passing them
   * here lets a phone stack them as drawers over this one.
   */
  stacked?: ReactNode;
}

export type OverlayPresentationKind = "dialog" | "drawer";
const Presentation = createContext<OverlayPresentationKind>("dialog");
const IsStacked = createContext(false);

/**
 * How dialogs below this point are shown. The host decides: a centred dialog
 * where there is room, a drawer from the bottom edge on a phone.
 */
export function OverlayPresentation({
  value,
  children,
}: {
  value: OverlayPresentationKind;
  children: ReactNode;
}) {
  return (
    <Presentation.Provider value={value}>{children}</Presentation.Provider>
  );
}
/** The presentation in force, for an overlay that is built outside `Dialog`. */
export function useOverlayPresentation(): OverlayPresentationKind {
  return useContext(Presentation);
}

/** The first control a dialog names as its start, if any is usable. */
const namedStart = (root: HTMLElement | null): HTMLElement | null =>
  root?.querySelector<HTMLElement>("[data-autofocus]:not(:disabled)") ?? null;

function CloseButton({
  title,
  disabled,
  onClick,
}: {
  title: string;
  disabled: boolean;
  onClick: () => void;
}) {
  return (
    <button
      type="button"
      className={s.close}
      aria-label={`Close ${title}`}
      disabled={disabled}
      onClick={onClick}
    >
      <X aria-hidden="true" />
    </button>
  );
}

function NativeDialog({
  title,
  subtitle,
  onClose,
  busy = false,
  size = "standard",
  closeSide = "end",
  chrome = "titled",
  className,
  returnFocusRef,
  children,
  actions,
  stacked,
}: DialogProps) {
  const id = useId();
  const dialog = useRef<HTMLDialogElement>(null);
  useEffect(() => {
    const node = dialog.current;
    if (!node) return;
    const opener = document.activeElement;
    node.showModal();
    // A dialog starts at the control it names: its first field, or its safe
    // action. With none named, it starts on its heading, so no button is
    // drawn as though it were already chosen.
    (
      namedStart(node) ?? node.querySelector<HTMLElement>("[data-dialog-title]")
    )?.focus({ preventScroll: true });
    return () => {
      node.close();
      const target =
        opener instanceof HTMLElement &&
        opener !== document.body &&
        opener.isConnected
          ? opener
          : returnFocusRef?.current;
      if (target?.isConnected) target.focus({ preventScroll: true });
    };
  }, [returnFocusRef]);
  return (
    <>
      <dialog
        ref={dialog}
        className={className ? `${s.dialog} ${className}` : s.dialog}
        data-presentation="dialog"
        data-size={size}
        data-chrome={chrome}
        aria-labelledby={id}
        onCancel={(event) => {
          event.preventDefault();
          if (!busy) onClose();
        }}
        onPointerDown={(event) => {
          if (!busy && event.target === event.currentTarget) onClose();
        }}
      >
        <header className={s.head} data-close-side={closeSide}>
          <div>
            <h2 id={id} className={s.title} tabIndex={-1} data-dialog-title="">
              {title}
            </h2>
            {subtitle && <p className={s.subtitle}>{subtitle}</p>}
          </div>
          <CloseButton title={title} disabled={busy} onClick={onClose} />
        </header>
        {children !== undefined && (
          <div className={s.body} data-dialog-body="">
            {children}
          </div>
        )}
        {actions && <footer className={s.foot}>{actions}</footer>}
      </dialog>
      {stacked}
    </>
  );
}

/**
 * The same dialog as a drawer from the bottom edge, for a phone. It can be
 * swiped down to dismiss, which is the same safe outcome as Close.
 */
function DrawerDialog({
  title,
  subtitle,
  onClose,
  busy = false,
  size = "standard",
  closeSide = "end",
  chrome = "titled",
  className,
  returnFocusRef,
  children,
  actions,
  stacked,
}: DialogProps) {
  const isStacked = useContext(IsStacked);
  const Root = isStacked ? Drawer.NestedRoot : Drawer.Root;
  // The drawer leaves by itself before its owner is told, so the owner can
  // simply stop rendering it without cutting the motion short.
  const [isShown, setIsShown] = useState(true);
  const content = useRef<HTMLDivElement>(null);
  const [opener] = useState(() => document.activeElement);
  return (
    <Root
      open={isShown}
      dismissible={!busy}
      onOpenChange={(next) => {
        if (!next && !busy) setIsShown(false);
      }}
      onAnimationEnd={(isOpen) => {
        if (!isOpen) onClose();
      }}
    >
      <Drawer.Portal>
        <Drawer.Overlay className={s.drawerOverlay} />
        <Drawer.Content
          ref={content}
          className={className ? `${s.drawer} ${className}` : s.drawer}
          data-presentation="drawer"
          data-size={size}
          data-chrome={chrome}
          // The description is optional; the title always names the drawer.
          aria-describedby={undefined}
          onOpenAutoFocus={(event) => {
            // A field is not focused on arrival: that would raise the keyboard
            // over the drawer before it has been read.
            const start = namedStart(content.current);
            event.preventDefault();
            (start && start.tagName !== "INPUT"
              ? start
              : content.current
            )?.focus({ preventScroll: true });
          }}
          onCloseAutoFocus={(event) => {
            const isOpenerGone =
              !(opener instanceof HTMLElement) ||
              opener === document.body ||
              !opener.isConnected;
            if (!isOpenerGone || !returnFocusRef?.current) return;
            event.preventDefault();
            returnFocusRef.current.focus({ preventScroll: true });
          }}
        >
          <span aria-hidden="true" className={s.drawerHandle} />
          <header className={s.head} data-close-side={closeSide}>
            <div>
              <Drawer.Title className={s.title}>{title}</Drawer.Title>
              {subtitle && (
                <Drawer.Description className={s.subtitle}>
                  {subtitle}
                </Drawer.Description>
              )}
            </div>
            <CloseButton
              title={title}
              disabled={busy}
              onClick={() => setIsShown(false)}
            />
          </header>
          {children !== undefined && (
            <div className={s.body} data-dialog-body="">
              {children}
            </div>
          )}
          {actions && <footer className={s.foot}>{actions}</footer>}
        </Drawer.Content>
      </Drawer.Portal>
      <IsStacked.Provider value={true}>{stacked}</IsStacked.Provider>
    </Root>
  );
}

/**
 * A modal task or decision. Dismissing it is always the safe outcome: the
 * owner decides what each action does, and closing is never one of them.
 * Focus returns to the control that opened it. Under a drawer presentation it
 * rises from the bottom edge instead of sitting in the middle of the screen.
 */
export function Dialog(props: DialogProps) {
  const presentation = useContext(Presentation);
  if (!props.open) return null;
  return presentation === "drawer" ? (
    <DrawerDialog {...props} />
  ) : (
    <NativeDialog {...props} />
  );
}

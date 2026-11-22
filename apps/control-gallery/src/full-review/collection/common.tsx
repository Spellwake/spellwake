import { cardNames } from "./card-inventory";
import {
  useEffect,
  useId,
  useLayoutEffect,
  useRef,
  useState,
  type ReactNode,
} from "react";
import { ActionButton, PanelTabs, TextField } from "@spellwake/ui";
import { ChevronDown, X } from "lucide-react";
import { findScreen } from "../catalogue";
import type { ScreenProps } from "../../review-types";
import { decks } from "./deck-library";

export type Jump = ScreenProps["go"];
export type Rect = readonly [number, number, number, number];
/** Only the individual inventory object's art is sampled. Titles, counts, menus and frame are DOM. */
export function Art({
  source,
  rect,
  alt = "",
  className = "",
}: {
  source: string;
  rect: Rect;
  alt?: string;
  className?: string;
}) {
  const page = findScreen(source)?.view;
  return page ? (
    <svg
      className={"cs-art " + className}
      viewBox={rect.join(" ")}
      role={alt ? "img" : undefined}
      aria-label={alt || undefined}
      aria-hidden={!alt}
      preserveAspectRatio="xMidYMid meet"
    >
      <svg
        x={rect[0]}
        y={rect[1]}
        width={rect[2]}
        height={rect[3]}
        viewBox={rect.join(" ")}
        overflow="hidden"
      >
        <image href={page.image} width={page.width} height={page.height} />
      </svg>
    </svg>
  ) : null;
}
export function DeckArt({ index, name }: { index: number; name?: string }) {
  if (index < 0) return <span className="cs-deck-art" aria-hidden="true" />;
  const x = [396, 787, 1190][index % 3] ?? 396,
    y = index % 6 < 3 ? 217 : 559;
  return (
    <Art
      source="DL-01"
      rect={[x, y, 203, 247]}
      className="cs-deck-art"
      alt={name ?? decks[index % decks.length]?.name ?? "Deck"}
    />
  );
}
export { cardNames } from "./card-inventory";
export function CardArt({
  index,
  alternate = false,
}: {
  index: number;
  alternate?: boolean;
}) {
  const x = [263, 503, 737, 974][index % 4] ?? 263;
  const rc: Rect = [x + 4, index % 8 < 4 ? 208 : 580, 189, 251];
  return (
    <Art
      source={alternate ? "CLC-02" : "CLC-01"}
      rect={rc}
      alt={cardNames[index % 8] ?? "Card"}
    />
  );
}
export const packNames = [
  "Foundations",
  "Modern Horizons 3",
  "Bloomburrow",
  "Duskmourn",
];
export function PackArt({ index }: { index: number }) {
  return (
    <Art
      source="CP-01"
      rect={
        [
          [319, 209, 242, 422],
          [627, 209, 245, 422],
          [937, 209, 244, 422],
          [1248, 209, 244, 422],
        ][index % 4] as unknown as Rect
      }
      alt={packNames[index % 4] ?? "Pack"}
    />
  );
}
export function Btn({
  children,
  onClick,
  disabled = false,
  primary = false,
  label,
}: {
  children: ReactNode;
  onClick?: () => void;
  disabled?: boolean;
  primary?: boolean;
  label?: string;
}) {
  return (
    <ActionButton
      aria-label={label}
      treatment="compact-form"
      emphasis={primary ? "primary" : "neutral"}
      disabled={disabled}
      onClick={onClick}
    >
      {children}
    </ActionButton>
  );
}
export function Search({
  label,
  value,
  onChange,
}: {
  label: string;
  value: string;
  onChange: (s: string) => void;
}) {
  return (
    <TextField
      type="search"
      label={label}
      placeholder={label + "…"}
      value={value}
      onValueChange={onChange}
      hideLabel
      treatment="compact-form"
    />
  );
}
export function Choice({
  value,
  items,
  change,
  label,
  open = false,
}: {
  value: string;
  items: readonly string[];
  change: (v: string) => void;
  label?: string;
  open?: boolean;
}) {
  const [shown, set] = useState(open);
  const ref = useRef<HTMLDivElement>(null);
  useEffect(() => {
    function close(e: PointerEvent) {
      if (ref.current && !ref.current.contains(e.target as Node)) set(false);
    }
    document.addEventListener("pointerdown", close);
    return () => document.removeEventListener("pointerdown", close);
  }, []);
  return (
    <div
      className="cs-choice"
      ref={ref}
      onKeyDown={(e) => {
        if (e.key === "Escape") {
          set(false);
          ref.current?.querySelector("button")?.focus();
        }
      }}
    >
      <button
        aria-label={label ?? value}
        aria-expanded={shown}
        onClick={() => set(!shown)}
      >
        {value}
        <ChevronDown size={17} />
      </button>
      {shown && (
        <div className="cs-options">
          {items.map((item) => (
            <button
              key={item}
              aria-current={item === value ? true : undefined}
              onClick={() => {
                change(item);
                set(false);
              }}
            >
              {item}
            </button>
          ))}
        </div>
      )}
    </div>
  );
}
export function Tabs({
  labels,
  value,
  change,
}: {
  labels: readonly string[];
  value: string;
  change: (v: string) => void;
}) {
  return (
    <div className="cs-tabs">
      <PanelTabs
        label="Panels"
        value={value}
        onValueChange={change}
        tabs={labels.map((label) => ({ value: label, label, content: null }))}
      />
    </div>
  );
}
export function Modal({
  title,
  subtitle,
  busy = false,
  close,
  children,
  footer,
  className = "",
}: {
  title: string;
  subtitle?: string;
  busy?: boolean;
  close?: () => void;
  children: ReactNode;
  footer?: ReactNode;
  className?: string;
}) {
  const id = useId(),
    dialog = useRef<HTMLDialogElement>(null);
  useLayoutEffect(() => {
    const node = dialog.current;
    if (!node) return;
    const previous = document.activeElement;
    const embedded = Boolean(node.closest(".cs-example"));
    if (embedded) node.show();
    else node.showModal();
    return () => {
      node.close();
      if (previous instanceof HTMLElement && previous.isConnected && !embedded)
        previous.focus({ preventScroll: true });
    };
  }, []);
  return (
    <dialog
      ref={dialog}
      className={"cs-overlay " + className}
      aria-labelledby={id}
      onCancel={(e) => {
        e.preventDefault();
        if (!busy) close?.();
      }}
      onMouseDown={(e) => {
        if (!busy && e.target === e.currentTarget) close?.();
      }}
    >
      <section className="cs-dialog">
        <header>
          <div>
            <h2 id={id}>{title}</h2>
            {subtitle && <small>{subtitle}</small>}
          </div>
          {close && (
            <button
              disabled={busy}
              aria-label={"Close " + title}
              onClick={close}
            >
              <X />
            </button>
          )}
        </header>
        <div className="cs-dialog-body">{children}</div>
        {footer && <footer>{footer}</footer>}
      </section>
    </dialog>
  );
}
/** Reference boards show independent, scaled examples with their original scene dimensions. */
export function Example({
  title,
  children,
  width = 660,
  height = 350,
}: {
  title: string;
  children: ReactNode;
  width?: number;
  height?: number;
}) {
  const host = useRef<HTMLDivElement>(null);
  const [scale, setScale] = useState(1);
  useLayoutEffect(() => {
    const el = host.current;
    if (!el) return;
    function update() {
      if (el)
        setScale(Math.min(el.clientWidth / width, el.clientHeight / height));
    }
    update();
    const observer = new ResizeObserver(update);
    observer.observe(el);
    return () => observer.disconnect();
  }, [width, height]);
  return (
    <section className="cs-example">
      <h3>{title}</h3>
      <div ref={host} className="cs-example-host">
        <div
          className="cs-example-body"
          style={{
            width,
            height,
            transform: "translate(-50%,-50%) scale(" + scale + ")",
          }}
        >
          {children}
        </div>
      </div>
    </section>
  );
}
export function Board({
  children,
  columns = 2,
  title,
}: {
  children: ReactNode;
  columns?: number;
  title?: string;
}) {
  return (
    <div className="cs-board">
      {title && <h2>{title}</h2>}
      <div
        className="cs-board-grid"
        style={{ gridTemplateColumns: "repeat(" + columns + ",minmax(0,1fr))" }}
      >
        {children}
      </div>
    </div>
  );
}

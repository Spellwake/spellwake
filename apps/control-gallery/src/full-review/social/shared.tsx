import { useLayoutEffect, useRef, type ReactNode } from "react";
import {
  ActionButton,
  CoinAmount,
  LoadingShape,
  LoadingState,
  PlayerAvatar,
  TextField,
} from "@spellwake/ui";
import { X } from "lucide-react";
import { SectionRail, type RailArea } from "../SectionRail";
import { ReviewHeader } from "../ReviewHeader";
import coin from "../../../../../assets/ui-study/coin.svg?inline";

export type Go = (id: string) => void;
export const art: Record<string, string> = {};
for (const [path, src] of Object.entries(
  import.meta.glob<string>(
    "../../../../../.impeccable/review/open-play/approved-screens/implementation/social-screen-art/*.webp",
    { eager: true, query: "?inline", import: "default" },
  ),
))
  art[path.split("/").at(-1)!.split(".")[0]!] = src;
export { coin };
export function Coins({ n }: { n: number | null }) {
  return <CoinAmount amount={n} iconSrc={coin} />;
}
export function Avatar({ name, size = 58 }: { name: string; size?: number }) {
  return (
    <PlayerAvatar diameter={size} ring="gold">
      <img src={art[name.toLowerCase()] ?? art.you} alt="" />
    </PlayerAvatar>
  );
}
export function Btn({
  children,
  onClick,
  disabled = false,
  danger = false,
}: {
  children: ReactNode;
  onClick?: () => void;
  disabled?: boolean;
  danger?: boolean;
}) {
  return (
    <ActionButton
      className={danger ? "soc-danger" : ""}
      onClick={onClick}
      disabled={disabled}
    >
      {children}
    </ActionButton>
  );
}
export function Search({
  value,
  set,
  label,
}: {
  value: string;
  set: (s: string) => void;
  label: string;
}) {
  return (
    <TextField
      type="search"
      hideLabel
      label={label}
      value={value}
      onValueChange={set}
      placeholder={label + "…"}
    />
  );
}
export function Skeleton({ count = 3 }: { count?: number }) {
  return (
    <LoadingState label="Loading">
      <div className="soc-skeleton">
        {Array.from({ length: count }, (_, i) => (
          <div key={i}>
            <LoadingShape width={48} height={40} />
            <LoadingShape width="45%" height={15} />
            <LoadingShape width="18%" height={15} />
          </div>
        ))}
      </div>
    </LoadingState>
  );
}
export function Notice({ children }: { children: ReactNode }) {
  return (
    <p className="soc-notice" role="status">
      {children}
    </p>
  );
}
export function Shell({
  area,
  current,
  go,
  children,
  balance = 8000,
}: {
  area: RailArea;
  current: string;
  go: Go;
  children: ReactNode;
  balance?: number;
}) {
  return (
    <div className="soc-shell">
      <ReviewHeader active={area} go={go} coins={balance} />
      <div className="soc-page">
        <SectionRail area={area} current={current} go={go} />
        <div className="soc-main">{children}</div>
      </div>
    </div>
  );
}
export function Window({
  title,
  children,
  close,
  wide = false,
  embedded = false,
}: {
  title: string;
  children: ReactNode;
  close: () => void;
  wide?: boolean;
  embedded?: boolean;
}) {
  const ref = useRef<HTMLDialogElement>(null);
  const closeRef = useRef(close);
  closeRef.current = close;
  useLayoutEffect(() => {
    const d = ref.current;
    if (!d) return;
    d.showModal();
    return () => {
      d.close();
    };
  }, []);
  const inner = (
    <>
      <div className="soc-window-title">
        <h2>{title}</h2>
        <button
          type="button"
          className="soc-x"
          aria-label={"Close " + title}
          onClick={close}
        >
          <X />
        </button>
      </div>
      {children}
    </>
  );
  if (embedded)
    return (
      <section aria-label={title} className="soc-window soc-embedded">
        {inner}
      </section>
    );
  return (
    <dialog
      ref={ref}
      aria-label={title}
      className={"soc-window soc-modal" + (wide ? " wide" : "")}
      onCancel={(e) => {
        e.preventDefault();
        closeRef.current();
      }}
      onClick={(e) => {
        const d = e.currentTarget,
          r = d.getBoundingClientRect();
        if (
          e.target === d &&
          (e.clientX < r.left ||
            e.clientX > r.right ||
            e.clientY < r.top ||
            e.clientY > r.bottom)
        )
          closeRef.current();
      }}
    >
      {inner}
    </dialog>
  );
}
/** Scale reference-board specimens as a group so every independent state fits the native frame. */
export function Board({
  title,
  panels,
  cols = 2,
  scenic = false,
}: {
  title: string;
  panels: readonly [string, ReactNode][];
  cols?: number;
  scenic?: boolean;
}) {
  const canvas = useRef<HTMLDivElement>(null),
    grid = useRef<HTMLDivElement>(null);
  useLayoutEffect(() => {
    const host = canvas.current,
      body = grid.current;
    if (!host || !body) return;
    let active = true;
    function fit() {
      if (!active || !host || !body) return;
      const available = host.clientHeight;
      if (!available) return;
      let low = 0.2,
        high = 1;
      function size(factor: number) {
        body!.style.width = host!.clientWidth / factor + "px";
        body!.style.transform = "scale(" + factor + ")";
      }
      size(1);
      if (body.scrollHeight <= available) return;
      for (let i = 0; i < 9; i++) {
        const middle = (low + high) / 2;
        size(middle);
        if (body.scrollHeight * middle <= available) low = middle;
        else high = middle;
      }
      size(low);
    }
    const observer = new ResizeObserver(fit);
    observer.observe(host);
    fit();
    void document.fonts.ready.then(fit);
    return () => {
      active = false;
      observer.disconnect();
    };
  }, [cols, panels.length]);
  return (
    <section className={"soc-board" + (scenic ? " scenic" : "")}>
      {title && <h1>{title}</h1>}
      <div className="soc-board-canvas" ref={canvas}>
        <div
          className="soc-board-grid"
          ref={grid}
          style={{ gridTemplateColumns: "repeat(" + cols + ",minmax(0,1fr))" }}
        >
          {panels.map(([name, node]) => (
            <section key={name} className="soc-spec">
              <h2>{name}</h2>
              {node}
            </section>
          ))}
        </div>
      </div>
    </section>
  );
}

import { useLayoutEffect, useRef, useState, type ReactNode } from "react";
export type Area = [number, number, number, number];
export function Artboard({
  width,
  height,
  children,
}: {
  width: number;
  height: number;
  children: ReactNode;
}) {
  const host = useRef<HTMLDivElement>(null),
    [scale, set] = useState(1);
  useLayoutEffect(() => {
    const el = host.current;
    if (!el) return;
    const fit = () =>
      set(Math.min(el.clientWidth / width, el.clientHeight / height));
    fit();
    const ob = new ResizeObserver(fit);
    ob.observe(el);
    return () => ob.disconnect();
  }, [width, height]);
  return (
    <div ref={host} className="soc-artboard">
      <div style={{ width, height, transform: "scale(" + scale + ")" }}>
        {children}
      </div>
    </div>
  );
}
export function Box({
  area,
  children,
  scale = 1,
  frame = false,
  className = "",
}: {
  area: Area;
  children: ReactNode;
  scale?: number;
  frame?: boolean;
  className?: string;
}) {
  const [x, y, w, h] = area;
  const inner = useRef<HTMLDivElement>(null);
  useLayoutEffect(() => {
    const body = inner.current;
    const host = body?.parentElement;
    if (!body || !host || h < 100) return;
    let active = true;
    function measure() {
      if (!active || !body || !host) return;
      const fit = (k: number) => {
        body.style.width = host.clientWidth / k + "px";
        body.style.height = host.clientHeight / k + "px";
        body.style.transform = "scale(" + k + ")";
        return body.scrollHeight * k <= host.clientHeight + 1;
      };
      if (fit(scale)) return;
      let low = scale / 2,
        high = scale;
      for (let i = 0; i < 9; i++) {
        const mid = (low + high) / 2;
        if (fit(mid)) low = mid;
        else high = mid;
      }
      fit(low);
    }
    const observer = new ResizeObserver(measure);
    observer.observe(host);
    measure();
    void document.fonts.ready.then(measure);
    return () => {
      active = false;
      observer.disconnect();
    };
  }, [scale, h, w]);
  return (
    <section
      className={"soc-place " + (frame ? " framed " : "") + className}
      style={{ left: x, top: y, width: w, height: h }}
    >
      <div
        ref={inner}
        style={{
          width: w / scale,
          height: h / scale,
          transform: "scale(" + scale + ")",
        }}
      >
        {children}
      </div>
    </section>
  );
}

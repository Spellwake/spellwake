import { useEffect, useRef, type ReactElement } from "react";
import { assignInlineVars } from "@vanilla-extract/dynamic";
import { paletteFor, productKind } from "./pack-palette";
import { usePackStill, useSetSymbol } from "./pack-assets";
import * as s from "./packs.css";

/**
 * A sealed pack on the shelf: a still of the same 3D wrapper the opening
 * lifts and tears, carrying the set's official symbol. Until the still is
 * ready, or where 3D is unavailable, a drawn wrapper in the same colours
 * stands in. It is decoration; the caption names the pack.
 */
export function PackWrapper({
  setName,
  product,
  setCode,
}: {
  setName: string;
  product: string;
  /** Finds the set's symbol; without it the set's name is matched instead. */
  setCode?: string | null;
}): ReactElement {
  const symbol = useSetSymbol(setCode ?? null, setName);
  const still = usePackStill(setName, product, symbol);
  return (
    <span className={s.packArt} aria-hidden="true" data-pack-wrapper="">
      {still ? (
        <>
          <StillCanvas still={still} />
          <span className={s.stillSheen} />
        </>
      ) : (
        <DrawnWrapper setName={setName} product={product} symbol={symbol.url} />
      )}
    </span>
  );
}

/** The 3D still, painted into a canvas at its rendered size. */
function StillCanvas({ still }: { still: ImageBitmap }): ReactElement {
  const canvas = useRef<HTMLCanvasElement>(null);
  useEffect(() => {
    const element = canvas.current;
    const context = element?.getContext("2d");
    if (!element || !context) return;
    context.clearRect(0, 0, element.width, element.height);
    context.drawImage(still, 0, 0);
  }, [still]);
  return (
    <canvas
      ref={canvas}
      className={s.still}
      width={still.width}
      height={still.height}
    />
  );
}

/** The wrapper in CSS: set colours, crimped ends, the set symbol and the label. */
function DrawnWrapper({
  setName,
  product,
  symbol,
}: {
  setName: string;
  product: string;
  symbol: string | null;
}): ReactElement {
  const palette = paletteFor(setName);
  const kind = productKind(setName, product);
  return (
    <span
      className={s.wrapper}
      style={assignInlineVars({
        [s.hue]: palette.hue,
        [s.deep]: palette.deep,
        [s.glow]: palette.glow,
      })}
    >
      <span className={s.emblem} />
      <span className={s.sheen} />
      {symbol && (
        <span
          className={s.symbol}
          style={{
            maskImage: `url("${symbol}")`,
            WebkitMaskImage: `url("${symbol}")`,
          }}
        />
      )}
      <span className={s.label}>
        <span className={s.labelSet}>{setName}</span>
        <span className={s.labelRule} />
        {kind && <span className={s.labelKind}>{kind}</span>}
      </span>
    </span>
  );
}

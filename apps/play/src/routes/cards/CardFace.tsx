import { useEffect, useState, type ReactElement } from "react";
import { ImageOff } from "lucide-react";
import { CardsIcon } from "@spellwake/ui";
import * as s from "./cards.css";

/** Total tries at loading one face, the first included. */
const ART_ATTEMPTS = 3;

export type ArtState = "none" | "loading" | "loaded" | "failed";

/**
 * A card face at printed proportions. A face that fails to load is tried
 * again, three tries in all. Where there is no artwork, or it never loads, a
 * quiet card-sized frame keeps the card's name. Artwork is never invented.
 */
export function CardFace({
  url,
  name,
  reportStatus = false,
  onStateChange,
  loading = "lazy",
}: {
  url: string | null;
  name: string;
  /** Says "Loading artwork…" over the frame while the face loads, as the inspector does. */
  reportStatus?: boolean;
  onStateChange?: (state: ArtState) => void;
  loading?: "lazy" | "eager";
}): ReactElement {
  const [attempt, setAttempt] = useState(0);
  const [state, setState] = useState<ArtState>(url ? "loading" : "none");
  useEffect(() => {
    setAttempt(0);
    setState(url ? "loading" : "none");
  }, [url]);
  useEffect(() => {
    onStateChange?.(state);
  }, [state, onStateChange]);
  const source =
    url && attempt > 0
      ? `${url}${url.includes("?") ? "&" : "?"}try=${attempt}`
      : url;
  return (
    <span className={s.face} data-card-face="">
      {source && state !== "failed" && (
        <img
          key={source}
          className={s.faceImage}
          src={source}
          alt={name}
          loading={loading}
          decoding="async"
          draggable={false}
          onLoad={() => setState("loaded")}
          onError={() => {
            if (attempt + 1 < ART_ATTEMPTS)
              window.setTimeout(() => setAttempt((n) => n + 1), 400);
            else setState("failed");
          }}
        />
      )}
      {(state === "none" || state === "failed") && (
        <span className={s.placeholder} role="img" aria-label={name}>
          {state === "failed" ? (
            <ImageOff className={s.placeholderIcon} aria-hidden="true" />
          ) : (
            <CardsIcon className={s.placeholderIcon} aria-hidden="true" />
          )}
          <span className={s.placeholderName}>{name}</span>
        </span>
      )}
      {reportStatus && state === "loading" && (
        <span className={s.artStatus} role="status">
          <span className={s.smallSpinner} aria-hidden="true" />
          Loading artwork…
        </span>
      )}
    </span>
  );
}

import {
  lazy,
  Suspense,
  useEffect,
  useLayoutEffect,
  useRef,
  useState,
  type CSSProperties,
} from "react";
import { ArrowLeft, ArrowRight, Maximize2 } from "lucide-react";
import { findScreen, screens, sections } from "./catalogue";
import "./review.css";

const RevisedDamage = lazy(() => import("./RevisedDamage"));
const renderers = {
  trade: lazy(() => import("./trade")),
  match: lazy(() => import("./match")),
  collection: lazy(() => import("./collection")),
  social: lazy(() => import("./social")),
};
function screenFamily(section: string) {
  if (section === "trade") return "trade";
  if (section.startsWith("in-game-") || section === "x-value") return "match";
  if (
    [
      "messaging",
      "market",
      "market-supply",
      "market-predictions",
      "league",
      "public-profile",
      "account-entry",
    ].includes(section)
  )
    return "social";
  return "collection";
}
const params = () => new URLSearchParams(location.search);
function linkTo(screen: string, frame = false) {
  return (
    "?" +
    new URLSearchParams({ [frame ? "scene" : "review"]: screen }).toString()
  );
}
export function ReviewScene() {
  const entry = findScreen(params().get("scene"));
  useEffect(() => {
    if (window.parent !== window) return;
    // A standalone scene pushes its destination before reloading. Traversing those
    // history entries can change the URL without replacing the mounted scene.
    const restore = () => {
      if (params().get("scene") !== entry?.view.id) location.reload();
    };
    window.addEventListener("popstate", restore);
    return () => window.removeEventListener("popstate", restore);
  }, [entry]);
  if (!entry) return <p>Screen unavailable.</p>;
  const Renderer = renderers[screenFamily(entry.section.id)];
  function go(id: string, data?: unknown) {
    if (!findScreen(id)) return;
    if (parent !== window)
      parent.postMessage(
        { type: "spellwake:review-navigate", id, data },
        location.origin,
      );
    else {
      history.pushState({ screenData: data }, "", linkTo(id, true));
      location.reload();
    }
  }
  return (
    <main
      className="sw-native-screen"
      data-screen={entry.view.id}
      data-section={entry.section.id}
      style={{ width: entry.view.width, height: entry.view.height }}
    >
      <Suspense fallback={<div aria-busy="true" />}>
        <Renderer
          section={entry.section.id}
          view={entry.view}
          go={go}
          data={window.parent.history.state?.screenData}
        />
      </Suspense>
    </main>
  );
}
type Mode = "Live" | "Compare" | "Reference";
export function FullReview() {
  const start = () => findScreen(params().get("review")) ?? screens[0]!;
  const [entry, setEntry] = useState(start),
    [mode, setMode] = useState<Mode>("Live");
  const frame = useRef<HTMLIFrameElement>(null);
  const livePane = useRef<HTMLDivElement>(null),
    [scale, setScale] = useState(1);
  const { view, section } = entry,
    index = screens.indexOf(entry);
  function navigate(id: string, data?: unknown) {
    const next = findScreen(id);
    if (!next || next === entry) return;
    history.pushState({ screenData: data }, "", linkTo(id));
    setEntry(next);
  }
  useEffect(() => {
    const pop = () => setEntry(start());
    const received = (event: MessageEvent) => {
      if (
        event.origin === location.origin &&
        event.source === frame.current?.contentWindow &&
        event.data?.type === "spellwake:review-navigate" &&
        typeof event.data.id === "string"
      )
        navigate(event.data.id, event.data.data);
    };
    window.addEventListener("popstate", pop);
    window.addEventListener("message", received);
    document.title = view.title + " · Spellwake screen review";
    return () => {
      window.removeEventListener("popstate", pop);
      window.removeEventListener("message", received);
    };
  }, [entry]);
  useLayoutEffect(() => {
    const el = livePane.current;
    if (!el) return;
    const measure = () => {
      if (el.clientWidth) setScale(el.clientWidth / view.width);
    };
    measure();
    const observer = new ResizeObserver(measure);
    observer.observe(el);
    return () => observer.disconnect();
  }, [view, mode]);
  return (
    <main className="sw-book">
      <header className="sw-book-head">
        <h1>Screen review</h1>
        <a href="../index.html">All approved images</a>
      </header>
      <nav className="sw-book-nav" aria-label="Review screens">
        <label>
          Section
          <select
            aria-label="Section"
            value={section.id}
            onChange={(e) => {
              const first = sections.find((s) => s.id === e.target.value)
                ?.views[0];
              if (first) navigate(first.id);
            }}
          >
            {sections.map((s) => (
              <option key={s.id} value={s.id}>
                {s.title} ({s.views.length})
              </option>
            ))}
          </select>
        </label>
        <label>
          Screen
          <select
            aria-label="Screen"
            value={view.id}
            onChange={(e) => navigate(e.target.value)}
          >
            {section.views.map((s) => (
              <option key={s.id} value={s.id}>
                {s.title}
              </option>
            ))}
          </select>
        </label>
        <div className="sw-paging">
          <button
            aria-label="Previous screen"
            disabled={index === 0}
            onClick={() => navigate(screens[index - 1]!.view.id)}
          >
            <ArrowLeft />
          </button>
          <span>
            {index + 1} / {screens.length}
          </span>
          <button
            aria-label="Next screen"
            disabled={index === screens.length - 1}
            onClick={() => navigate(screens[index + 1]!.view.id)}
          >
            <ArrowRight />
          </button>
        </div>
      </nav>
      <div
        className="sw-book-stage"
        style={
          { "--sw-screen-ratio": view.width / view.height } as CSSProperties
        }
      >
        <div className="sw-book-toolbar">
          <h2>{view.title}</h2>
          <div role="group" aria-label="Review display">
            {(["Live", "Compare", "Reference"] as const).map((m) => (
              <button
                aria-pressed={mode === m}
                key={m}
                onClick={() => setMode(m)}
              >
                {m}
              </button>
            ))}
          </div>
          <a
            href={linkTo(view.id, true)}
            target="_blank"
            rel="noreferrer"
            aria-label="Open live screen at native size"
          >
            <Maximize2 size={17} />
          </a>
        </div>
        <div className="sw-canvases" data-mode={mode}>
          <figure hidden={mode === "Reference"}>
            <figcaption>Live screen</figcaption>
            <div
              ref={livePane}
              style={{ aspectRatio: view.width + "/" + view.height }}
            >
              <iframe
                ref={frame}
                key={view.id}
                title={view.title + " – live screen"}
                src={linkTo(view.id, true)}
                style={{
                  width: view.width,
                  height: view.height,
                  transform: "scale(" + scale + ")",
                }}
              />
            </div>
          </figure>
          {mode !== "Live" && (
            <figure>
              <figcaption>Approved reference</figcaption>
              <a href={view.image} target="_blank" rel="noreferrer">
                <img
                  src={view.image}
                  width={view.width}
                  height={view.height}
                  alt={view.title + " – approved reference"}
                  fetchPriority="high"
                />
              </a>
            </figure>
          )}
        </div>
        {/^DA0(?:[1-6]|2C)$/.test(view.id) && (
          <Suspense fallback={null}>
            <RevisedDamage />
          </Suspense>
        )}
      </div>
    </main>
  );
}

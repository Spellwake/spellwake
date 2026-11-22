import { useEffect, useId, useRef, useState } from "react";
import { TextField } from "@spellwake/ui";
import { Board, Btn, Example, Modal, Tabs, type Jump } from "./common";
import { DeckLibrary } from "./Library";
import sample from "../../../../../.impeccable/review/open-play/import-deck/accepted-paste-example.txt?raw";
import { createDeck, type ShelfContext } from "./deck-library";
import { supportedLink, resolveImport, FileFormatError } from "./import-source";
const modes = ["Paste list", "Link", "File"];
const defaultOrigin: ShelfContext = {
  source: "DL-01",
  query: "",
  formatFilter: "All formats",
  scrollTop: 0,
};
const statusText: Record<string, string> = {
  Processing: "Importing deck…",
  "Exhausted failure": "Couldn’t import deck.",
  "Link unavailable": "Couldn’t open link. Paste or upload a list.",
  "Unconfirmed outcome": "Couldn’t confirm import.",
};
function ImportForm({
  initial,
  go,
  scenario = "",
  origin = defaultOrigin,
  fixture = false,
}: {
  initial: string;
  go: Jump;
  scenario?: string;
  origin?: ShelfContext;
  fixture?: boolean;
}) {
  const [mode, setMode] = useState(initial);
  const [paste, setPaste] = useState(
    (fixture || scenario) && scenario !== "Empty paste" ? sample.trim() : "",
  );
  const [url, setUrl] = useState(
    scenario === "Unsupported link"
      ? "https://example.com/deck"
      : fixture || scenario
        ? "https://moxfield.com/decks/example"
        : "",
  );
  const [file, setFile] = useState<File | undefined>(() =>
    scenario === "Selected file"
      ? new File(
          [
            '<cockatrice_deck version="1"><zone name="main"><card number="1" name="Forest"/></zone></cockatrice_deck>',
          ],
          "my-deck.cod",
        )
      : undefined,
  );
  const fileMessage = "Choose a .txt, .dek, or .cod file.";
  const [error, setError] = useState(
    scenario === "Unsupported file" ? fileMessage : "",
  );
  const [notice, setNotice] = useState(statusText[scenario] ?? "");
  const [working, setWorking] = useState(false);
  const active = useRef(false),
    mounted = useRef(true),
    fileInput = useRef<HTMLInputElement>(null),
    title = useId();
  useEffect(() => {
    mounted.current = true;
    return () => {
      mounted.current = false;
    };
  }, []);
  useEffect(() => {
    if (!notice || working || statusText[scenario] === notice) return;
    const timer = setTimeout(() => setNotice(""), 5500);
    return () => clearTimeout(timer);
  }, [notice, working, scenario]);
  const pending = working || scenario === "Processing";
  const invalidLink = !!url.trim() && !supportedLink(url);
  const accept = (f?: File) => {
    if (!f || active.current || pending) return;
    setFile(f);
    setNotice("");
    setError(/\.(txt|dek|cod)$/i.test(f.name) ? "" : fileMessage);
    if (fileInput.current) fileInput.current.value = "";
  };
  const disabled =
    scenario === "Unconfirmed outcome" ||
    pending ||
    (mode === "Paste list"
      ? !paste.trim()
      : mode === "Link"
        ? !url.trim() || invalidLink
        : !file || !!error);
  const close = () => {
    if (!active.current && !pending) go(origin.source, origin);
  };
  async function submit() {
    if (disabled || active.current) return;
    if (mode === "Link") {
      setNotice(statusText["Link unavailable"]!);
      return;
    }
    const source = mode === "Paste list" ? paste : file;
    if (!source) return;
    active.current = true;
    setWorking(true);
    setNotice("Importing deck…");
    let saved = false;
    try {
      let entries;
      for (let attempt = 1; mounted.current; attempt++) {
        try {
          entries = await resolveImport(source);
          break;
        } catch (err) {
          if (!mounted.current) return;
          if (err instanceof FileFormatError || err instanceof RangeError) {
            if (mode === "File") {
              setError(err.message);
              setNotice("");
            } else setNotice(err.message);
            return;
          }
          if (attempt === 3) {
            setNotice("Couldn’t import deck.");
            return;
          }
        }
      }
      if (!mounted.current || !entries) return;
      saved = createDeck(go, origin, entries);
      if (!saved) setNotice("Couldn’t import deck.");
    } finally {
      if (mounted.current && !saved) {
        active.current = false;
        setWorking(false);
      }
    }
  }
  return (
    <Modal
      title="Import deck"
      className={mode === "Link" ? "cs-import cs-import-link" : "cs-import"}
      close={close}
      busy={pending}
      footer={
        <>
          <Btn disabled={pending} onClick={close}>
            Cancel
          </Btn>
          <Btn disabled={disabled} primary onClick={submit}>
            Import deck
          </Btn>
        </>
      }
    >
      <div inert={pending} className="cs-import-inputs">
        <Tabs
          labels={modes}
          value={mode}
          change={(v) => {
            setMode(v);
            setNotice("");
          }}
        />
        {mode === "Paste list" ? (
          <div className="cs-import-source">
            <label htmlFor={title}>Deck list</label>
            <textarea
              id={title}
              placeholder="Paste your deck list…"
              value={paste}
              onChange={(e) => {
                setPaste(e.currentTarget.value);
                setNotice("");
              }}
            />
            {!paste.trim() && <small>Paste a deck list.</small>}
          </div>
        ) : mode === "Link" ? (
          <TextField
            label="Deck link"
            value={url}
            onValueChange={(value) => {
              setUrl(value);
              setNotice("");
            }}
            type="url"
            hint="Moxfield or TappedOut."
            {...(invalidLink
              ? { error: "Use a Moxfield or TappedOut link." }
              : {})}
          />
        ) : (
          <div
            className="cs-dropfile"
            onDragOver={(e) => e.preventDefault()}
            onDrop={(e) => {
              e.preventDefault();
              accept(e.dataTransfer.files[0]);
            }}
          >
            <h2>{file?.name || "Drop a deck file here"}</h2>
            <small>{file ? "Deck file" : ".txt, .dek, .cod"}</small>
            <input
              type="file"
              accept=".txt,.dek,.cod"
              aria-label="Deck file"
              aria-describedby={error ? title + "-file-error" : undefined}
              hidden
              ref={fileInput}
              onChange={(e) => accept(e.target.files?.[0])}
            />
            <Btn onClick={() => fileInput.current?.click()}>Choose file</Btn>
            {error && (
              <small id={title + "-file-error"} role="status">
                {error}
              </small>
            )}
          </div>
        )}
      </div>
      <div className="cs-import-feedback" role="status">
        {notice && <span className="cs-notice">{notice}</span>}
      </div>
    </Modal>
  );
}
export function ImportDeck({
  id,
  go,
  origin,
}: {
  id: string;
  go: Jump;
  origin?: ShelfContext | undefined;
}) {
  if (id === "IMP-04" || id === "IMP-05") {
    const labels =
      id === "IMP-04"
        ? [
            "Selected file",
            "Empty paste",
            "Unsupported link",
            "Unsupported file",
          ]
        : [
            "Processing",
            "Exhausted failure",
            "Link unavailable",
            "Unconfirmed outcome",
          ];
    return (
      <Board>
        {labels.map((v, i) => (
          <Example key={v} title={v} width={680} height={495}>
            <ImportForm
              initial={
                (id === "IMP-04"
                  ? ["File", "Paste list", "Link", "File"]
                  : ["Paste list", "Paste list", "Link", "Paste list"])[i] ??
                "Paste list"
              }
              go={go}
              scenario={v}
            />
          </Example>
        ))}
      </Board>
    );
  }
  return (
    <>
      <DeckLibrary id={origin?.source ?? "DL-01"} go={go} launch={origin} />
      <ImportForm
        initial={modes[Number(id.slice(-2)) - 1] ?? "Paste list"}
        origin={origin ?? defaultOrigin}
        fixture={!origin}
        go={go}
      />
    </>
  );
}

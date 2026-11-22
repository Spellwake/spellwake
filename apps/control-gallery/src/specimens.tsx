import { useEffect, useRef, useState, type FormEvent } from "react";
import { Plus, Upload } from "lucide-react";
import { EmptyState, ActionButton, SelectMenu, TextField } from "@spellwake/ui";
import type { View } from "./references";

const formats = ["All formats", "Standard", "Modern", "Commander"].map(
  (label) => ({ label, value: label }),
);
const decks = [
  ["Goblins", "Standard"],
  ["Blue Control", "Modern"],
  ["Elves", "Commander"],
  ["Angels", "Standard"],
  ["Graveyard", "Modern"],
  ["Dragons", "Commander"],
] as const;

export function DeckNoMatches({ clearFilters }: { clearFilters: () => void }) {
  return (
    <EmptyState
      title="No matching decks"
      description="Try another name or format."
      action={
        <ActionButton
          className="empty-deck-action"
          treatment="compact-form"
          onClick={clearFilters}
        >
          Clear filters
        </ActionButton>
      }
    />
  );
}

export function DeckControls({
  view,
  navigate,
}: {
  view: View;
  navigate: (view: View) => void;
}) {
  const [format, setFormat] = useState(
    view === "DL-07" ? "Modern" : "All formats",
  );
  const [query, setQuery] = useState(view === "DL-03" ? "Blue" : "");
  const [created, setCreated] = useState(0);
  const extra = Array.from(
    { length: created },
    (_, i) => [`Untitled deck ${i + 1}`, "Standard"] as const,
  );
  const found = [...decks, ...extra].filter(
    ([name, group]) =>
      (format === "All formats" || group === format) &&
      name.toLowerCase().includes(query.trim().toLowerCase()),
  );
  return (
    <>
      <div
        className={view === "DL-02" ? "stage scene menu-demo" : "stage scene"}
      >
        <div className="deck-toolbar" data-specimen>
          <TextField
            id="deck-search"
            label="Search decks"
            hideLabel
            type="search"
            placeholder="Search decks..."
            value={query}
            onValueChange={setQuery}
            autoFocus={view === "DL-03"}
          />
          <SelectMenu
            label="Deck format"
            options={formats}
            value={format}
            onValueChange={setFormat}
            defaultOpen={view === "DL-02"}
          />
          <ActionButton
            id="open-deck-import"
            icon={<Upload />}
            onClick={() => navigate("IMP-02")}
          >
            Import deck
          </ActionButton>
          <ActionButton
            icon={<Plus />}
            emphasis="primary"
            onClick={() => {
              setCreated(created + 1);
              setFormat("All formats");
              setQuery("");
            }}
          >
            Create deck
          </ActionButton>
        </div>
      </div>
      {found.length === 0 && (
        <div className="preference-stage library-feedback">
          <div className="message-example">
            <DeckNoMatches
              clearFilters={() => {
                setFormat("All formats");
                setQuery("");
                document.getElementById("deck-search")?.focus();
              }}
            />
          </div>
        </div>
      )}
      <p className="readout" role="status">
        <strong>
          {found.length} {found.length === 1 ? "deck" : "decks"}
        </strong>
        <span>{found.map(([name]) => name).join(" · ")}</span>
      </p>
    </>
  );
}

export function PredictionControls() {
  const [query, setQuery] = useState("");
  const [message, setMessage] = useState("");
  const matches = ["Lightning Bolt", "Counterspell", "Sol Ring"].filter(
    (name) => name.toLowerCase().includes(query.trim().toLowerCase()),
  );
  return (
    <>
      <div className="stage scene">
        <div className="prediction-toolbar" data-specimen>
          <TextField
            label="Search predictions"
            hideLabel
            type="search"
            treatment="header"
            placeholder="Search predictions..."
            value={query}
            onValueChange={(q) => {
              setQuery(q);
              setMessage("");
            }}
          />
          <ActionButton
            treatment="header"
            icon={<Plus />}
            emphasis="primary"
            onClick={() => setMessage("Create prediction selected")}
          >
            Create prediction
          </ActionButton>
        </div>
      </div>
      <p className="readout" role="status">
        <strong>
          {message ||
            `${matches.length} ${matches.length === 1 ? "prediction" : "predictions"}`}
        </strong>
        <span>{matches.join(" · ") || "No matching predictions."}</span>
      </p>
    </>
  );
}

function supportedLink(value: string) {
  try {
    const url = new URL(value);
    return (
      ["https:", "http:"].includes(url.protocol) &&
      [
        "moxfield.com",
        "www.moxfield.com",
        "tappedout.net",
        "www.tappedout.net",
      ].includes(url.hostname) &&
      url.pathname.length > 1
    );
  } catch {
    return false;
  }
}
export function ImportControls({
  view,
  cancel,
}: {
  view: View;
  cancel: () => void;
}) {
  const unsupported = view === "IMP-04";
  const [link, setLink] = useState(
    unsupported
      ? "https://example.com/deck"
      : "https://moxfield.com/decks/example",
  );
  const [showError, setShowError] = useState(unsupported);
  const [busy, setBusy] = useState(false);
  const [message, setMessage] = useState("");
  const timer = useRef<ReturnType<typeof setTimeout> | undefined>(undefined);
  useEffect(() => () => clearTimeout(timer.current), []);
  function submit(event: FormEvent) {
    event.preventDefault();
    if (busy || !link.trim()) return;
    if (!supportedLink(link)) {
      setShowError(true);
      document.getElementById("deck-link")?.focus();
      return;
    }
    setBusy(true);
    setMessage("Submitting sample link…");
    timer.current = setTimeout(() => {
      setBusy(false);
      setMessage("Sample link submitted");
    }, 650);
  }
  return (
    <>
      <div className="stage form-stage">
        <form
          className={unsupported ? "import-form montage" : "import-form"}
          data-specimen
          onSubmit={submit}
          noValidate
        >
          <TextField
            treatment={unsupported ? "compact-form" : "default"}
            id="deck-link"
            label="Deck link"
            type="url"
            hint="Moxfield or TappedOut"
            error={showError ? "Use a Moxfield or TappedOut deck link." : ""}
            readOnly={busy}
            value={link}
            onValueChange={(value) => {
              setLink(value);
              setShowError(false);
              setMessage("");
            }}
          />
          <div className="form-actions">
            <ActionButton
              treatment={unsupported ? "compact-form" : "form"}
              disabled={busy}
              onClick={cancel}
            >
              Cancel
            </ActionButton>
            <ActionButton
              treatment={unsupported ? "compact-form" : "form"}
              emphasis="primary"
              type="submit"
              aria-busy={busy || undefined}
              disabled={busy || !link.trim()}
            >
              Import deck
            </ActionButton>
          </div>
        </form>
      </div>
      <p className="readout" role="status">
        <strong>{message || "Local preview"}</strong>
        <span>Try a deck link to check validation and submission.</span>
      </p>
    </>
  );
}

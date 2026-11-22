import { useRef, useState, type DragEvent, type ReactElement } from "react";
import {
  ActionButton,
  Dialog,
  FeedbackMessages,
  PanelTabs,
  TextField,
} from "@spellwake/ui";
import {
  DECK_FILE_TYPES,
  MAX_DECK_FILE_BYTES,
  deckTextFromFile,
  isDeckLink,
} from "../../data/deck-files";
import type { Outcome } from "../../decks/deck-sources";
import { useCommand, useNotice } from "../account/operation";
import * as s from "./import.css";

type SourceTab = "paste" | "link" | "file";
const TABS: readonly { value: SourceTab; label: string }[] = [
  { value: "paste", label: "Paste list" },
  { value: "link", label: "Link" },
  { value: "file", label: "File" },
];
const FILE_TYPES = DECK_FILE_TYPES.join(", ");
const LINK_UNAVAILABLE = "Couldn’t open link. Paste or upload a list.";

/**
 * Import deck: a list pasted, a link, or a file, made into one saved Untitled
 * deck that then opens in the builder. Each tab keeps what was entered while
 * the dialog is open; closing clears it all. While an import runs, the dialog
 * cannot be closed and nothing can be edited.
 */
export function ImportDeckDialog({
  onImport,
  onClose,
}: {
  /** Imports a list. On success the caller opens the deck, which closes this. */
  onImport: (text: string) => Promise<Outcome>;
  onClose: () => void;
}): ReactElement {
  const [tab, setTab] = useState<SourceTab>("paste");
  const [pasted, setPasted] = useState("");
  const [link, setLink] = useState("");
  const [file, setFile] = useState<{ name: string; text: string } | null>(null);
  const [problem, setProblem] = useState<{
    tab: SourceTab;
    text: string;
  } | null>(null);
  const [isOver, setIsOver] = useState(false);
  const chooser = useRef<HTMLInputElement>(null);
  const command = useCommand();
  const notice = useNotice();
  const { say } = notice;
  const isBusy = command.isBusy;
  const fieldProblem = problem?.tab === tab ? problem.text : null;

  const readFile = (chosen: File | undefined): void => {
    if (!chosen) return;
    setProblem(null);
    const extension = /\.[^.]+$/.exec(chosen.name.toLowerCase())?.[0] ?? "";
    if (
      !(DECK_FILE_TYPES as readonly string[]).includes(extension) ||
      chosen.size > MAX_DECK_FILE_BYTES
    ) {
      setFile(null);
      setProblem({ tab: "file", text: "Choose a .txt, .dek, or .cod file." });
      return;
    }
    void chosen
      .text()
      .then((contents) => {
        const read = deckTextFromFile(chosen.name, contents);
        if (read.status === "read")
          setFile({ name: chosen.name, text: read.text });
        else {
          setFile(null);
          setProblem({
            tab: "file",
            text: "Choose a .txt, .dek, or .cod file.",
          });
        }
      })
      .catch(() => say("Couldn’t import deck."));
  };

  const submit = (): void => {
    if (isBusy) return;
    setProblem(null);
    notice.clear();
    let text: string;
    if (tab === "paste") {
      if (pasted.trim() === "") {
        setProblem({ tab, text: "Paste a deck list." });
        return;
      }
      text = pasted;
    } else if (tab === "link") {
      if (!isDeckLink(link)) {
        setProblem({ tab, text: "Use a Moxfield or TappedOut deck link." });
        return;
      }
      // Reading a deck from either site is not built, so a valid link says so.
      say(LINK_UNAVAILABLE);
      return;
    } else {
      if (!file) {
        setProblem({ tab, text: "Choose a .txt, .dek, or .cod file." });
        return;
      }
      text = file.text;
    }
    say("Importing deck…", "working");
    void command
      .run(() => onImport(text))
      .then((outcome) => {
        if (outcome?.status === "fail") say(outcome.message);
        else notice.clear();
      });
  };

  const onDrop = (event: DragEvent<HTMLDivElement>): void => {
    event.preventDefault();
    setIsOver(false);
    if (!isBusy) readFile(event.dataTransfer.files[0]);
  };

  return (
    <Dialog
      open
      title="Import deck"
      className={s.shell}
      busy={isBusy}
      onClose={onClose}
      actions={
        <>
          <ActionButton
            className={s.action}
            disabled={isBusy}
            onClick={onClose}
          >
            Cancel
          </ActionButton>
          <ActionButton
            className={s.action}
            emphasis="primary"
            // The label stays put while it works; repeated presses are ignored.
            aria-busy={isBusy || undefined}
            onClick={submit}
          >
            Import deck
          </ActionButton>
        </>
      }
    >
      <div className={s.body} data-import-source={tab}>
        <PanelTabs
          label="Deck source"
          value={tab}
          onValueChange={(value) => {
            if (
              !isBusy &&
              (value === "paste" || value === "link" || value === "file")
            )
              setTab(value);
          }}
          tabs={TABS.map(({ value, label }) => ({
            value,
            label,
            content: null,
          }))}
        />
        <div className={s.panel} inert={isBusy || undefined}>
          {tab === "paste" && (
            <>
              <label className={s.label} htmlFor="import-deck-list">
                Deck list
              </label>
              <textarea
                id="import-deck-list"
                className={s.list}
                value={pasted}
                placeholder="Paste your deck list…"
                spellCheck={false}
                aria-invalid={fieldProblem ? true : undefined}
                aria-describedby={
                  fieldProblem ? "import-deck-problem" : undefined
                }
                data-autofocus=""
                onChange={(event) => {
                  setPasted(event.currentTarget.value);
                  setProblem(null);
                }}
              />
            </>
          )}
          {tab === "link" && (
            // A form so Enter in the one field imports, as the button does.
            <form
              noValidate
              onSubmit={(event) => {
                event.preventDefault();
                submit();
              }}
            >
              <TextField
                label="Deck link"
                type="url"
                inputMode="url"
                autoComplete="off"
                spellCheck={false}
                placeholder="https://moxfield.com/decks/…"
                value={link}
                onValueChange={(value) => {
                  setLink(value);
                  setProblem(null);
                }}
                {...(fieldProblem
                  ? { error: fieldProblem }
                  : { hint: "Moxfield or TappedOut" })}
              />
            </form>
          )}
          {tab === "file" && (
            <div
              className={s.drop}
              data-over={isOver}
              aria-invalid={fieldProblem ? true : undefined}
              onDragOver={(event) => {
                event.preventDefault();
                if (!isBusy) setIsOver(true);
              }}
              onDragLeave={() => setIsOver(false)}
              onDrop={onDrop}
            >
              <p className={s.dropTitle}>
                {file?.name ?? "Drop a deck file here"}
              </p>
              <p className={s.dropTypes}>{FILE_TYPES}</p>
              <ActionButton
                className={s.action}
                onClick={() => chooser.current?.click()}
              >
                Choose file
              </ActionButton>
              <input
                ref={chooser}
                className={s.hiddenInput}
                type="file"
                accept={DECK_FILE_TYPES.join(",")}
                tabIndex={-1}
                aria-hidden="true"
                onChange={(event) => {
                  readFile(event.currentTarget.files?.[0]);
                  // The same file chosen again is read again.
                  event.currentTarget.value = "";
                }}
              />
            </div>
          )}
          {fieldProblem && tab !== "link" && (
            <p id="import-deck-problem" className={s.message}>
              {fieldProblem}
            </p>
          )}
        </div>
        <div className={s.notice}>
          <FeedbackMessages
            label="Import messages"
            presentation="toast"
            items={notice.items}
          />
        </div>
      </div>
    </Dialog>
  );
}

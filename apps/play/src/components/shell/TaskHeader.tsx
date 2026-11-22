import {
  useEffect,
  useRef,
  useState,
  type ReactElement,
  type ReactNode,
} from "react";
import { ChevronLeft, Pencil } from "lucide-react";
import { ActionButton, designPixels } from "@spellwake/ui";
import * as s from "./frames.css";

/**
 * Header for a focused, full-screen task. It replaces the application header:
 * one way back, the task's title, and the task's own actions. A title that can
 * be renamed is edited in place, in the same type and position.
 */
export function TaskHeader({
  back,
  title,
  onRename,
  children,
}: {
  back: { label: string; onActivate: () => void };
  title: string;
  /** Commits a new, non-empty title. Omit for a fixed title. */
  onRename?: (title: string) => void;
  /** The task's actions, most important last. */
  children?: ReactNode;
}): ReactElement {
  const [draft, setDraft] = useState<string | null>(null);
  const input = useRef<HTMLInputElement>(null);
  const renameButton = useRef<HTMLButtonElement>(null);
  const isEditing = draft !== null;
  useEffect(() => {
    // Select the whole name once when editing starts.
    if (isEditing) input.current?.select();
  }, [isEditing]);
  const finish = (commit: boolean): void => {
    const next = draft?.trim();
    setDraft(null);
    if (commit && next && next !== title) onRename?.(next);
    renameButton.current?.focus();
  };
  return (
    <header className={s.taskHeader} data-frame="task-header">
      <ActionButton
        treatment="compact-form"
        icon={<ChevronLeft />}
        onClick={back.onActivate}
      >
        {back.label}
      </ActionButton>
      <div className={s.taskTitleRow}>
        {draft === null ? (
          <h1 className={s.taskTitle} title={title}>
            {title}
          </h1>
        ) : (
          <input
            ref={input}
            className={s.taskTitleInput}
            aria-label="Name"
            value={draft}
            maxLength={120}
            onChange={(event) => setDraft(event.target.value)}
            onBlur={() => finish(true)}
            onKeyDown={(event) => {
              if (event.key === "Enter") finish(true);
              if (event.key === "Escape") finish(false);
            }}
          />
        )}
        {onRename && draft === null && (
          <button
            ref={renameButton}
            type="button"
            className={s.taskRename}
            aria-label={`Rename ${title}`}
            onClick={() => setDraft(title)}
          >
            <Pencil size={designPixels(22)} aria-hidden="true" />
          </button>
        )}
      </div>
      <div className={s.taskActions}>{children}</div>
    </header>
  );
}

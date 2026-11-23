import {
  useEffect,
  useLayoutEffect,
  useRef,
  useState,
  type Dispatch,
  type ReactElement,
  type SetStateAction,
} from "react";
import { AnimatePresence, motion, useReducedMotion } from "motion/react";
import { useAtom } from "jotai";
import { AudioLines, Droplet, Music, Volume2, X } from "lucide-react";
import {
  ChatComposer,
  ChatMessage,
  ConfirmationDialog,
  Dialog,
  RangeSlider,
  SettingSwitch,
} from "@spellwake/ui";
import type { MatchChatEntry } from "../../match/match-sources";
import {
  CHAT_LIMIT,
  matchPreferences,
  readPreferences,
  settleDraft,
  type MatchPreferences,
} from "../../match/table-panels";
import * as s from "./match.css";

/** How close to the end of the history counts as reading the newest message. */
const AT_END = 24;

/**
 * This table's conversation, as approved (CM-02 to CM-05, CM-11): a floating
 * panel that never pauses the game. The draft belongs to the owner, so
 * closing the panel keeps it. A message is added only once it is accepted.
 */
export function TableChat({
  open,
  messages,
  send,
  watching,
  draft,
  setDraft,
  onSeen,
  onClose,
}: {
  open: boolean;
  messages: readonly MatchChatEntry[];
  /** Null where this table cannot send. */
  send: ((body: string) => Promise<boolean>) | null;
  /** A player who conceded reads the chat and cannot write. */
  watching: boolean;
  draft: string;
  setDraft: Dispatch<SetStateAction<string>>;
  /** The newest message is on screen. */
  onSeen: (id: string) => void;
  onClose: () => void;
}): ReactElement {
  const reduced = useReducedMotion();
  const log = useRef<HTMLDivElement>(null);
  const atEnd = useRef(true);
  const [sending, setSending] = useState(false);
  const [refused, setRefused] = useState(false);
  const newest = messages.at(-1);

  // New messages follow only a reader who is already at the newest one.
  useLayoutEffect(() => {
    const node = log.current;
    if (!open || !node) return;
    if (atEnd.current || newest?.isMine) {
      node.scrollTop = node.scrollHeight;
      atEnd.current = true;
    }
    if (atEnd.current && newest) onSeen(newest.id);
  }, [open, newest, onSeen]);
  useEffect(() => {
    if (open) atEnd.current = true;
  }, [open]);

  const submit = async (text: string) => {
    if (!send || sending) return;
    setSending(true);
    setRefused(false);
    const accepted = await send(text).catch(() => false);
    setSending(false);
    setRefused(!accepted);
    // Only the exact text that was sent is cleared.
    setDraft((current) => settleDraft(current, text, accepted));
  };

  return (
    <AnimatePresence>
      {open && (
        <motion.section
          className={s.chatPanel}
          aria-label="Table chat"
          data-table-chat={watching ? "watching" : "open"}
          initial={reduced ? false : { opacity: 0, y: -8 }}
          animate={{ opacity: 1, y: 0 }}
          exit={reduced ? { opacity: 0 } : { opacity: 0, y: -6 }}
          transition={{
            duration: reduced ? 0 : 0.24,
            ease: [0.22, 1, 0.36, 1],
          }}
          onKeyDown={(event) => {
            if (event.key !== "Escape") return;
            event.stopPropagation();
            onClose();
          }}
        >
          <header className={s.panelHeader}>
            Table chat
            <button
              type="button"
              className={s.panelClose}
              aria-label="Close table chat"
              onClick={onClose}
            >
              <X aria-hidden="true" />
            </button>
          </header>
          <div
            ref={log}
            className={s.chatLog}
            role="log"
            aria-label="Table messages"
            tabIndex={0}
            onScroll={(event) => {
              const node = event.currentTarget;
              atEnd.current =
                node.scrollHeight - node.scrollTop - node.clientHeight < AT_END;
              if (atEnd.current && newest) onSeen(newest.id);
            }}
          >
            {messages.length === 0 ? (
              <p className={s.chatQuiet}>No messages yet.</p>
            ) : (
              messages.map((message) => (
                <ChatMessage
                  key={message.id}
                  surface="table"
                  author={message.isMine ? "You" : message.senderName}
                  body={message.body}
                />
              ))
            )}
          </div>
          {refused && (
            <p className={s.chatRefused} role="status">
              Couldn’t send message.
            </p>
          )}
          {watching || !send ? (
            <ChatComposer mode="watching" />
          ) : (
            <ChatComposer
              mode="editable"
              surface="table"
              maxCharacters={CHAT_LIMIT}
              pending={sending}
              value={draft}
              placeholder="Message the table…"
              onChange={(text) => {
                setDraft(text);
                if (refused) setRefused(false);
              }}
              onSend={(text) => void submit(text)}
            />
          )}
        </motion.section>
      )}
    </AnimatePresence>
  );
}

const CHANNELS = [
  { key: "master", name: "Master volume", Icon: Volume2 },
  { key: "music", name: "Music", Icon: Music },
  { key: "effects", name: "Sound effects", Icon: AudioLines },
] as const;

/**
 * Match settings, as approved (CM-06, CM-07, CM-10A): three volumes and
 * Auto-tap mana, each kept as it is changed, with Concede beneath them for a
 * player still in the game.
 */
export function MatchSettings({
  open,
  onClose,
  onConcede,
}: {
  open: boolean;
  onClose: () => void;
  /** Absent for a player who is watching. */
  onConcede?: (() => void) | undefined;
}): ReactElement {
  const [stored, store] = useAtom(matchPreferences);
  const preferences = readPreferences(stored);
  const change = (next: Partial<MatchPreferences>) =>
    store({ ...preferences, ...next });
  return (
    <Dialog open={open} title="Settings" size="compact" onClose={onClose}>
      <div className={s.settings}>
        {CHANNELS.map(({ key, name, Icon }) => (
          <div key={key} className={s.setting}>
            <span className={s.settingName}>
              <Icon aria-hidden="true" />
              {name}
            </span>
            <span className={s.settingControl}>
              <RangeSlider
                label={name}
                value={preferences[key]}
                min={0}
                max={100}
                step={1}
                valueText={`${preferences[key]}%`}
                onValueChange={(value) => change({ [key]: value })}
              />
            </span>
          </div>
        ))}
        <div className={`${s.setting} ${s.settingRule}`}>
          <span className={s.settingName}>
            <Droplet aria-hidden="true" />
            Auto-tap mana
          </span>
          <SettingSwitch
            label="Auto-tap mana"
            checked={preferences.autoTap}
            onCheckedChange={(autoTap) => change({ autoTap })}
          />
        </div>
        {onConcede && (
          <button type="button" className={s.concedeEntry} onClick={onConcede}>
            Concede
          </button>
        )}
      </div>
    </Dialog>
  );
}

/**
 * Conceding, as approved (CM-08, CM-10B, CM-10C): one confirmation, guarded
 * while the game answers, which keeps its place with a short line if the game
 * refuses.
 */
export function ConcedeConfirmation({
  open,
  pending,
  failed,
  onCancel,
  onConfirm,
}: {
  open: boolean;
  pending: boolean;
  failed: boolean;
  onCancel: () => void;
  onConfirm: () => void;
}): ReactElement {
  return (
    <ConfirmationDialog
      open={open}
      appearance="match"
      tone="danger"
      title="Concede?"
      description="You can stay and watch."
      cancelLabel="Cancel"
      confirmLabel="Concede"
      pending={pending}
      dismissOnBackdrop
      onCancel={onCancel}
      onConfirm={onConfirm}
    >
      {failed && (
        <p className={s.concedeFailed} role="alert">
          Couldn’t concede.
        </p>
      )}
    </ConfirmationDialog>
  );
}

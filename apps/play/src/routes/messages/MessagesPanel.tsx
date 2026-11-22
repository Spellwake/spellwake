import { useEffect, useRef, useState, type ReactElement } from "react";
import {
  Ban,
  ChevronRight,
  EllipsisVertical,
  Plus,
  Repeat2,
  SendHorizontal,
  UserPlus,
  UsersRound,
  X,
} from "lucide-react";
import {
  ActionButton,
  ActionMenu,
  Dialog,
  FeedbackMessages,
  TextField,
} from "@spellwake/ui";
import { Portrait } from "../../account/Portrait";
import {
  ACTION_LABEL,
  canSend,
  MESSAGE_LIMIT,
  personMenu,
  relationshipLabel,
  type MessagingPerson,
  type MessagingSource,
  type PersonAction,
  type ThreadEvent,
} from "../../messaging/messaging-sources";
import { useCommand, useNotice } from "../account/operation";
import * as s from "./messages.css";

/** What the panel shows: one person's thread, Requests or Find People. */
export type MessagesView =
  { kind: "person"; id: string } | { kind: "requests" } | { kind: "find" };

const timeOf = (iso: string): string =>
  new Date(iso).toLocaleTimeString(undefined, {
    hour: "numeric",
    minute: "2-digit",
  });
const dayOf = (iso: string): string => {
  const date = new Date(iso);
  const today = new Date();
  const label = date.toLocaleDateString(undefined, {
    month: "short",
    day: "numeric",
  });
  return date.toDateString() === today.toDateString()
    ? `Today, ${label}`
    : label;
};

/**
 * Messages: friends on the rail; Requests and Find People below it; the
 * chosen person's thread to the right. One thread per person, with its own
 * draft. Nothing here makes someone a friend except Accept.
 */
export function MessagesPanel({
  source,
  view,
  onView,
  onClose,
  onTrade,
  onJoin,
  presentation,
}: {
  source: MessagingSource;
  view: MessagesView;
  onView: (view: MessagesView) => void;
  onClose: () => void;
  /** Opens the trade room with the person a trade card came from. */
  onTrade: (personId: string) => void;
  /** Opens the shared lobby for a game card. */
  onJoin: () => void;
  presentation: "panel" | "sheet";
}): ReactElement {
  const ready = source.status === "ready" ? source : null;
  // In the phone drawer the drawer's own close serves; the header leaves room for it.
  const close =
    presentation === "panel" ? (
      <button
        type="button"
        className={s.iconButton}
        aria-label="Close Messages"
        onClick={onClose}
      >
        <X aria-hidden="true" />
      </button>
    ) : (
      <span className={s.closeRoom} aria-hidden="true" />
    );
  const content = !ready ? (
    <div className={s.main}>
      <div className={s.head}>
        <div className={s.headText}>
          <h2 className={s.title}>Messages</h2>
        </div>
        {close}
      </div>
      <p className={s.quiet} role="status">
        {source.status === "loading" ? (
          <span className={s.spinner} aria-hidden="true" />
        ) : source.status === "signed-out" ? (
          "Sign in to see your messages."
        ) : source.status === "unregistered" ? (
          "Choose a username to start messaging."
        ) : (
          "Couldn’t load messages."
        )}
      </p>
    </div>
  ) : view.kind === "requests" ? (
    <PeopleList
      title="Requests"
      people={ready.requests}
      emptyText="No requests"
      onOpen={(id) => onView({ kind: "person", id })}
      close={close}
    />
  ) : view.kind === "find" ? (
    <FindPeople
      source={ready}
      onOpen={(id) => onView({ kind: "person", id })}
      close={close}
    />
  ) : (
    <Thread source={ready} close={close} onTrade={onTrade} onJoin={onJoin} />
  );
  return (
    <div
      className={presentation === "panel" ? s.panel : s.sheet}
      role={presentation === "panel" ? "dialog" : undefined}
      aria-label={presentation === "panel" ? "Messages" : undefined}
      onKeyDown={(event) => {
        if (event.key === "Escape" && presentation === "panel") {
          event.stopPropagation();
          onClose();
        }
      }}
    >
      <nav className={s.rail} aria-label="People">
        <div className={s.friends} role="list" aria-label="Friends">
          {ready?.friends.map((friend) => (
            <span key={friend.id} role="listitem">
              <button
                type="button"
                className={s.railButton}
                aria-label={
                  friend.unread
                    ? `${friend.name}, ${friend.unread} unread`
                    : friend.name
                }
                aria-current={view.kind === "person" && view.id === friend.id}
                onClick={() => onView({ kind: "person", id: friend.id })}
              >
                <Portrait portrait={friend.portrait} diameter={56} />
                {friend.unread > 0 && (
                  <span className={s.unreadDot} aria-hidden="true" />
                )}
              </button>
            </span>
          ))}
        </div>
        <span className={s.railDivider} />
        {ready && ready.requests.length > 0 && (
          <button
            type="button"
            className={s.roundIcon}
            aria-label={
              ready.attention
                ? `Requests, ${ready.attention} need attention`
                : "Requests"
            }
            aria-current={view.kind === "requests"}
            onClick={() => onView({ kind: "requests" })}
          >
            <UserPlus aria-hidden="true" />
            {ready.attention > 0 && (
              <span className={s.badge}>{ready.attention}</span>
            )}
          </button>
        )}
        <button
          type="button"
          className={s.roundIcon}
          aria-label="Find People"
          aria-current={view.kind === "find"}
          onClick={() => onView({ kind: "find" })}
        >
          <Plus aria-hidden="true" />
        </button>
      </nav>
      {content}
      {ready?.failure && (
        <div className={s.notice}>
          <FeedbackMessages
            label="Messages notices"
            presentation="toast"
            items={[
              {
                id: "read",
                text: ready.failure,
                onDismiss: ready.dismissFailure,
              },
            ]}
          />
        </div>
      )}
    </div>
  );
}

function PeopleList({
  title,
  people,
  emptyText,
  onOpen,
  close,
}: {
  title: string;
  people: readonly MessagingPerson[];
  emptyText: string;
  onOpen: (id: string) => void;
  close: ReactElement;
}): ReactElement {
  return (
    <div className={s.main}>
      <div className={s.head}>
        <div className={s.headText}>
          <h2 className={s.title} tabIndex={-1}>
            {title}
          </h2>
        </div>
        {close}
      </div>
      {people.length === 0 ? (
        <p className={s.quiet}>{emptyText}</p>
      ) : (
        <ul className={s.list}>
          {people.map((person) => (
            <li key={person.id}>
              <PersonRow
                person={person}
                line={person.preview}
                onOpen={onOpen}
              />
            </li>
          ))}
        </ul>
      )}
    </div>
  );
}

function PersonRow({
  person,
  line,
  onOpen,
}: {
  person: MessagingPerson;
  line: string | null;
  onOpen: (id: string) => void;
}): ReactElement {
  return (
    <button type="button" className={s.row} onClick={() => onOpen(person.id)}>
      <Portrait portrait={person.portrait} diameter={60} />
      <span>
        <span className={s.rowName}>{person.name}</span>
        {line && <span className={s.rowLine}>{line}</span>}
      </span>
      <span className={s.rowTrail}>
        {person.unread > 0 && (
          <span className={s.dot} aria-label={`${person.unread} unread`} />
        )}
        <ChevronRight aria-hidden="true" />
      </span>
    </button>
  );
}

/** One search for discoverable players. Choosing one opens their thread. */
function FindPeople({
  source,
  onOpen,
  close,
}: {
  source: Extract<MessagingSource, { status: "ready" }>;
  onOpen: (id: string) => void;
  close: ReactElement;
}): ReactElement {
  const { search } = source;
  return (
    <div className={s.main}>
      <div className={s.head}>
        <div className={s.headText}>
          <h2 className={s.title}>Find People</h2>
        </div>
        {close}
      </div>
      <div className={s.search}>
        <TextField
          type="search"
          label="Search players"
          placeholder="Search players…"
          value={search.query}
          onValueChange={search.setQuery}
          hideLabel
          treatment="compact-form"
          autoFocus
        />
      </div>
      {search.state === "searching" ? (
        <p className={s.quiet} role="status">
          <span className={s.spinner} aria-hidden="true" />
        </p>
      ) : search.state === "failed" ? (
        <p className={s.quiet}>Players couldn’t be loaded.</p>
      ) : search.state === "ready" && search.results.length === 0 ? (
        <p className={s.quiet}>No players found</p>
      ) : (
        <ul className={s.list}>
          {search.results.map((person) => (
            <li key={person.id}>
              <PersonRow
                person={person}
                line={person.relationship === "friend" ? "Friend" : "Player"}
                onOpen={onOpen}
              />
            </li>
          ))}
        </ul>
      )}
    </div>
  );
}

/** The open person's thread: history, cards, the composer and its delivery state. */
function Thread({
  source,
  close,
  onTrade,
  onJoin,
}: {
  source: Extract<MessagingSource, { status: "ready" }>;
  close: ReactElement;
  onTrade: (personId: string) => void;
  onJoin: () => void;
}): ReactElement {
  const thread = source.thread;
  const history = useRef<HTMLDivElement>(null);
  const [confirm, setConfirm] = useState<"remove" | "block" | null>(null);
  const command = useCommand();
  const notice = useNotice();
  const menuTrigger = useRef<HTMLButtonElement>(null);
  const count = thread?.events.length ?? 0;
  // New messages scroll into view.
  useEffect(() => {
    history.current?.scrollTo({ top: history.current.scrollHeight });
  }, [count, thread?.person.id]);
  if (!thread)
    return (
      <div className={s.main}>
        <div className={s.head}>
          <div className={s.headText}>
            <h2 className={s.title}>Messages</h2>
          </div>
          {close}
        </div>
        <p className={s.empty}>Choose someone to message.</p>
      </div>
    );
  const { person } = thread;
  const act = (action: PersonAction) => {
    void command
      .run(() => source.act(person.id, action))
      .then((outcome) => {
        if (outcome?.status === "fail") notice.say(outcome.message);
      });
  };
  const menu = personMenu(person);
  let lastDay = "";
  return (
    <div className={s.main}>
      <div className={s.head}>
        <div className={s.headText}>
          <h2 className={s.title} id="thread-title" tabIndex={-1}>
            {person.name}
          </h2>
          <p className={s.relation}>
            {person.isBlocked && <Ban size={18} aria-hidden="true" />}
            {relationshipLabel(person)}
          </p>
        </div>
        {menu.length > 0 && (
          <ActionMenu
            label={`${person.name}: actions`}
            commands={menu.map((action) => ({
              id: action,
              label: ACTION_LABEL[action],
            }))}
            onCommand={(id) => {
              const action = id as PersonAction;
              // Remove friend and Block ask first; the rest act at once.
              if (action === "remove" || action === "block") setConfirm(action);
              else act(action);
            }}
            trigger={
              <button
                ref={menuTrigger}
                type="button"
                className={s.iconButton}
                aria-label={`${person.name}: actions`}
              >
                <EllipsisVertical aria-hidden="true" />
              </button>
            }
          />
        )}
        {close}
      </div>
      <div
        className={s.body}
        ref={history}
        aria-label={`Conversation with ${person.name}`}
        role="log"
      >
        {person.relationship === "incoming" && (
          <div className={s.requestCard}>
            <div className={s.cardHead}>
              <UserPlus aria-hidden="true" />
              {person.name} wants to be friends
            </div>
            <div className={s.cardActions}>
              <ActionButton
                treatment="compact-form"
                emphasis="primary"
                pending={command.isBusy}
                onClick={() => act("accept")}
              >
                Accept
              </ActionButton>
              <ActionButton
                treatment="compact-form"
                disabled={command.isBusy}
                onClick={() => act("decline")}
              >
                Decline
              </ActionButton>
            </div>
          </div>
        )}
        {thread.state === "loading" ? (
          <p className={s.quiet} role="status">
            <span className={s.spinner} aria-hidden="true" />
          </p>
        ) : thread.events.length === 0 ? (
          person.relationship !== "incoming" && (
            <p className={s.empty}>
              {person.canContact ? "Start a conversation." : "No messages yet."}
            </p>
          )
        ) : (
          <>
            {thread.hasOlder && (
              <p className={s.quiet}>
                <ActionButton
                  treatment="compact-form"
                  onClick={thread.loadOlder}
                >
                  Earlier messages
                </ActionButton>
              </p>
            )}
            {thread.events.map((event) => {
              const day = dayOf(event.sentAt);
              const divider = day !== lastDay ? (lastDay = day) : null;
              return (
                <div key={event.id}>
                  {divider && <p className={s.day}>{divider}</p>}
                  <Event
                    event={event}
                    person={person}
                    respond={source.respond}
                    onTrade={onTrade}
                    onJoin={onJoin}
                  />
                </div>
              );
            })}
          </>
        )}
      </div>
      {person.isBlocked ? (
        <p className={s.blocked} role="status">
          <Ban aria-hidden="true" />
          Player blocked.
        </p>
      ) : person.isDeleted ? (
        <p className={s.blocked} role="status">
          This player’s account was deleted.
        </p>
      ) : !person.canContact ? (
        <p className={s.blocked} role="status">
          You can’t message this player now.
        </p>
      ) : source.delivery.state === "unconfirmed" ? (
        <p className={s.status} role="status">
          Couldn’t confirm delivery status.
          <ActionButton
            treatment="compact-form"
            emphasis="primary"
            pending={source.delivery.isChecking}
            onClick={source.checkDelivery}
          >
            Check delivery
          </ActionButton>
        </p>
      ) : source.delivery.state === "rejected" ? (
        <p className={s.status} role="status">
          {source.delivery.message}
        </p>
      ) : null}
      <Composer source={source} person={person} />
      <FeedbackMessages
        label="Thread messages"
        presentation="inline"
        items={notice.items}
      />
      {confirm && (
        <Dialog
          open
          title={
            confirm === "remove"
              ? `Remove ${person.name}?`
              : `Block ${person.name}?`
          }
          size="compact"
          busy={command.isBusy}
          returnFocusRef={menuTrigger}
          onClose={() => setConfirm(null)}
          actions={
            <>
              <ActionButton
                disabled={command.isBusy}
                onClick={() => setConfirm(null)}
              >
                Cancel
              </ActionButton>
              <ActionButton
                emphasis="primary"
                pending={command.isBusy}
                onClick={() => {
                  const action = confirm;
                  void command
                    .run(() => source.act(person.id, action))
                    .then((outcome) => {
                      if (outcome?.status === "success") setConfirm(null);
                      else if (outcome) notice.say(outcome.message);
                    });
                }}
              >
                {confirm === "remove" ? "Remove friend" : "Block"}
              </ActionButton>
            </>
          }
        >
          <p>
            {confirm === "remove"
              ? "You’ll stay able to read your messages."
              : "They won’t be able to message you. Your history stays."}
          </p>
        </Dialog>
      )}
    </div>
  );
}

function Event({
  event,
  person,
  respond,
  onTrade,
  onJoin,
}: {
  event: ThreadEvent;
  person: MessagingPerson;
  respond: Extract<MessagingSource, { status: "ready" }>["respond"];
  onTrade: (personId: string) => void;
  onJoin: () => void;
}): ReactElement {
  const command = useCommand();
  if (event.kind === "message")
    return (
      <div className={event.isMine ? `${s.message} ${s.mine}` : s.message}>
        {!event.isMine && <Portrait portrait={person.portrait} diameter={44} />}
        <span className={s.bubble}>{event.body}</span>
        <time className={s.time} dateTime={event.sentAt}>
          {timeOf(event.sentAt)}
        </time>
      </div>
    );
  const isOpen = event.state === "open" && !person.isBlocked;
  const outcome =
    event.state === "declined"
      ? "Declined"
      : event.state === "expired"
        ? "Expired"
        : null;
  return (
    <div className={s.card}>
      <div className={s.cardHead}>
        {event.kind === "trade" ? (
          <Repeat2 aria-hidden="true" />
        ) : (
          <UsersRound aria-hidden="true" />
        )}
        {event.kind === "trade" ? "Trade offer" : "Game invite"}
        {event.kind === "game" && (
          <span className={s.cardDetail}>{event.detail}</span>
        )}
      </div>
      {outcome ? (
        <p className={s.cardOutcome}>{outcome}</p>
      ) : (
        <div className={s.cardActions}>
          <ActionButton
            treatment="compact-form"
            emphasis="primary"
            disabled={!isOpen}
            onClick={event.kind === "trade" ? () => onTrade(person.id) : onJoin}
          >
            {event.kind === "trade" ? "Trade" : "Join"}
          </ActionButton>
          <ActionButton
            treatment="compact-form"
            disabled={!isOpen || !respond}
            pending={command.isBusy}
            onClick={() =>
              respond && void command.run(() => respond(event.id, "decline"))
            }
          >
            Decline
          </ActionButton>
        </div>
      )}
    </div>
  );
}

/** Enter sends; Shift+Enter adds a line; typing in an IME never sends. */
function Composer({
  source,
  person,
}: {
  source: Extract<MessagingSource, { status: "ready" }>;
  person: MessagingPerson;
}): ReactElement {
  const blocked = !person.canContact;
  const isSending = source.delivery.state === "sending";
  const ready =
    canSend(source.draft) &&
    !blocked &&
    source.delivery.state !== "unconfirmed";
  return (
    <form
      className={s.composer}
      onSubmit={(event) => {
        event.preventDefault();
        if (ready && !isSending) source.send();
      }}
    >
      <textarea
        className={s.textarea}
        aria-label={`Message ${person.name}`}
        placeholder={`Message ${person.name}…`}
        rows={1}
        value={source.draft}
        maxLength={MESSAGE_LIMIT * 2}
        disabled={blocked}
        onChange={(event) => source.setDraft(event.currentTarget.value)}
        onKeyDown={(event) => {
          if (
            event.key === "Enter" &&
            !event.shiftKey &&
            !event.nativeEvent.isComposing
          ) {
            event.preventDefault();
            if (ready && !isSending) source.send();
          }
        }}
      />
      <button
        type="submit"
        className={s.iconButton}
        aria-label="Send"
        disabled={!ready || isSending}
      >
        <SendHorizontal aria-hidden="true" />
      </button>
    </form>
  );
}

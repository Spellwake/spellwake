import {
  useEffect,
  useRef,
  type FormEvent,
  type ReactElement,
  type ReactNode,
  type Ref,
} from "react";
import { motion } from "motion/react";
import { ArrowLeft, ArrowRight, Check, Info, LoaderCircle } from "lucide-react";
import {
  ActionButton,
  EmailCodeEntry,
  ProviderActions,
  TextField,
} from "@spellwake/ui";
import type { SignInProvider } from "../../auth/oauth";
import { motionLanguage as m } from "../../lib/motion";
import * as s from "./entry.css";

/**
 * The account entry screens as plain presentation: what to show and what the
 * person did. Sending codes, opening providers and creating accounts belong to
 * the routes that render these. Copy and anatomy follow the approved
 * account-entry walkthrough (AE-01 to AE-07).
 */
const asset = (path: string): string => `${import.meta.env.BASE_URL}${path}`;
const PROVIDERS: readonly {
  id: SignInProvider;
  label: string;
  iconSrc: string;
}[] = [
  {
    id: "google",
    label: "Continue with Google",
    iconSrc: asset("entry/google.svg"),
  },
  {
    id: "apple",
    label: "Continue with Apple",
    iconSrc: asset("entry/apple.svg"),
  },
  {
    id: "discord",
    label: "Continue with Discord",
    iconSrc: asset("entry/discord.svg"),
  },
];

/** A passive line of status. `tone` picks its mark: a ring while working, a tick when done. */
export type EntryNotice = { text: string; tone: "working" | "done" | "info" };

function Notice({ notice }: { notice: EntryNotice | null }): ReactElement {
  const Icon =
    notice?.tone === "working"
      ? LoaderCircle
      : notice?.tone === "done"
        ? Check
        : Info;
  return (
    // The region is always present so a screen reader hears each new status.
    <div role="status">
      {notice && (
        <p className={s.notice}>
          <Icon
            aria-hidden="true"
            className={`${s.noticeIcon} ${notice.tone === "working" ? s.spinner : ""}`}
          />
          {notice.text}
        </p>
      )}
    </div>
  );
}

/**
 * Moves focus to a card's first text field when `signal` changes, and once on
 * arrival when `onArrival` is set. A field is refocused after a refused entry
 * so the correction starts where the problem is.
 */
function useFieldFocus(signal: number, onArrival: boolean) {
  const card = useRef<HTMLDivElement>(null);
  const seen = useRef<number | null>(null);
  useEffect(() => {
    const isArrival = seen.current === null;
    const isRepeat = seen.current === signal;
    seen.current = signal;
    if (isArrival ? !onArrival : isRepeat) return;
    card.current?.querySelector("input")?.focus();
  }, [signal, onArrival]);
  return card;
}

/** The card every entry screen shares: mark, heading, content, and a link under it. */
function EntryCard({
  title,
  lede,
  children,
  below,
  cardRef,
  focusHeading = true,
}: {
  title: string;
  lede?: ReactNode;
  children?: ReactNode;
  below?: ReactNode;
  cardRef?: Ref<HTMLDivElement>;
  /** False where the screen sends focus to its field on arrival. */
  focusHeading?: boolean;
}): ReactElement {
  const heading = useRef<HTMLHeadingElement>(null);
  // Each step is a new screen: move focus to its heading so it is announced.
  useEffect(() => {
    if (focusHeading) heading.current?.focus();
  }, [title, focusHeading]);
  return (
    <section className={s.scene} aria-label="Account entry">
      <motion.div
        key={title}
        className={s.stack}
        initial={{ opacity: 0, y: m.travel / 2 }}
        animate={{ opacity: 1, y: 0 }}
        transition={{ duration: m.duration.scene, ease: m.ease.arrive }}
      >
        <div className={s.card} ref={cardRef} data-entry-card>
          <img className={s.mark} src={asset("logo.svg")} alt="" />
          <h1
            ref={heading}
            tabIndex={-1}
            className={lede ? `${s.title} ${s.titleWithLede}` : s.title}
          >
            {title}
          </h1>
          {lede && <p className={s.lede}>{lede}</p>}
          {children}
        </div>
        {below}
      </motion.div>
    </section>
  );
}

export type SignInScreenProps = {
  mode: "sign-in" | "sign-up";
  email: string;
  /** The email field's error, in its own slot. */
  error: string | null;
  /** True while a code is being sent or a provider is opening. Choices lock; labels stay. */
  isBusy: boolean;
  notice: EntryNotice | null;
  /** Change this to send focus to the email field, as after a refused address. */
  focusField: number;
  onEmailChange: (email: string) => void;
  onSubmitEmail: () => void;
  onProvider: (provider: SignInProvider) => void;
  onSwitchMode: () => void;
};

/** AE-01 and AE-02: providers, or an email for a one-time code. */
export function SignInScreen({
  mode,
  email,
  error,
  isBusy,
  notice,
  focusField,
  onEmailChange,
  onSubmitEmail,
  onProvider,
  onSwitchMode,
}: SignInScreenProps): ReactElement {
  const isSignUp = mode === "sign-up";
  const card = useFieldFocus(focusField, false);
  return (
    <EntryCard
      cardRef={card}
      title={isSignUp ? "Sign up for Spellwake" : "Sign in to Spellwake"}
      below={
        <button type="button" className={s.link} onClick={onSwitchMode}>
          {isSignUp ? "Already have an account?" : "Don’t have an account?"}
          <span className={s.linkAccent}>
            {isSignUp ? "Sign in" : "Sign up"}
          </span>
          <ArrowRight aria-hidden="true" size={16} />
        </button>
      }
    >
      <ProviderActions
        providers={PROVIDERS}
        disabled={isBusy}
        onChoose={onProvider}
      />
      <div className={s.or}>OR</div>
      <form
        noValidate
        className={s.form}
        onSubmit={(event: FormEvent) => {
          event.preventDefault();
          if (!isBusy) onSubmitEmail();
        }}
      >
        <TextField
          label="Email"
          className={s.field}
          type="email"
          name="email"
          autoComplete="email"
          autoCapitalize="none"
          spellCheck={false}
          enterKeyHint="send"
          placeholder="you@example.com"
          maxLength={254}
          value={email}
          onValueChange={onEmailChange}
          {...(error ? { error } : {})}
        />
        <ActionButton type="submit" className={s.submit} disabled={isBusy}>
          {isSignUp ? "Sign up with Email" : "Login with Email"}
        </ActionButton>
      </form>
      <Notice notice={notice} />
    </EntryCard>
  );
}

export type CodeScreenProps = {
  email: string;
  code: string;
  error: string | null;
  isChecking: boolean;
  /** True while a resend is in flight. */
  isResending: boolean;
  /** Seconds until another code may be requested. */
  resendIn: number;
  notice: EntryNotice | null;
  /** Change this to send focus back to the first slot, as after a refused code. */
  focusField: number;
  onCodeChange: (code: string) => void;
  onVerify: (code: string) => void;
  onResend: () => void;
  onChangeEmail: () => void;
};

/** AE-03: the six-digit code sent to an email. */
export function CodeScreen({
  email,
  code,
  error,
  isChecking,
  isResending,
  resendIn,
  notice,
  focusField,
  onCodeChange,
  onVerify,
  onResend,
  onChangeEmail,
}: CodeScreenProps): ReactElement {
  const card = useFieldFocus(focusField, true);
  return (
    <EntryCard
      cardRef={card}
      focusHeading={false}
      title="Enter your code"
      lede={<>We sent a 6-digit code to {email}</>}
      below={
        <button
          type="button"
          className={s.link}
          disabled={isChecking || isResending}
          onClick={onChangeEmail}
        >
          <ArrowLeft aria-hidden="true" size={16} />
          Use a different email
        </button>
      }
    >
      <EmailCodeEntry
        value={code}
        onChange={onCodeChange}
        onVerify={onVerify}
        onResend={onResend}
        // A resend in flight holds the button, as a running countdown does.
        resendIn={isResending ? Math.max(resendIn, 1) : resendIn}
        pending={isChecking}
        {...(error ? { error } : {})}
      />
      <Notice notice={notice} />
    </EntryCard>
  );
}

export type UsernameScreenProps = {
  username: string;
  error: string | null;
  isCreating: boolean;
  notice: EntryNotice | null;
  /** Change this to send focus to the field, as after a refused name. */
  focusField: number;
  onUsernameChange: (username: string) => void;
  onSubmit: () => void;
};

/** AE-04: a new account's one, permanent public name. */
export function UsernameScreen({
  username,
  error,
  isCreating,
  notice,
  focusField,
  onUsernameChange,
  onSubmit,
}: UsernameScreenProps): ReactElement {
  const card = useFieldFocus(focusField, true);
  return (
    <EntryCard
      cardRef={card}
      focusHeading={false}
      title="Choose your username"
      lede="Your public name. You can’t change it later."
    >
      <form
        noValidate
        className={s.form}
        onSubmit={(event: FormEvent) => {
          event.preventDefault();
          if (!isCreating) onSubmit();
        }}
      >
        <TextField
          label="Username"
          className={s.field}
          name="username"
          autoComplete="off"
          autoCapitalize="none"
          autoCorrect="off"
          spellCheck={false}
          enterKeyHint="done"
          maxLength={24}
          value={username}
          onValueChange={onUsernameChange}
          {...(error ? { error } : {})}
        />
        <ActionButton type="submit" className={s.submit} disabled={isCreating}>
          Create account
        </ActionButton>
      </form>
      <Notice notice={notice} />
    </EntryCard>
  );
}

/** AE-07: waiting on the account, or a return that could not be completed. */
export function ReturnScreen({
  state,
  onBack,
}: {
  state: "checking" | "failed" | "original-method";
  /** Ends the tentative session and returns to sign-in. */
  onBack: () => void;
}): ReactElement {
  if (state === "checking")
    return (
      <EntryCard title="Signing you in…">
        <LoaderCircle aria-hidden="true" className={s.bigSpinner} />
      </EntryCard>
    );
  return (
    <EntryCard
      title={
        state === "failed" ? "Couldn’t finish signing in" : "Sign in again"
      }
      {...(state === "original-method"
        ? { lede: "Use your original sign-in method." }
        : {})}
    >
      <ActionButton className={s.submit} onClick={onBack}>
        Back to sign in
      </ActionButton>
    </EntryCard>
  );
}

/**
 * What an account shows once its deletion is requested: it waits here, outside
 * the app, until the deletion finishes. Wording as approved for the requested
 * state (AS-R10), in the entry card's anatomy.
 */
export function DeletionPendingScreen({
  onBack,
}: {
  /** Ends this device's session. The deletion carries on without it. */
  onBack: () => void;
}): ReactElement {
  return (
    <EntryCard
      title="Deletion requested"
      lede="Waiting for committed activity to finish."
    >
      <ActionButton className={s.submit} onClick={onBack}>
        Back to sign in
      </ActionButton>
    </EntryCard>
  );
}

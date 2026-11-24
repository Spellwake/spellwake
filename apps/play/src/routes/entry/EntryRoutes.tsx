import {
  useEffect,
  useRef,
  useState,
  type ReactElement,
  type ReactNode,
} from "react";
import {
  Navigate,
  useLocation,
  useNavigate,
  useSearchParams,
} from "react-router-dom";
import { isTauri } from "@tauri-apps/api/core";
import { useAccount } from "../../auth/AccountProvider";
import { sendEmailCode, verifyEmailCode } from "../../auth/email-code";
import {
  CODE_LENGTH,
  emailProblem,
  registrationProblem,
  RESEND_WAIT_SECONDS,
  usernameProblem,
} from "../../auth/entry-flow";
import {
  authDestination,
  beginProviderSignIn,
  browserAuthCallback,
  exchangeAuthReturn,
  NATIVE_AUTH_CALLBACK,
  parseAuthReturn,
  type SignInProvider,
} from "../../auth/oauth";
import { endCurrentSession } from "../../auth/services";
import { ownerBearer } from "../../data/account";
import {
  holdInvite,
  inviteLocation,
  readInvite,
  takeHeldInvite,
} from "../../data/room-invite";
import {
  CodeScreen,
  DeletionPendingScreen,
  ReturnScreen,
  SignInScreen,
  UsernameScreen,
  type EntryNotice,
} from "./EntryScreens";

/**
 * Account entry wired to the Auth service and the application API. The
 * canonical account state decides where a person is: a browser return or an
 * accepted code is never treated as signed in until the API says so.
 */
const SIGN_IN = "/sign-in";
const USERNAME = "/username";
const NOTICE_MS = 4000;
/** How long an accepted provider return may take to appear as a session. */
const SESSION_WAIT_MS = 10_000;
const working = (text: string): EntryNotice => ({ text, tone: "working" });
const passive = (text: string): EntryNotice => ({ text, tone: "info" });
/**
 * Where an allowed destination actually lands. Entry goes straight to the
 * final screen, because a redirect that leads to another redirect can stall
 * while the page between them is still leaving.
 */
const landing = (destination: string): string =>
  destination === "/collection" ? "/collection/decks" : destination;
const browserCallback = (): string =>
  browserAuthCallback(window.location.origin, import.meta.env.BASE_URL);

/** A passive notice that clears itself. A working notice stays until its work ends. */
function useNotice(): [
  EntryNotice | null,
  (notice: EntryNotice | null) => void,
] {
  const [notice, setNotice] = useState<EntryNotice | null>(null);
  useEffect(() => {
    if (!notice || notice.tone === "working") return;
    const timer = window.setTimeout(() => setNotice(null), NOTICE_MS);
    return () => window.clearTimeout(timer);
  }, [notice]);
  return [notice, setNotice];
}

/** True while the account has not settled into the app: the frame then shows no navigation. */
export function useIsEnteringAccount(): boolean {
  const { state } = useAccount();
  return !(
    state.status === "unconfigured" ||
    (state.status === "ready" && state.session.accountId !== null)
  );
}

/**
 * True for an account whose deletion is requested. It is held outside the app,
 * with no navigation, until the deletion finishes.
 */
export function useIsAccountRestricted(): boolean {
  const { state } = useAccount();
  return state.status === "ready" && state.session.state === "deletion_pending";
}

function DeletionPending(): ReactElement {
  const { services } = useAccount();
  const navigate = useNavigate();
  return (
    <DeletionPendingScreen
      onBack={() =>
        void (
          services ? endCurrentSession(services) : Promise.resolve(true)
        ).then(() => navigate(SIGN_IN, { replace: true }))
      }
    />
  );
}

/**
 * What the account's own state shows while it cannot yet be used: the neutral
 * check, or a return that failed. Back to sign in ends the tentative session.
 */
function AccountReturn(): ReactElement | null {
  const { state, services } = useAccount();
  const navigate = useNavigate();
  // A check that answers at once should not flash a card.
  const [isPatient, setIsPatient] = useState(true);
  useEffect(() => {
    const timer = window.setTimeout(() => setIsPatient(false), 350);
    return () => window.clearTimeout(timer);
  }, []);
  if (state.status === "loading" && isPatient) return null;
  return (
    <ReturnScreen
      state={
        state.status === "rejected"
          ? "original-method"
          : state.status === "unavailable"
            ? "failed"
            : "checking"
      }
      onBack={() =>
        void (
          services ? endCurrentSession(services) : Promise.resolve(true)
        ).then(() => navigate(SIGN_IN, { replace: true }))
      }
    />
  );
}

/**
 * Lets a settled account through to the app. Everyone else is sent to the
 * entry step their account state calls for. A build with no application API
 * has no accounts and passes straight through to the local preview.
 */
export function AccountGate({ children }: { children: ReactNode }): ReactNode {
  const { state } = useAccount();
  const location = useLocation();
  const navigate = useNavigate();
  const isSignedOut = state.status === "signed_out";
  const isSettled = state.status === "ready" && !!state.session.accountId;
  // Sign-in carries only a plain destination, so an invitation link opened
  // while signed out is kept in this tab and taken up once the account is in.
  useEffect(() => {
    if (!isSignedOut) return;
    const invite = readInvite(location);
    if (invite) holdInvite(invite);
  }, [isSignedOut, location]);
  useEffect(() => {
    if (!isSettled) return;
    const invite = takeHeldInvite();
    if (invite) void navigate(inviteLocation(invite), { replace: true });
  }, [isSettled, navigate]);
  if (state.status === "unconfigured") return children;
  if (state.status === "signed_out") {
    const next = authDestination(location.pathname);
    // Only an allowed destination is carried through sign-in.
    return (
      <Navigate
        to={next === location.pathname ? `${SIGN_IN}?next=${next}` : SIGN_IN}
        replace
      />
    );
  }
  if (state.status === "ready") {
    if (state.session.state === "deletion_pending") return <DeletionPending />;
    return state.session.accountId ? (
      children
    ) : (
      <Navigate to={USERNAME} replace />
    );
  }
  return <AccountReturn />;
}

/** AE-01 to AE-03: providers, or an email and its one-time code. */
export function EntryRoute({
  mode,
}: {
  mode: "sign-in" | "sign-up";
}): ReactElement | null {
  const { state, services } = useAccount();
  const navigate = useNavigate();
  const [parameters] = useSearchParams();
  const next = authDestination(parameters.get("next"));
  const [email, setEmail] = useState("");
  /** The exact address a code was sent to, or null on the entry step. */
  const [sentTo, setSentTo] = useState<string | null>(null);
  const [code, setCode] = useState("");
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState<
    "sending" | "provider" | "checking" | "resending" | "verified" | null
  >(null);
  const [notice, setNotice] = useNotice();
  const [focusField, setFocusField] = useState(0);
  const [resendAt, setResendAt] = useState(0);
  const [now, setNow] = useState(() => Date.now());
  /** Answers to an attempt the person has since moved on from are ignored. */
  const attempt = useRef(0);
  /**
   * True while one operation is in flight. A ref, because two activations can
   * arrive before the next render and state alone would let both through.
   */
  const inFlight = useRef(false);
  const begin = (kind: NonNullable<typeof busy>): boolean => {
    if (inFlight.current) return false;
    inFlight.current = true;
    setBusy(kind);
    return true;
  };
  const end = (): void => {
    inFlight.current = false;
    setBusy(null);
  };
  useEffect(
    () => () => {
      attempt.current += 1;
    },
    [],
  );
  useEffect(() => {
    if (resendAt <= now) return;
    const timer = window.setTimeout(() => setNow(Date.now()), 250);
    return () => window.clearTimeout(timer);
  }, [resendAt, now]);

  if (state.status === "unconfigured" || !services)
    return <Navigate to="/" replace />;
  if (state.status === "ready")
    return (
      <Navigate
        to={state.session.accountId ? landing(next) : USERNAME}
        replace
      />
    );
  // An accepted code, or a session found on arrival, waits on the account itself.
  if (busy === "verified" || state.status !== "signed_out")
    return state.status === "loading" && busy !== "verified" ? null : (
      <AccountReturn />
    );

  const live = services;
  const restartCooldown = (): void => {
    const start = Date.now();
    setNow(start);
    setResendAt(start + RESEND_WAIT_SECONDS * 1000);
  };
  async function send(): Promise<void> {
    const problem = emailProblem(email);
    if (problem) {
      setError(problem);
      setFocusField((value) => value + 1);
      return;
    }
    if (!begin("sending")) return;
    const address = email.trim();
    const mine = ++attempt.current;
    setNotice(working("Sending code…"));
    const result = await sendEmailCode({ auth: live.auth }, address);
    if (mine !== attempt.current) return;
    end();
    if (result !== "sent") {
      setNotice(passive("Couldn’t send code."));
      return;
    }
    setNotice(null);
    setError(null);
    setCode("");
    restartCooldown();
    setSentTo(address);
  }
  async function verify(candidate: string): Promise<void> {
    if (sentTo === null || candidate.length !== CODE_LENGTH) return;
    if (!begin("checking")) return;
    const mine = ++attempt.current;
    setNotice(working("Checking code…"));
    const result = await verifyEmailCode(
      { auth: live.auth },
      sentTo,
      candidate,
    );
    if (mine !== attempt.current) return;
    setNotice(null);
    if (result === "verified") {
      // The account provider now reads the canonical session and decides the destination.
      // The entry stays locked until it does.
      setBusy("verified");
      return;
    }
    end();
    if (result === "rejected") {
      setCode("");
      setError("Invalid or expired code.");
      setFocusField((value) => value + 1);
    } else setNotice(passive("Couldn’t verify code."));
  }
  async function resend(): Promise<void> {
    if (sentTo === null || !begin("resending")) return;
    const mine = ++attempt.current;
    const result = await sendEmailCode({ auth: live.auth }, sentTo);
    if (mine !== attempt.current) return;
    end();
    if (result !== "sent") {
      // No new countdown and no claim of success.
      setNotice(passive("Couldn’t resend code."));
      return;
    }
    setCode("");
    setError(null);
    restartCooldown();
    setNotice({ text: "Code resent.", tone: "done" });
  }
  async function openProvider(provider: SignInProvider): Promise<void> {
    if (!begin("provider")) return;
    const native = isTauri();
    const mine = ++attempt.current;
    if (native) setNotice(passive("Continue in your browser."));
    const isOpen = await beginProviderSignIn(
      live,
      provider,
      native ? NATIVE_AUTH_CALLBACK : browserCallback(),
      next,
      native,
    );
    if (mine !== attempt.current) return;
    // On the web the page is leaving. In the app the entry stays usable beside the browser.
    if (native || !isOpen) end();
    if (!isOpen) setNotice(passive("Couldn’t start sign-in."));
  }

  if (sentTo !== null)
    return (
      <CodeScreen
        email={sentTo}
        code={code}
        error={error}
        isChecking={busy === "checking"}
        isResending={busy === "resending"}
        resendIn={Math.max(0, (resendAt - now) / 1000)}
        notice={notice}
        focusField={focusField}
        onCodeChange={(value) => {
          setCode(value);
          setError(null);
          // Six digits submit once on their own. Verify stays as the fallback.
          if (value.length === CODE_LENGTH) void verify(value);
        }}
        onVerify={(value) => void verify(value)}
        onResend={() => void resend()}
        onChangeEmail={() => {
          // Back to the same mode with the address kept for correction.
          attempt.current += 1;
          end();
          setSentTo(null);
          setCode("");
          setError(null);
          setNotice(null);
        }}
      />
    );
  return (
    <SignInScreen
      mode={mode}
      email={email}
      error={error}
      isBusy={busy !== null}
      notice={notice}
      focusField={focusField}
      onEmailChange={(value) => {
        setEmail(value);
        setError(null);
      }}
      onSubmitEmail={() => void send()}
      onProvider={(provider) => void openProvider(provider)}
      onSwitchMode={() =>
        void navigate({
          pathname: mode === "sign-in" ? "/sign-up" : SIGN_IN,
          search: parameters.has("next") ? `?next=${next}` : "",
        })
      }
    />
  );
}

/**
 * The browser's return from a provider. The code leaves the address before it
 * is exchanged, and success still waits on the canonical account.
 */
export function AuthCallbackRoute(): ReactElement {
  const { state, services } = useAccount();
  const navigate = useNavigate();
  const location = useLocation();
  const [returned] = useState(() =>
    parseAuthReturn(window.location.href, browserCallback()),
  );
  const [outcome, setOutcome] = useState<"exchanging" | "failed" | "done">(
    returned && services ? "exchanging" : "failed",
  );
  useEffect(() => {
    if (location.search) void navigate(location.pathname, { replace: true });
  }, [location.pathname, location.search, navigate]);
  useEffect(() => {
    if (!returned || !services) return;
    let isCurrent = true;
    void exchangeAuthReturn(services, returned).then((isExchanged) => {
      if (isCurrent) setOutcome(isExchanged ? "done" : "failed");
    });
    return () => {
      isCurrent = false;
    };
  }, [returned, services]);
  // An accepted exchange is not a session yet: the account provider reads it a
  // moment later. If it never does, the return has failed.
  const isAwaitingSession = outcome === "done" && state.status === "signed_out";
  useEffect(() => {
    if (!isAwaitingSession) return;
    const timer = window.setTimeout(
      () => setOutcome("failed"),
      SESSION_WAIT_MS,
    );
    return () => window.clearTimeout(timer);
  }, [isAwaitingSession]);
  if (outcome === "done" && returned && !isAwaitingSession)
    return <Navigate to={landing(returned.next)} replace />;
  return (
    <ReturnScreen
      state={outcome === "failed" ? "failed" : "checking"}
      onBack={() => void navigate(SIGN_IN, { replace: true })}
    />
  );
}

/** AE-04: a signed-in person with no account yet chooses their one public name. */
export function UsernameRoute(): ReactElement | null {
  const { state, services, refresh } = useAccount();
  const [username, setUsername] = useState("");
  const [error, setError] = useState<string | null>(null);
  const [isCreating, setIsCreating] = useState(false);
  const [notice, setNotice] = useNotice();
  const [focusField, setFocusField] = useState(0);
  const inFlight = useRef(false);

  if (state.status === "unconfigured" || !services)
    return <Navigate to="/" replace />;
  if (state.status === "signed_out") return <Navigate to={SIGN_IN} replace />;
  if (state.status !== "ready") return <AccountReturn />;
  if (state.session.accountId)
    return <Navigate to={landing(authDestination(null))} replace />;
  const session = state.session;
  const live = services;

  async function create(): Promise<void> {
    const problem = usernameProblem(username);
    if (problem) {
      setError(problem);
      setFocusField((value) => value + 1);
      return;
    }
    // One registration at a time. The name stays in the field whatever happens.
    if (inFlight.current) return;
    inFlight.current = true;
    setIsCreating(true);
    setNotice(working("Creating account…"));
    try {
      const result = await live.api.register(
        await ownerBearer(live, session),
        username,
      );
      if (result.status === "success") {
        // The canonical session now names the account, which moves this screen on.
        await refresh();
        return;
      }
      const reason =
        result.status === "fail"
          ? registrationProblem(result.data)
          : { notice: "Couldn’t create account." };
      if ("field" in reason) {
        setNotice(null);
        setError(reason.field);
        setFocusField((value) => value + 1);
      } else setNotice(passive(reason.notice));
    } catch {
      setNotice(passive("Couldn’t create account."));
    } finally {
      inFlight.current = false;
      setIsCreating(false);
    }
  }

  return (
    <UsernameScreen
      username={username}
      error={error}
      isCreating={isCreating}
      notice={notice}
      focusField={focusField}
      onUsernameChange={(value) => {
        setUsername(value);
        setError(null);
      }}
      onSubmit={() => void create()}
    />
  );
}

let hasReadLaunchReturn = false;
/**
 * The native app's return from a provider, which arrives as a deep link. It
 * sits outside the routed page so changing screens cannot replay it. Nothing
 * renders: an accepted return moves to its destination and the account gate
 * takes over.
 */
export function NativeAuthReturn(): null {
  const { services } = useAccount();
  const navigate = useNavigate();
  useEffect(() => {
    if (!isTauri() || !services) return;
    let isCurrent = true;
    let stop: (() => void) | undefined;
    const receive = (urls: readonly string[] | null, isLive: boolean): void => {
      for (const raw of urls ?? []) {
        const returned = parseAuthReturn(raw, NATIVE_AUTH_CALLBACK);
        if (!returned) continue;
        void exchangeAuthReturn(services, returned).then((isExchanged) => {
          if (!isCurrent) return;
          if (isExchanged)
            void navigate(landing(returned.next), { replace: true });
          // The link the app was launched with may be old, so only a live return reports failure.
          else if (isLive) void navigate("/auth/callback", { replace: true });
        });
      }
    };
    void import("@tauri-apps/plugin-deep-link")
      .then(async ({ getCurrent, onOpenUrl }) => {
        if (!hasReadLaunchReturn) {
          hasReadLaunchReturn = true;
          receive(await getCurrent(), false);
        }
        const unlisten = await onOpenUrl((urls) => receive(urls, true));
        if (isCurrent) stop = unlisten;
        else unlisten();
      })
      .catch(() => console.warn("native_auth code=listener_unavailable"));
    return () => {
      isCurrent = false;
      stop?.();
    };
  }, [services, navigate]);
  return null;
}

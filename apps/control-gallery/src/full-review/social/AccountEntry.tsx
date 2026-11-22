import { useState, useEffect } from "react";
import {
  EmailCodeEntry,
  ProviderActions,
  TextField,
  ActionButton,
} from "@spellwake/ui";
import google from "../../../../../.impeccable/review/open-play/account-entry/google-symbol.svg?inline";
import appleSvg from "../../../../../.impeccable/review/open-play/account-entry/apple-symbol.svg?raw";
import discord from "../../../../../.impeccable/review/open-play/account-entry/discord-symbol.svg?inline";
import type { ScreenProps } from "../../review-types";
import { Board, Notice } from "./shared";
const providers = [
  { id: "google", label: "Continue with Google", iconSrc: google },
  {
    id: "apple",
    label: "Continue with Apple",
    iconSrc:
      "data:image/svg+xml," +
      encodeURIComponent(appleSvg.replace("<path ", '<path fill="white" ')),
  },
  { id: "discord", label: "Continue with Discord", iconSrc: discord },
];
type Step =
  "sign-in" | "sign-up" | "code" | "username" | "check" | "failed" | "original";
type EmailAttempt = { email: string; origin: "AE-01" | "AE-02" };
function readAttempt(data: unknown): EmailAttempt | undefined {
  if (
    data &&
    typeof data === "object" &&
    "email" in data &&
    typeof data.email === "string" &&
    "origin" in data &&
    (data.origin === "AE-01" || data.origin === "AE-02")
  )
    return { email: data.email, origin: data.origin };
}
function Entry({
  step,
  go,
  attempt,
  variant = "",
}: {
  step: Step;
  go: ScreenProps["go"];
  attempt?: EmailAttempt | undefined;
  variant?: string;
}) {
  const isSignUp = step === "sign-up",
    entry = isSignUp || step === "sign-in",
    code = step === "code";
  const [text, set] = useState(
    step === "username"
      ? variant === "invalid"
        ? "moon watcher"
        : "Moonwatcher"
      : variant === "invalid"
        ? "mage@"
        : variant === "sending"
          ? "mage@example.com"
          : (attempt?.email ?? ""),
  );
  const [digits, setDigits] = useState(variant === "checking" ? "123456" : "");
  const [error, setError] = useState(
    variant === "invalid"
      ? entry
        ? "Enter a valid email address."
        : code
          ? "Invalid or expired code."
          : "Use 3–24 letters, numbers or underscores."
      : variant === "taken"
        ? "That username is taken."
        : "",
  );
  const [left, setLeft] = useState(code && variant !== "invalid" ? 15 : 0);
  const [status, setStatus] = useState(
    (
      {
        sending: "Sending code…",
        checking: "Checking code…",
        creating: "Creating account…",
        resent: "Code resent.",
        handoff: "Continue in your browser.",
      } as Record<string, string>
    )[variant] ?? "",
  );
  useEffect(() => {
    if (left === 0) return;
    const t = setTimeout(() => setLeft(left - 1), 1000);
    return () => clearTimeout(t);
  }, [left]);
  function proceed(input: HTMLInputElement) {
    if (entry) {
      const email = input.value.trim();
      if (
        !input.validity.valid ||
        email.length > 254 ||
        !email.split("@")[1]?.includes(".")
      ) {
        setError("Enter a valid email address.");
        input.focus();
        return;
      }
      go("AE-03", { email, origin: isSignUp ? "AE-02" : "AE-01" });
    } else if (step === "username") {
      if (!/^\w{3,24}$/.test(text)) {
        setError("Use 3–24 letters, numbers or underscores.");
        input.focus();
        return;
      }
      go("DL-01");
    }
  }
  const title = entry
    ? isSignUp
      ? "Sign up for Spellwake"
      : "Sign in to Spellwake"
    : code
      ? "Enter your code"
      : step === "username"
        ? "Choose your username"
        : step === "check"
          ? "Signing you in…"
          : step === "failed"
            ? "Couldn’t finish signing in"
            : "Sign in again";
  const excerpt = [
    "invalid",
    "taken",
    "checking",
    "sending",
    "creating",
  ].includes(variant);
  return (
    <div className="soc-auth-stack">
      <section className="soc-auth-card">
        {(!excerpt || variant === "checking" || variant === "creating") && (
          <h2>{title}</h2>
        )}
        {entry && !excerpt && (
          <>
            <ProviderActions
              providers={providers}
              onChoose={() => go("AE-07")}
            />
            <div className="soc-or">OR</div>
          </>
        )}
        {code && (
          <>
            {!excerpt && (
              <p>
                We sent a 6-digit code to {attempt?.email ?? "mage@example.com"}
                .
              </p>
            )}
            <EmailCodeEntry
              value={digits}
              onChange={(s) => {
                setDigits(s);
                setError("");
              }}
              onVerify={(value) => {
                if (value !== "123456") {
                  setDigits("");
                  setError("Invalid or expired code.");
                  return;
                }
                go(attempt?.origin === "AE-01" ? "DL-01" : "AE-04");
              }}
              onResend={() => {
                setDigits("");
                setError("");
                setLeft(15);
                setStatus("Code resent.");
              }}
              resendIn={left}
              error={error}
              pending={variant === "checking"}
            />
          </>
        )}
        {step === "username" && !excerpt && (
          <p>Your public name. You can’t change it later.</p>
        )}
        {(entry || step === "username") && (
          <form
            noValidate
            onSubmit={(e) => {
              e.preventDefault();
              const input = e.currentTarget.elements.namedItem(
                entry ? "email" : "username",
              );
              if (input instanceof HTMLInputElement) proceed(input);
            }}
          >
            <TextField
              label={entry ? "Email" : "Username"}
              type={entry ? "email" : "text"}
              name={entry ? "email" : "username"}
              autoComplete="username"
              required
              placeholder={entry ? "you@example.com" : ""}
              value={text}
              onValueChange={(v) => {
                set(v);
                setError("");
              }}
              error={error}
            />
            <ActionButton
              type="submit"
              disabled={["creating", "sending"].includes(variant)}
              className="soc-auth-submit"
            >
              {isSignUp
                ? "Sign up with Email"
                : entry
                  ? "Login with Email"
                  : "Create account"}
            </ActionButton>
          </form>
        )}
        {step === "original" && <p>Use your original sign-in method.</p>}
        {(step === "original" || step === "failed") && (
          <ActionButton className="soc-auth-submit" onClick={() => go("AE-01")}>
            Back to sign in
          </ActionButton>
        )}
        {status && <Notice>{status}</Notice>}
      </section>
      {entry && !excerpt && (
        <button
          className="soc-link"
          onClick={() => go(isSignUp ? "AE-01" : "AE-02")}
        >
          {isSignUp
            ? "Already have an account? Sign in"
            : "Don’t have an account? Sign up"}{" "}
          →
        </button>
      )}
      {code && (
        <button
          className="soc-link"
          disabled={variant === "checking"}
          onClick={() => go(attempt?.origin ?? "AE-01", attempt)}
        >
          ← Use a different email
        </button>
      )}
    </div>
  );
}
export function AccountEntry({
  id,
  go,
  data,
}: { id: string } & Pick<ScreenProps, "go" | "data">) {
  const boards: Record<string, [string, Step, string][]> = {
    "AE-05": [
      ["Invalid email", "sign-in", "invalid"],
      ["Invalid or expired code", "code", "invalid"],
      ["Username taken", "username", "taken"],
      ["Invalid username", "username", "invalid"],
    ],
    "AE-06": [
      ["Sending email", "sign-in", "sending"],
      ["Checking code", "code", "checking"],
      ["Code resent", "code", "resent"],
      ["Creating account", "username", "creating"],
    ],
    "AE-07": [
      ["Continue in browser", "sign-in", "handoff"],
      ["Checking account", "check", ""],
      ["Couldn’t finish sign-in", "failed", ""],
      ["Original method required", "original", ""],
    ],
  };
  const panels = boards[id];
  if (panels)
    return (
      <Board
        scenic
        title=""
        panels={panels.map(([label, step, variant]) => [
          label,
          <Entry step={step} variant={variant} go={go} />,
        ])}
      />
    );
  return (
    <div className="soc-auth">
      <Entry
        step={
          id === "AE-04"
            ? "username"
            : id === "AE-03"
              ? "code"
              : id === "AE-02"
                ? "sign-up"
                : "sign-in"
        }
        go={go}
        attempt={readAttempt(data)}
      />
    </div>
  );
}

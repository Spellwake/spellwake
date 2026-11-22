import { useState, type ReactElement } from "react";
import { useSearchParams } from "react-router-dom";
import {
  CodeScreen,
  DeletionPendingScreen,
  ReturnScreen,
  SignInScreen,
  UsernameScreen,
  type EntryNotice,
} from "../entry/EntryScreens";

const noop = (): void => undefined;
const working = (text: string): EntryNotice => ({ text, tone: "working" });

/**
 * Entry probe, development only. It shows each approved account-entry state
 * with stand-in values, chosen by `?state=`, so the screens can be reviewed
 * and stress-tested without an Auth service. Nothing here signs anyone in.
 */
export function EntryProbe(): ReactElement {
  const [parameters] = useSearchParams();
  const state = parameters.get("state") ?? "sign-in";
  const [text, setText] = useState(
    state === "invalid-email"
      ? "mage@"
      : state === "sending"
        ? "mage@example.com"
        : state === "invalid-username"
          ? "moon watcher"
          : ["username", "taken", "creating"].includes(state)
            ? "Moonwatcher"
            : "",
  );
  const [code, setCode] = useState(state === "checking" ? "123456" : "");
  if (state === "deletion-pending")
    return <DeletionPendingScreen onBack={noop} />;
  if (state.startsWith("return-"))
    return (
      <ReturnScreen
        state={
          state === "return-failed"
            ? "failed"
            : state === "return-original"
              ? "original-method"
              : "checking"
        }
        onBack={noop}
      />
    );
  if (["username", "taken", "invalid-username", "creating"].includes(state))
    return (
      <UsernameScreen
        username={text}
        error={
          state === "taken"
            ? "That username is taken."
            : state === "invalid-username"
              ? "Use 3–24 letters, numbers or underscores."
              : null
        }
        isCreating={state === "creating"}
        notice={state === "creating" ? working("Creating account…") : null}
        focusField={0}
        onUsernameChange={setText}
        onSubmit={noop}
      />
    );
  if (["code", "invalid-code", "checking", "resent"].includes(state))
    return (
      <CodeScreen
        email="mage@example.com"
        code={code}
        error={state === "invalid-code" ? "Invalid or expired code." : null}
        isChecking={state === "checking"}
        isResending={false}
        resendIn={state === "invalid-code" ? 0 : 15}
        notice={
          state === "checking"
            ? working("Checking code…")
            : state === "resent"
              ? { text: "Code resent.", tone: "done" }
              : null
        }
        focusField={0}
        onCodeChange={setCode}
        onVerify={noop}
        onResend={noop}
        onChangeEmail={noop}
      />
    );
  return (
    <SignInScreen
      mode={state === "sign-up" ? "sign-up" : "sign-in"}
      email={text}
      error={state === "invalid-email" ? "Enter a valid email address." : null}
      isBusy={state === "sending" || state === "handoff"}
      notice={
        state === "sending"
          ? working("Sending code…")
          : state === "handoff"
            ? { text: "Continue in your browser.", tone: "info" }
            : null
      }
      focusField={0}
      onEmailChange={setText}
      onSubmitEmail={noop}
      onProvider={noop}
      onSwitchMode={noop}
    />
  );
}

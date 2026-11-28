import {
  useEffect,
  useLayoutEffect,
  useRef,
  useState,
  type ReactNode,
} from "react";
import { EmailCodeEntry, ProviderActions } from "@spellwake/ui";
import type { ControlUse } from "./UsageMap";
import { references, type View } from "./references";
import google from "../../../assets/ui-review/open-play/account-entry/google-symbol.svg?inline";
import appleSvg from "../../../assets/ui-review/open-play/account-entry/apple-symbol.svg?raw";
import discord from "../../../assets/ui-review/open-play/account-entry/discord-symbol.svg?inline";

const apple =
  "data:image/svg+xml," +
  encodeURIComponent(appleSvg.replace("<path ", '<path fill="white" '));
const providers = [
  { id: "google", name: "Google", iconSrc: google },
  { id: "apple", name: "Apple", iconSrc: apple },
  { id: "discord", name: "Discord", iconSrc: discord },
] as const;
type ProviderId = (typeof providers)[number]["id"];
const choices = providers.map((p) => ({
  ...p,
  label: "Continue with " + p.name,
}));
export const accountEntryUses: ControlUse[] = [
  {
    id: "provider-buttons",
    mapSection: "component",
    name: "Provider sign-in actions",
    description:
      "Sign-in and sign-up share the same three providers. The compact browser-handoff view keeps the original methods visible.",
    screens: ["AE01:providers", "AE02:providers", "AE07:providers"],
  },
  {
    id: "email-code",
    mapSection: "component",
    name: "Email code entry",
    description:
      "One input handles six digits, including paste and autofill. The same form handles validation, checking and resend cooldown.",
    screens: ["AE03:code", "AE05:code", "AE06:checking", "AE06:resent"],
  },
];
function Stage({ view, children }: { view: View; children: ReactNode }) {
  const [, , width, height] = references[view].crop;
  return (
    <div className="popup-stage">
      <div
        className="popup-scene account-scene"
        data-example={view}
        data-specimen
        style={{ width, height }}
      >
        {children}
      </div>
    </div>
  );
}
function Providers({ view }: { view: View }) {
  const compact = view === "AE07:providers";
  const [handoff, setHandoff] = useState<ProviderId | null>(null);
  return (
    <>
      <Stage view={view}>
        <div
          className={"account-provider-group" + (compact ? " compact" : "")}
          data-provider-handoff={handoff ?? undefined}
        >
          <ProviderActions
            providers={choices}
            disabled={compact || handoff !== null}
            onChoose={setHandoff}
          />
        </div>
      </Stage>
      {(handoff || compact) && (
        <div className="account-demo-footer">
          <span role="status">Continue in your browser.</span>
          {handoff && (
            <button type="button" onClick={() => setHandoff(null)}>
              Reset example
            </button>
          )}
        </div>
      )}
    </>
  );
}
function Code({ view }: { view: View }) {
  const invalid = view === "AE05:code",
    checking = view === "AE06:checking",
    resent = view === "AE06:resent";
  const [value, setValue] = useState(checking ? "123456" : "");
  const [error, setError] = useState(invalid ? "Invalid or expired code." : "");
  const [phase, setPhase] = useState<"edit" | "checking" | "verified">(
    checking ? "checking" : "edit",
  );
  const [resentNotice, setResentNotice] = useState(resent);
  const [deadline, setDeadline] = useState(() =>
    !invalid && !checking ? Date.now() + 15000 : 0,
  );
  const [now, setNow] = useState(Date.now);
  const seconds = Math.max(0, Math.ceil((deadline - now) / 1000));
  const input = useRef<HTMLInputElement>(null);
  const timer = useRef<number | undefined>(undefined);
  const locked = useRef(false);
  useEffect(() => () => window.clearTimeout(timer.current), []);
  useEffect(() => {
    if (!seconds) return;
    const timeout = window.setTimeout(
      () => setNow(Date.now()),
      Math.max(1, deadline - Date.now() - (seconds - 1) * 1000),
    );
    return () => window.clearTimeout(timeout);
  }, [deadline, seconds]);
  useLayoutEffect(() => {
    if (invalid || view === "AE03:code")
      input.current?.focus({ preventScroll: true });
  }, [invalid, view]);
  useLayoutEffect(() => {
    if (phase === "edit" && error)
      input.current?.focus({ preventScroll: true });
  }, [error, phase]);
  function verify(code: string) {
    if (locked.current || phase !== "edit") return;
    locked.current = true;
    setPhase("checking");
    setError("");
    setResentNotice(false);
    timer.current = window.setTimeout(() => {
      locked.current = false;
      if (code === "123456") setPhase("verified");
      else {
        setPhase("edit");
        setValue("");
        setError("Invalid or expired code.");
      }
    }, 650);
  }
  function resend() {
    if (phase !== "edit" || Date.now() < deadline) return;
    const time = Date.now();
    setNow(time);
    setDeadline(time + 15000);
    setValue("");
    setError("");
    setResentNotice(true);
    input.current?.focus({ preventScroll: true });
  }
  return (
    <>
      <Stage view={view}>
        <div className="account-code-group">
          {phase !== "verified" && (
            <EmailCodeEntry
              inputRef={input}
              value={value}
              error={error}
              visibleLabel={invalid}
              onChange={(next) => {
                setValue(next);
                setError("");
                setResentNotice(false);
              }}
              onVerify={verify}
              onResend={resend}
              pending={phase === "checking"}
              resendIn={seconds}
            />
          )}
        </div>
      </Stage>
      <div className="account-demo-footer">
        <span role="status">
          {phase === "verified"
            ? "Code verified."
            : phase === "checking"
              ? "Checking code…"
              : resentNotice
                ? "Code resent."
                : "Example code: 123456"}
        </span>
        {phase === "verified" && (
          <button
            type="button"
            onClick={() => {
              setValue("");
              setPhase("edit");
            }}
          >
            Reset example
          </button>
        )}
      </div>
    </>
  );
}
export function AccountEntrySpecimen({ view }: { view: View }) {
  return view.endsWith(":providers") ? (
    <Providers view={view} />
  ) : (
    <Code view={view} />
  );
}

import {
  useCallback,
  useEffect,
  useRef,
  type ReactElement,
  type RefObject,
} from "react";
import { useLocation, useNavigate } from "react-router-dom";
import { parseAsString, useQueryState } from "nuqs";
import { Dialog } from "@spellwake/ui";
import { useAccount } from "../auth/AccountProvider";
import { useDeviceClass } from "../lib/display-scale";
import {
  MessagesPanel,
  type MessagesView,
} from "../routes/messages/MessagesPanel";
import { useConnectedMessaging } from "./connected-messaging";
import { useDemoMessaging } from "./demo-messaging";
import type { MessagingSource } from "./messaging-sources";
import { tradePath } from "../trade/trade-sources";

/** `?messages=` carries what Messages shows: a person, Requests, Find People, or the first friend. */
export function useMessagesParam() {
  return useQueryState("messages", parseAsString);
}
const viewOf = (value: string, source: MessagingSource): MessagesView => {
  if (value === "requests") return { kind: "requests" };
  if (value === "find") return { kind: "find" };
  if (value !== "open") return { kind: "person", id: value };
  // Opening Messages starts at the first friend, or Find People with none.
  const first = source.status === "ready" ? source.friends[0] : undefined;
  return first ? { kind: "person", id: first.id } : { kind: "find" };
};

function Host({
  value,
  source,
  trigger,
}: {
  value: string;
  source: MessagingSource;
  trigger: RefObject<HTMLElement | null>;
}): ReactElement {
  const [, setValue] = useMessagesParam();
  const navigate = useNavigate();
  const location = useLocation();
  const isPhone = useDeviceClass() === "phone";
  const view = viewOf(value, source);
  const panel = useRef<HTMLDivElement>(null);
  // "open" becomes the first friend once the friends are known, so their thread loads.
  const opened = value === "open" && source.status === "ready" ? view : null;
  useEffect(() => {
    if (opened)
      void setValue(opened.kind === "person" ? opened.id : opened.kind, {
        history: "replace",
      });
  }, [opened, setValue]);
  const close = useCallback(() => {
    void setValue(null);
    trigger.current?.focus();
  }, [setValue, trigger]);
  // A press outside the panel closes it, as the approved panel does.
  useEffect(() => {
    if (isPhone) return;
    const onPress = (event: PointerEvent) => {
      const target = event.target as Element | null;
      if (
        target &&
        !panel.current?.contains(target) &&
        !trigger.current?.contains(target) &&
        !target.closest(
          "[role=dialog], [role=menu], [data-radix-popper-content-wrapper]",
        )
      )
        close();
    };
    document.addEventListener("pointerdown", onPress);
    return () => document.removeEventListener("pointerdown", onPress);
  }, [isPhone, close, trigger]);
  const body = (
    <MessagesPanel
      source={source}
      view={view}
      onView={(next) =>
        void setValue(next.kind === "person" ? next.id : next.kind)
      }
      onClose={close}
      onTrade={(personId) =>
        void navigate(tradePath(personId), {
          // Back and Done return to this surface with this person's thread open.
          state: { from: `${location.pathname}?messages=${personId}` },
        })
      }
      onJoin={() => void navigate("/open-play/lobby")}
      presentation={isPhone ? "sheet" : "panel"}
    />
  );
  if (isPhone)
    return (
      <Dialog
        open
        title="Messages"
        chrome="bare"
        onClose={close}
        returnFocusRef={trigger}
      >
        {body}
      </Dialog>
    );
  return <div ref={panel}>{body}</div>;
}

const useIsPreview = (): boolean =>
  useAccount().state.status === "unconfigured";
function PreviewHost(props: {
  value: string;
  trigger: RefObject<HTMLElement | null>;
}) {
  const view = props.value;
  const source = useDemoMessaging(
    view === "requests" || view === "find" || view === "open" ? null : view,
  );
  return <Host {...props} source={source} />;
}
function AccountHost(props: {
  value: string;
  trigger: RefObject<HTMLElement | null>;
}) {
  const view = props.value;
  const source = useConnectedMessaging(
    view === "requests" || view === "find" || view === "open" ? null : view,
  );
  return <Host {...props} source={source} />;
}

/** Messages over whatever screen is open, while the address names it. */
export function MessagesOverlay({
  trigger,
}: {
  trigger: RefObject<HTMLElement | null>;
}): ReactElement | null {
  const [value] = useMessagesParam();
  const isPreview = useIsPreview();
  if (!value) return null;
  return isPreview ? (
    <PreviewHost value={value} trigger={trigger} />
  ) : (
    <AccountHost value={value} trigger={trigger} />
  );
}

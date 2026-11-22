import { ArrowRightLeft } from "lucide-react";
import type { SVGProps } from "react";
import { useAnimationControls, motion } from "motion/react";
import { useId, useLayoutEffect, useRef } from "react";
import { FriendRequestIcon } from "./FriendRequestIcon";
import { ActionButton } from "../button/ActionButton";
import { controlMotion } from "../control-motion";
import { useAllowsControlMotion } from "../useAllowsControlMotion";
import * as s from "./event.css";

type EventContent =
  | { kind: "friend" | "trade" }
  | { kind: "game"; format: string; arrangement: string };
type EventState =
  | { state: "available"; onPrimary: () => void; onDecline: () => void }
  | { state: "blocked" }
  | { state: "resolved"; title: string; detail?: string };

export type ConversationEventProps = EventContent & EventState;

function GroupIcon(props: SVGProps<SVGSVGElement>) {
  return (
    <svg viewBox="0 0 44 27" fill="none" stroke="currentColor" {...props}>
      <circle cx="22" cy="8" r="6" />
      <circle cx="8" cy="10" r="4" />
      <circle cx="36" cy="10" r="4" />
      <path d="M10 25v-2c0-5 5-8 12-8s12 3 12 8v2M1 23v-1c0-4 3-7 8-7m26 0c5 0 8 3 8 7v1" />
    </svg>
  );
}

const content = {
  friend: {
    icon: FriendRequestIcon,
    title: "Add this player to your friends?",
    action: "Accept",
  },
  trade: { icon: ArrowRightLeft, title: "Trade offer", action: "Trade" },
  game: { icon: GroupIcon, title: "Game invite", action: "Join" },
};

/** The conversation supplies the authoritative event state. Opening a trade does not resolve it. */
export function ConversationEvent(props: ConversationEventProps) {
  const id = useId();
  const animate = useAllowsControlMotion();
  const { icon: Icon, title, action } = content[props.kind];
  const resolved = props.state === "resolved";
  const reveal = useAnimationControls();
  const lastState = useRef(props.state);
  useLayoutEffect(() => {
    const changed = lastState.current !== props.state;
    lastState.current = props.state;
    if (changed && resolved && animate) {
      void reveal.start({
        opacity: [0.6, 1],
        transition: {
          duration: controlMotion.settle,
          ease: controlMotion.ease,
        },
      });
    } else {
      reveal.set({ opacity: 1 });
    }
    return () => reveal.stop();
  }, [props.state, resolved, animate, reveal]);
  return (
    <article
      className={s.card}
      aria-labelledby={id}
      data-kind={props.kind}
      data-resolved={resolved}
      data-blocked={props.state === "blocked"}
    >
      <Icon className={s.symbol} aria-hidden="true" strokeWidth={1.3} />
      <motion.header initial={false} animate={reveal} className={s.heading}>
        <h3 id={id}>{resolved ? props.title : title}</h3>
        {props.kind === "game" && (
          <span className={s.format}>
            {props.format} · {props.arrangement}
          </span>
        )}
      </motion.header>

      {resolved && props.detail && <p className={s.detail}>{props.detail}</p>}

      {!resolved && (
        <footer className={s.actions}>
          <ActionButton
            emphasis="primary"
            disabled={props.state === "blocked"}
            onClick={props.state === "available" ? props.onPrimary : undefined}
          >
            {action}
          </ActionButton>
          <ActionButton
            disabled={props.state === "blocked"}
            onClick={props.state === "available" ? props.onDecline : undefined}
          >
            Decline
          </ActionButton>
        </footer>
      )}
    </article>
  );
}

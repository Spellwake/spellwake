import { motion } from "motion/react";
import type { ReactNode } from "react";
import { controlMotion } from "../control-motion";
import { useAllowsControlMotion } from "../useAllowsControlMotion";
import * as s from "./conversation.css";

export type ChatMessageProps = {
  author: string;
  body: string;
  /** Animate newly accepted messages; restored history is already in place. */
  isNew?: boolean;
} & (
  | { surface: "table" }
  | {
      surface: "direct";
      outgoing: boolean;
      timestamp: string;
      avatar?: ReactNode;
    }
);

export function ChatMessage(props: ChatMessageProps) {
  const animate = useAllowsControlMotion();
  const direct = props.surface === "direct";
  return (
    <motion.div
      className={direct ? s.directMessage : s.tableMessage}
      data-outgoing={direct && props.outgoing}
      initial={animate && props.isNew ? { opacity: 0, y: 4 } : false}
      animate={{ opacity: 1, y: 0 }}
      transition={{
        duration: animate ? controlMotion.settle : 0,
        ease: controlMotion.ease,
      }}
    >
      {direct && !props.outgoing && props.avatar}
      <span className={direct ? s.hidden : s.author}>{props.author}: </span>
      <span className={direct ? s.bubble : s.body}>{props.body}</span>
      {direct && <span className={s.timestamp}>{props.timestamp}</span>}
    </motion.div>
  );
}

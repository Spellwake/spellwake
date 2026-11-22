import { Plus } from "lucide-react";
import { useLayoutEffect, useRef, type ReactNode } from "react";
import { motion } from "motion/react";
import { PlayerAvatar } from "../identity/PlayerAvatar";
import { controlMotion } from "../control-motion";
import { useAllowsControlMotion } from "../useAllowsControlMotion";
import { FriendRequestIcon } from "./FriendRequestIcon";
import { AttentionBadge } from "./AttentionBadge";
import * as s from "./peopleRail.css";
import { designPixels } from "../design-pixels";

export type PeopleDestination =
  { kind: "friend"; id: string } | { kind: "requests" | "find" };
export interface PeopleRailProps {
  friends: readonly { id: string; name: string; artwork: ReactNode }[];
  /** Null means no retained section. Zero keeps Requests reachable without a badge. */
  requests: { attentionCount: number } | null;
  selected: PeopleDestination | null;
  onSelect: (destination: PeopleDestination) => void;
  /** Focus the thread heading if its last Requests entry disappears while the control has focus. */
  focusThreadHeading: () => void;
}

export function PeopleRail({
  friends,
  requests,
  selected,
  onSelect,
  focusThreadHeading,
}: PeopleRailProps) {
  const animate = useAllowsControlMotion();
  const requestsFocused = useRef(false);
  useLayoutEffect(() => {
    if (!requests && requestsFocused.current) {
      requestsFocused.current = false;
      focusThreadHeading();
    }
  }, [requests, focusThreadHeading]);
  const utilities = requests
    ? (["requests", "find"] as const)
    : (["find"] as const);
  return (
    <nav className={s.rail} aria-label="Messaging">
      <div className={s.friends}>
        {friends.map((friend) => {
          const active =
            selected?.kind === "friend" && selected.id === friend.id;
          return (
            <button
              className={s.person}
              type="button"
              key={friend.id}
              aria-label={friend.name}
              aria-current={active ? "true" : undefined}
              onClick={() => onSelect({ kind: "friend", id: friend.id })}
            >
              <PlayerAvatar diameter={60} ring={active ? "selected" : "subtle"}>
                {friend.artwork}
              </PlayerAvatar>
            </button>
          );
        })}
      </div>
      <div className={s.utilities}>
        {utilities.map((kind) => {
          const active = selected?.kind === kind;
          const count = requests?.attentionCount ?? 0;
          return (
            <button
              type="button"
              key={kind}
              className={s.utility}
              aria-current={active ? "true" : undefined}
              aria-label={
                kind === "find"
                  ? "Find People"
                  : count > 0
                    ? `Requests, ${count} ${count === 1 ? "person needs" : "people need"} attention`
                    : "Requests"
              }
              onFocus={
                kind === "requests"
                  ? () => {
                      requestsFocused.current = true;
                    }
                  : undefined
              }
              onBlur={
                kind === "requests"
                  ? () => {
                      requestsFocused.current = false;
                    }
                  : undefined
              }
              onClick={() => onSelect({ kind })}
            >
              <motion.span
                aria-hidden="true"
                className={s.glow}
                initial={false}
                animate={{ opacity: active ? 1 : 0 }}
                transition={{ duration: animate ? controlMotion.settle : 0 }}
              />
              {kind === "find" ? (
                <Plus
                  aria-hidden="true"
                  size={designPixels(28)}
                  strokeWidth={1.6}
                />
              ) : (
                <FriendRequestIcon
                  aria-hidden="true"
                  width={28}
                  height={26}
                  strokeWidth={2.2}
                />
              )}
              {kind === "requests" && (
                <span className={s.badgeAnchor}>
                  <AttentionBadge count={count} />
                </span>
              )}
            </button>
          );
        })}
      </div>
    </nav>
  );
}

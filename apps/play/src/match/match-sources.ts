import type {
  PlayerCommand,
  PlayerSnapshot,
} from "@spellwake/application-contracts";
import type { SourceGap } from "../decks/deck-sources";

export type MatchActionCommand = PlayerCommand["action"];

export type MatchChatEntry = {
  id: string;
  senderId: string;
  senderName: string;
  body: string;
  sentAt: string;
  isMine: boolean;
};

export type MatchConnectionState =
  "connecting" | "live" | "reconnecting" | "ended";

export type MatchSource =
  | { status: SourceGap | "idle" }
  | {
      status: "ready";
      matchId: string | null;
      gameId: string;
      snapshot: PlayerSnapshot;
      seatNames: ReadonlyMap<string, string>;
      artByPrintingId: ReadonlyMap<string, string>;
      connectionState: MatchConnectionState;
      commandPending: boolean;
      commandError: string | null;
      dismissCommandError: () => void;
      sendAction: (action: MatchActionCommand) => Promise<void>;
      chooseOpening: ((playFirst: boolean) => Promise<void>) | null;
      chat: {
        messages: readonly MatchChatEntry[];
        send: ((body: string) => Promise<boolean>) | null;
      };
      onLeave: () => void;
    };

export type ReadyMatch = Extract<MatchSource, { status: "ready" }>;

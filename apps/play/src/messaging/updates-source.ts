import type { MessagingGap } from "./messaging-sources";
import type { Update } from "./updates";

/** The account's updates, as the Updates thread in Messages reads them. */
export type UpdatesSource =
  | { status: MessagingGap }
  | {
      status: "ready";
      items: readonly Update[];
      /** Updates not yet seen, for the rail and the header. */
      unread: number;
      /** Marks what is shown as seen, once the thread is open. */
      markSeen: () => void;
      /** Opens what an update is about; null where it leads nowhere. */
      open: ((update: Update) => void) | null;
      /** Set when an update could not be opened; it leaves by itself. */
      notice: string | null;
    };

import type { CommandAck } from "@spellwake/application-contracts";
import type { ApiResponse } from "./application-api";
import type {
  IntentDisposition,
  IntentJournal,
  IntentJournalAction,
} from "./intent-journal";

export type RecoverableAction<Draft, Command, Errors> =
  | {
      type: "submit";
      expected: IntentJournal<Command, Errors> | null;
      draft: Draft;
    }
  | { type: "recover"; expected: IntentJournal<Command, Errors> };

/** Exact immutable attempts are serialized under one durable slot, independent of domain dispatch. */
export async function recoverIntent<Draft, Command, Errors>(
  input: RecoverableAction<Draft, Command, Errors>,
  options: {
    key: string;
    build: (draft: Draft) => Command;
    act: (
      action: IntentJournalAction<Command, Errors>,
    ) => Promise<IntentJournal<Command, Errors> | null>;
    send: (command: Command) => Promise<ApiResponse<CommandAck>>;
    lookup: (
      command: Command,
    ) => Promise<ApiResponse<IntentDisposition<Errors>>>;
    refreshCurrent: () => Promise<void>;
  },
): Promise<IntentJournal<Command, Errors>> {
  // Forms can change while the exclusive lock is being acquired.
  const requested = structuredClone(input);
  if (!navigator.locks)
    throw new Error("Save recovery is unavailable in this browser.");
  return navigator.locks.request(options.key, async () => {
    let saved: IntentJournal<Command, Errors> | null;
    if (requested.type === "submit") {
      saved = await options.act({
        type: "prepare",
        expected: requested.expected,
        command: options.build(requested.draft),
      });
    } else {
      saved = await options.act({ type: "refresh" });
      if (JSON.stringify(saved) !== JSON.stringify(requested.expected))
        throw new Error(
          "This attempt changed in another window. Review it before continuing.",
        );
    }
    if (!saved) {
      const latest = await options.act({ type: "refresh" });
      if (latest) {
        await options.refreshCurrent();
        throw new Error(
          latest.state === "unresolved"
            ? "Another save needs confirmation. Recover it before saving."
            : "A save changed in another window. Review your edits.",
        );
      }
      throw new Error(
        "The saved attempt is unavailable. Recover it before starting another.",
      );
    }
    const command = saved.command;
    const observe = async (disposition: IntentDisposition<Errors>) => {
      const confirmed = await options.act({
        type: "observe",
        command,
        disposition,
      });
      if (!confirmed)
        throw new Error("The result could not be saved. Recover this attempt.");
      return confirmed;
    };
    const send = async () => {
      const response = await options.send(command);
      if (response.status === "success")
        return observe({ state: "accepted", response });
      const disposition = await options.lookup(command);
      if (disposition.status === "success") return observe(disposition.data);
      throw new Error(
        "The result is not confirmed. Recover this attempt before saving again.",
      );
    };
    let result = saved;
    if (saved.state === "unresolved") {
      if (requested.type === "recover") {
        const disposition = await options.lookup(command);
        if (disposition.status !== "success")
          throw new Error(
            "The result could not be checked. Keep this attempt and retry recovery.",
          );
        result =
          disposition.data.state === "unknown"
            ? await send()
            : await observe(disposition.data);
      } else result = await send();
    }
    if (result.state === "unresolved")
      throw new Error("This attempt is still processing. Try recovery again.");
    // Each domain rechecks the initiating session before refreshing current UI.
    await options.refreshCurrent();
    return result;
  });
}

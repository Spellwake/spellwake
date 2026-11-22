import { atom } from "jotai";
import { z } from "zod";
import type { CommandAck } from "@spellwake/application-contracts";
import { createDurableCell } from "./durable-cell";

type IntentSource<Command> = {
  version: 1;
  apiOrigin: string;
  accountId: string;
  command: Command;
};
export type IntentJournal<Command, Errors> = IntentSource<Command> &
  (
    | { state: "unresolved" }
    | { state: "accepted"; ack: CommandAck }
    | { state: "rejected"; errors: Errors }
  );
export type IntentDisposition<Errors> =
  | { state: "unknown"; response: null }
  | { state: "pending"; response: null }
  | { state: "accepted"; response: { status: "success"; data: CommandAck } }
  | { state: "rejected"; response: { status: "fail"; data: Errors } };
export type IntentJournalAction<Command, Errors> =
  | { type: "refresh" }
  | {
      type: "prepare";
      expected: IntentJournal<Command, Errors> | null;
      command: Command;
    }
  | {
      type: "observe";
      command: Command;
      disposition: IntentDisposition<Errors>;
    };

/** Private storage transitions shared by explicit deck/default adapters, always under their key's Web Lock. */
export function createIntentJournal<Command, Errors>(
  key: string,
  apiOrigin: string,
  accountId: string,
  schema: z.ZodType<IntentJournal<Command, Errors>>,
  commandSchema: z.ZodType<Command>,
  dispositionSchema: z.ZodType<IntentDisposition<Errors>>,
  requestId: (command: Command) => string,
  getStorage: () => Pick<Storage, "getItem" | "setItem">,
) {
  const cell = createDurableCell(key, schema, getStorage);
  const actionAtom = atom(
    null,
    (
      _get,
      set,
      action: IntentJournalAction<Command, Errors>,
    ): IntentJournal<Command, Errors> | null => {
      const prior = cell.read();
      if (action.type === "refresh" || prior.status === "unavailable") {
        set(cell.refreshAtom, prior);
        return prior.status === "saved" ? schema.parse(prior.value) : null;
      }
      if (action.type === "prepare") {
        const expected =
          action.expected === null ? null : schema.safeParse(action.expected);
        if (expected !== null && !expected.success) return null;
        const value = prior.status === "saved" ? prior.value : null;
        if (
          JSON.stringify(value) !==
          JSON.stringify(expected === null ? null : expected.data)
        )
          return null;
        const parsed = commandSchema.safeParse(action.command);
        if (!parsed.success) return null;
        if (value?.state === "unresolved") {
          if (JSON.stringify(value.command) !== JSON.stringify(parsed.data))
            return null;
          set(cell.refreshAtom, prior);
          return schema.parse(value);
        }
        if (value && requestId(value.command) === requestId(parsed.data))
          return null;
        const next = schema.safeParse({
          version: 1,
          apiOrigin,
          accountId,
          state: "unresolved",
          command: parsed.data,
        });
        return next.success ? set(cell.writeAtom, next.data) : null;
      }
      if (prior.status !== "saved") return null;
      const parsed = commandSchema.safeParse(action.command);
      const observed = dispositionSchema.safeParse(action.disposition);
      if (
        !parsed.success ||
        !observed.success ||
        JSON.stringify(parsed.data) !== JSON.stringify(prior.value.command)
      )
        return null;
      const disposition = observed.data;
      if (prior.value.state !== "unresolved") {
        const same =
          (prior.value.state === "accepted" &&
            disposition.state === "accepted" &&
            prior.value.ack.resourceId ===
              disposition.response.data.resourceId) ||
          (prior.value.state === "rejected" &&
            disposition.state === "rejected" &&
            JSON.stringify(prior.value.errors) ===
              JSON.stringify(disposition.response.data));
        if (!same) return null;
        set(cell.refreshAtom, prior);
        return schema.parse(prior.value);
      }
      if (disposition.state === "unknown" || disposition.state === "pending") {
        set(cell.refreshAtom, prior);
        return schema.parse(prior.value);
      }
      const next: IntentJournal<Command, Errors> =
        disposition.state === "accepted"
          ? {
              ...sourceValue(prior.value),
              state: "accepted",
              ack: disposition.response.data,
            }
          : {
              ...sourceValue(prior.value),
              state: "rejected",
              errors: disposition.response.data,
            };
      return set(cell.writeAtom, next);
    },
  );
  return { key, stateAtom: cell.stateAtom, actionAtom };
}

function sourceValue<Command>(
  journal: IntentSource<Command>,
): IntentSource<Command> {
  return {
    version: journal.version,
    apiOrigin: journal.apiOrigin,
    accountId: journal.accountId,
    command: journal.command,
  };
}

import { base64url, newPrivateCapability } from "../utils/private-capability";
/** Immutable consent/observer seed. A saved record never grants account authority. */
import { atom, type WritableAtom } from "jotai";
import { z } from "zod";
import {
  identifierSchema,
  observeDeletionSchema,
  type ObserveDeletion,
} from "@spellwake/application-contracts";
import { publicOrigin } from "../lib/public-config";
import { createDurableCell, notifyDurableStorage } from "./durable-cell";

const fields = {
  version: z.literal(1),
  apiOrigin: z.string().refine((value) => publicOrigin(value) === value),
  authOrigin: z.string().refine((value) => publicOrigin(value) === value),
  accountId: identifierSchema,
  subjectId: identifierSchema,
  requestId: identifierSchema,
  statusSeed: observeDeletionSchema.shape.statusKey,
};
export const deletionJournalSchema = z
  .discriminatedUnion("state", [
    z.strictObject({ ...fields, state: z.literal("prepared") }),
    z.strictObject({
      ...fields,
      state: z.literal("accepted"),
      ack: z.strictObject({ resourceId: identifierSchema }),
    }),
  ])
  .refine(
    (record) =>
      record.state !== "accepted" || record.ack.resourceId === record.accountId,
  );
export type DeletionJournal = z.infer<typeof deletionJournalSchema>;
export type DeletionScope = Pick<
  DeletionJournal,
  "apiOrigin" | "authOrigin" | "accountId" | "subjectId"
>;
type Action =
  | { type: "refresh" }
  | { type: "prepare"; requestId: string; statusSeed: string }
  | { type: "accept"; expected: DeletionJournal; resourceId: string };

export function deletionJournalKey(
  apiOrigin: string,
  accountId: string,
): string {
  if (
    publicOrigin(apiOrigin) !== apiOrigin ||
    !identifierSchema.safeParse(accountId).success
  )
    throw new Error("Deletion scope is unavailable.");
  return `spellwake.deletion.v1:${apiOrigin}:${accountId}`;
}
/** Transitions require record -> account lock; prepare/accept cannot reopen closed scope. */
export function createDeletionJournal(
  scope: DeletionScope,
  getStorage: () => Pick<Storage, "getItem" | "setItem">,
) {
  const key = deletionJournalKey(scope.apiOrigin, scope.accountId);
  const schema = deletionJournalSchema.refine(
    (record) =>
      record.apiOrigin === scope.apiOrigin &&
      record.authOrigin === scope.authOrigin &&
      record.accountId === scope.accountId &&
      record.subjectId === scope.subjectId,
  );
  const cell = createDurableCell(key, schema, getStorage);
  const actionAtom: WritableAtom<null, [Action], DeletionJournal | null> = atom(
    null,
    (_get, set, action) => {
      const prior = cell.read();
      if (action.type === "refresh" || prior.status === "unavailable") {
        set(cell.refreshAtom, prior);
        return prior.status === "saved" ? schema.parse(prior.value) : null;
      }
      if (action.type === "prepare") {
        if (prior.status === "saved") {
          set(cell.refreshAtom, prior);
          return schema.parse(prior.value);
        }
        const value = schema.safeParse({
          version: 1,
          ...scope,
          requestId: action.requestId,
          statusSeed: action.statusSeed,
          state: "prepared",
        });
        return value.success ? set(cell.writeAtom, value.data) : null;
      }
      const expected = schema.safeParse(action.expected);
      const sourceFields = [
        "version",
        "apiOrigin",
        "authOrigin",
        "accountId",
        "subjectId",
        "requestId",
        "statusSeed",
      ] as const;
      if (
        prior.status !== "saved" ||
        !expected.success ||
        action.resourceId !== scope.accountId ||
        !sourceFields.every(
          (field) => prior.value[field] === expected.data[field],
        )
      )
        return null;
      if (prior.value.state === "accepted") return schema.parse(prior.value);
      return set(cell.writeAtom, {
        ...prior.value,
        state: "accepted",
        ack: { resourceId: action.resourceId },
      });
    },
  );
  return { key, stateAtom: cell.stateAtom, actionAtom, read: cell.read };
}
/** Random entropy is persisted before admission; only its source-bound derivative goes on the wire. */
export function newDeletionSeed(): string {
  return newPrivateCapability();
}
/** Fixed domain/order/UTF8 vector, independent of local accepted flags. */
export async function deletionObserver(
  record: DeletionJournal,
): Promise<ObserveDeletion> {
  const value = deletionJournalSchema.parse(record);
  const canonical = JSON.stringify({
    domain: "spellwake.deletion-observer.v1",
    version: 1,
    apiOrigin: value.apiOrigin,
    authOrigin: value.authOrigin,
    accountId: value.accountId,
    subjectId: value.subjectId,
    requestId: value.requestId,
    statusSeed: value.statusSeed,
  });
  const digest = await crypto.subtle.digest(
    "SHA-256",
    new TextEncoder().encode(canonical),
  );
  return observeDeletionSchema.parse({
    accountId: value.accountId,
    requestId: value.requestId,
    statusKey: base64url(new Uint8Array(digest)),
  });
}
/** Existing072contract hashes decoded capability bytes, never its base64 spelling. */
export async function deletionStatusHash(
  record: DeletionJournal,
): Promise<string> {
  const observer = await deletionObserver(record);
  const bytes = Uint8Array.from(
    atob(observer.statusKey.replace(/-/g, "+").replace(/_/g, "/")),
    (value) => value.charCodeAt(0),
  );
  const digest = await crypto.subtle.digest("SHA-256", bytes);
  return Array.from(new Uint8Array(digest), (byte) =>
    byte.toString(16).padStart(2, "0"),
  ).join("");
}
export function readDeletionJournal(
  key: string,
  apiOrigin: string,
  authOrigin: string,
  storage: Pick<Storage, "getItem">,
): DeletionJournal | null {
  const raw = storage.getItem(key);
  if (raw === null) return null;
  if (raw.length > 8192) throw new Error("Deletion recovery is unavailable.");
  const value = deletionJournalSchema.parse(JSON.parse(raw));
  if (
    value.apiOrigin !== apiOrigin ||
    value.authOrigin !== authOrigin ||
    deletionJournalKey(apiOrigin, value.accountId) !== key
  )
    throw new Error("Deletion recovery scope differs.");
  return value;
}
/** Notify after a verified transition without publishing the secret/value in an event. */
export function deletionJournalChanged(key: string): void {
  notifyDurableStorage([key]);
}

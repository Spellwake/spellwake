import { expect, test } from "bun:test";
import type { AccountSession } from "@spellwake/application-contracts";
import type { Session } from "@supabase/supabase-js";
import { readForOwner } from "./owner-read";
import { socialMutationFeedback } from "./social-commands";
import type { SocialJournal } from "./social-recovery";
import type { SocialIntent } from "./social-journal";

const subjectId = "22222222-2222-4222-8222-222222222222";
const owner = {
  state: "active",
  accountId: "11111111-1111-4111-8111-111111111111",
  subjectId,
  sessionId: "33333333-3333-4333-8333-333333333333",
  method: "email_code",
} satisfies AccountSession;

function session(id = subjectId): Session {
  return {
    access_token: "original-bearer",
    refresh_token: "fixture-refresh",
    token_type: "bearer",
    expires_in: 3600,
    user: {
      id,
      aud: "authenticated",
      app_metadata: {},
      user_metadata: {},
      created_at: "2026-11-02T00:00:00Z",
    },
  };
}

test("mutation feedback follows the selected slot and immutable attempt as windows confirm or replace it", () => {
  const intent = {
    kind: "social",
    input: {
      kind: "send_dm",
      command: {
        requestId: "44444444-4444-4444-8444-444444444444",
        conversationId: owner.sessionId,
        body: "Original text",
      },
    },
  } satisfies SocialIntent;
  const input = { type: "submit" as const, expected: null, draft: intent };
  const slot = { kind: "send_dm" as const, targetId: owner.sessionId };
  const journal: SocialJournal = {
    version: 1,
    apiOrigin: "https://api.example",
    accountId: owner.accountId,
    command: { subjectId, intent },
    state: "unresolved",
  };
  const error = new Error("The result is not confirmed.");
  expect(
    socialMutationFeedback(
      input,
      slot,
      { status: "saved", value: journal },
      true,
      error,
    ),
  ).toEqual({ isPending: true, error });
  expect(
    socialMutationFeedback(
      input,
      { ...slot, targetId: owner.accountId },
      { status: "saved", value: journal },
      true,
      error,
    ),
  ).toEqual({ isPending: false, error: null });
  expect(
    socialMutationFeedback(
      input,
      slot,
      {
        status: "saved",
        value: {
          ...journal,
          state: "accepted",
          ack: { resourceId: owner.accountId },
        },
      },
      false,
      error,
    ),
  ).toEqual({ isPending: false, error: null });
  const changed: SocialJournal = {
    ...journal,
    command: {
      subjectId,
      intent: {
        ...intent,
        input: {
          ...intent.input,
          command: {
            ...intent.input.command,
            requestId: "55555555-5555-4555-8555-555555555555",
            body: "New attempt",
          },
        },
      },
    },
  };
  expect(
    socialMutationFeedback(
      input,
      slot,
      { status: "saved", value: changed },
      true,
      error,
    ),
  ).toEqual({ isPending: false, error: null });
  expect(
    socialMutationFeedback(
      { type: "recover", expected: journal },
      slot,
      { status: "saved", value: journal },
      false,
      error,
    ).error,
  ).toBe(error);
  expect(
    socialMutationFeedback(
      undefined,
      slot,
      { status: "saved", value: journal },
      false,
      error,
    ).error,
  ).toBeNull();
  expect(journal.state).toBe("unresolved");
  expect(intent.input.command.body).toBe("Original text");
});

test("social reads deliver only to the owner that requested them", async () => {
  for (const phase of [
    "before",
    "credential",
    "response",
    "unchanged",
  ] as const) {
    let current = phase !== "before";
    let credentials = 0;
    let reads = 0;
    const result = readForOwner(
      {
        owner,
        isCurrent: () => current,
        services: {
          auth: {
            getSession: async () => {
              credentials++;
              if (phase === "credential") current = false;
              return { data: { session: session() }, error: null };
            },
          },
        },
      },
      new AbortController().signal,
      async (bearer) => {
        reads++;
        expect(bearer).toBe("original-bearer");
        if (phase === "response") current = false;
        return "private-history";
      },
    );
    if (phase === "unchanged") expect(await result).toBe("private-history");
    else await expect(result).rejects.toThrow("Your account changed");
    expect(credentials).toBe(phase === "before" ? 0 : 1);
    expect(reads).toBe(phase === "before" || phase === "credential" ? 0 : 1);
  }
});

test("social reads reject replacement SDK identity and canceled deliveries", async () => {
  for (const phase of [
    "identity",
    "before",
    "credential",
    "response",
  ] as const) {
    const controller = new AbortController();
    if (phase === "before") controller.abort();
    let reads = 0;
    const result = readForOwner(
      {
        owner,
        isCurrent: () => true,
        services: {
          auth: {
            getSession: async () => {
              if (phase === "credential") controller.abort();
              return {
                data: {
                  session: session(
                    phase === "identity" ? owner.accountId : subjectId,
                  ),
                },
                error: null,
              };
            },
          },
        },
      },
      controller.signal,
      async () => {
        reads++;
        controller.abort();
        return "private-history";
      },
    );
    await expect(result).rejects.toThrow();
    expect(reads).toBe(phase === "response" ? 1 : 0);
  }
});

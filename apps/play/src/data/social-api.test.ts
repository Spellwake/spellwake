import { expect, test } from "bun:test";
import {
  DIRECT_MESSAGE_PAGE_LIMIT,
  directMessagePageSchema,
  messageCursorSchema,
  messagePositionBefore,
  socialPersonSchema,
  socialStatusSchema,
} from "@spellwake/application-contracts";
import { ApplicationApi } from "./application-api";

const owner = "11111111-1111-4111-8111-111111111111";
const peer = "22222222-2222-4222-8222-222222222222";
const conversation = "33333333-3333-4333-8333-333333333333";
const messageId = (value: number) =>
  `44444444-4444-4444-8444-${String(value).padStart(12, "0")}`;
const person = {
  id: peer,
  username: "test_peer",
  isDeleted: false,
  relationship: "none",
  friendshipId: null,
  isBlocked: false,
  canContact: false,
  conversationId: conversation,
  unreadMessages: "0",
  supporter: false,
};

test("status preserves full exact counts and binds nullable selected identity and owner locally", async () => {
  const valid = {
    accountId: owner,
    unreadMessages: "9007199254740993",
    personId: peer,
    person,
  };
  expect(socialStatusSchema.safeParse(valid).success).toBe(true);
  for (const bad of [
    { ...valid, personId: null },
    { ...valid, person: { ...person, id: owner } },
    {
      ...valid,
      unreadMessages: "0",
      person: { ...person, unreadMessages: "2" },
    },
    { ...valid, unreadMessages: "invalid" },
    { ...valid, person: undefined },
  ])
    expect(socialStatusSchema.safeParse(bad).success).toBe(false);
  const seen: { url: string; actor: string | null }[] = [];
  let response: unknown = valid;
  const api = ApplicationApi.create(
    "https://api.example",
    async (input, options) => {
      seen.push({
        url: String(input),
        actor: new Headers(options?.headers).get("authorization"),
      });
      return new Response(
        JSON.stringify({ status: "success", data: response }),
      );
    },
  );
  if (!api) throw new Error("Expected configured client");
  const result = await api.socialStatus("original-bearer", owner, peer);
  expect(result.status).toBe("success");
  expect(seen).toEqual([
    {
      url: `https://api.example/v1/social/status?personId=${peer}`,
      actor: "Bearer original-bearer",
    },
  ]);
  response = { ...valid, person: null };
  expect((await api.socialStatus("original-bearer", owner, peer)).status).toBe(
    "success",
  );
  response = { ...valid, personId: null, person: null };
  expect((await api.socialStatus("original-bearer", owner)).status).toBe(
    "success",
  );
  expect((await api.socialStatus("original-bearer", conversation)).status).toBe(
    "error",
  );
  expect((await api.socialStatus("original-bearer", owner, peer)).status).toBe(
    "error",
  );
});
function history(count = 2, body = "Original text") {
  const items = Array.from({ length: count }, (_, index) => ({
    id: messageId(count - index),
    senderId: peer,
    author: "test_peer",
    body,
    sentAt: `2026-11-02T12:00:00.${String(count - index).padStart(6, "0")}Z`,
  }));
  const last = items.at(-1);
  return {
    accountId: owner,
    conversationId: conversation,
    person,
    items,
    nextCursor:
      count === DIRECT_MESSAGE_PAGE_LIMIT && last
        ? { sentAt: last.sentAt, id: last.id }
        : null,
  };
}
function apiReturning(data: unknown) {
  const api = ApplicationApi.create("https://api.example", async () =>
    Response.json({ status: "success", data }),
  );
  if (!api) throw new Error("Expected configured fixture API.");
  return api;
}

test("social cursors preserve sub-millisecond ordering, offsets and exact ID ties", () => {
  const first = { sentAt: "2026-11-02T12:00:00.000001Z", id: messageId(2) };
  const second = { sentAt: "2026-11-02T12:00:00.000002Z", id: messageId(1) };
  expect(messagePositionBefore(first, second)).toBe(true);
  expect(messagePositionBefore(second, first)).toBe(false);
  expect(messagePositionBefore({ ...first, id: messageId(1) }, first)).toBe(
    true,
  );
  expect(
    messagePositionBefore(first, {
      ...first,
      sentAt: "2026-11-02T14:00:00.000001+02:00",
    }),
  ).toBe(false);
  expect(messagePositionBefore({ ...first, sentAt: "invalid" }, second)).toBe(
    false,
  );
  expect(
    messageCursorSchema.safeParse({
      ...first,
      sentAt: "2026-11-02T12:00:00.0000000001Z",
    }).success,
  ).toBe(false);
  const malformed = history();
  if (!malformed.items[0]) throw new Error("Expected fixture message.");
  malformed.items[0].body = "Unpaired \uD800";
  expect(directMessagePageSchema.safeParse(malformed).success).toBe(false);
});

test("private social projections reject unmasked deletion, duplicate identities and foreign authors", async () => {
  expect(socialPersonSchema.safeParse(person).success).toBe(true);
  expect(
    socialPersonSchema.safeParse({ ...person, isDeleted: true }).success,
  ).toBe(false);
  expect(
    socialPersonSchema.safeParse({
      ...person,
      isBlocked: true,
      canContact: true,
    }).success,
  ).toBe(false);
  expect(
    socialPersonSchema.safeParse({ ...person, authSubject: owner }).success,
  ).toBe(false);
  const valid = history();
  expect(directMessagePageSchema.safeParse(valid).success).toBe(true);
  const foreign = structuredClone(valid);
  foreign.items[0]!.senderId = conversation;
  const duplicate = structuredClone(valid);
  duplicate.items[1]!.id = duplicate.items[0]!.id;
  const wrongOwner = { ...valid, accountId: conversation };
  const wrongConversation = { ...valid, conversationId: owner };
  const reversed = { ...valid, items: [...valid.items].reverse() };
  for (const invalid of [
    foreign,
    duplicate,
    wrongOwner,
    wrongConversation,
    reversed,
  ]) {
    expect(
      (
        await apiReturning(invalid).directMessages(
          "original-bearer",
          owner,
          conversation,
        )
      ).status,
    ).toBe("error");
  }
  const cursor = { sentAt: valid.items[0]!.sentAt, id: valid.items[0]!.id };
  expect(
    (
      await apiReturning(valid).directMessages(
        "original-bearer",
        owner,
        conversation,
        cursor,
      )
    ).status,
  ).toBe("error");
});

test("directory and thread reads enforce local owner, literal query and cursor without sending an actor", async () => {
  let calls = 0;
  const api = ApplicationApi.create(
    "https://api.example",
    async (input, init) => {
      calls++;
      const url = new URL(String(input));
      expect(new Headers(init?.headers).get("Authorization")).toBe(
        "Bearer original-bearer",
      );
      expect(init?.method).toBe("GET");
      expect(init?.cache).toBe("no-store");
      expect(url.searchParams.has("accountId")).toBe(false);
      expect(init?.body).toBeUndefined();
      return Response.json({
        status: "success",
        data: { accountId: owner, items: [person], nextCursor: null },
      });
    },
  );
  if (!api) throw new Error("Expected configured fixture API.");
  expect(
    (await api.socialDirectory("original-bearer", owner, " TEST_ ")).status,
  ).toBe("success");
  expect((await api.socialThreads("original-bearer", owner)).status).toBe(
    "success",
  );
  expect(
    (await api.socialThreads("original-bearer", conversation)).status,
  ).toBe("error");
  expect(
    (await api.socialDirectory("original-bearer", owner, "%_")).status,
  ).toBe("error");
  expect((await api.socialThreads("original-bearer", owner, peer)).status).toBe(
    "error",
  );
  expect(
    (await api.socialDirectory("original-bearer", owner, " ")).status,
  ).toBe("fail");
  expect(calls).toBe(5);
  const nonAscii = apiReturning({
    accountId: owner,
    items: [{ ...person, username: "i_player" }],
    nextCursor: null,
  });
  expect(
    (await nonAscii.socialDirectory("original-bearer", owner, "İ")).status,
  ).toBe("error");
});

test("sixteen maximum-length escaped Unicode messages fit the existing bounded transport", async () => {
  const page = history(DIRECT_MESSAGE_PAGE_LIMIT, "🎲".repeat(4000));
  // Also exercise an ASCII-only encoder's surrogate-pair representation.
  const body = Array.from(JSON.stringify({ status: "success", data: page }))
    .map((character) =>
      character.charCodeAt(0) <= 127
        ? character
        : character
            .split("")
            .map(
              (unit) =>
                `\\u${unit.charCodeAt(0).toString(16).padStart(4, "0")}`,
            )
            .join(""),
    )
    .join("");
  expect(new TextEncoder().encode(body).length).toBeLessThan(1024 * 1024);
  const api = ApplicationApi.create("https://api.example", async (input) => {
    expect(new URL(String(input)).searchParams.get("limit")).toBe("16");
    return new Response(body, {
      headers: { "Content-Type": "application/json" },
    });
  });
  if (!api) throw new Error("Expected configured fixture API.");
  const result = await api.directMessages(
    "original-bearer",
    owner,
    conversation,
  );
  expect(result.status).toBe("success");
  if (result.status !== "success")
    throw new Error("Expected exact decoded history.");
  expect(result.data.items).toHaveLength(16);
  expect(result.data.items[0]?.body).toBe("🎲".repeat(4000));
});

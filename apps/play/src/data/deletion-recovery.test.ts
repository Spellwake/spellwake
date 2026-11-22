import { expect, test } from "bun:test";
import { QueryClient } from "@tanstack/react-query";
import { ownsDeletionQuery } from "./deletion-recovery";
const accountId = "10000000-0000-4000-8000-000000000001";
const subjectId = "20000000-0000-4000-8000-000000000002";
const other = "30000000-0000-4000-8000-000000000003";
test("erasure selects exact owner positions and validated canonical session data, never a coincidental ID or other generation", () => {
  const client = new QueryClient();
  const cases: [readonly unknown[], unknown, boolean][] = [
    [["deck-library", accountId, "session"], { items: [] }, true],
    [["purchase", accountId, "session", other], {}, true],
    [["deck-library", other, accountId], {}, false],
    [["unknown", accountId], {}, false],
    [["social-threads", accountId, "https://api.example", "session"], {}, true],
    [
      ["social-status", accountId, "https://api.example", "session", other],
      {},
      true,
    ],
    [
      ["social-status", accountId, "https://other.example", "session", other],
      {},
      false,
    ],
    [["social-status", other, "https://api.example", accountId], {}, false],
    [
      ["social-threads", accountId, "https://other.example", "session"],
      {},
      false,
    ],
    [["direct-messages", other, accountId], {}, false],
    [["deletion-observer", "https://api.example", accountId], {}, false],
    [
      ["account-session", 1],
      {
        status: "success",
        data: {
          accountId,
          subjectId,
          sessionId: other,
          method: "email_code",
          state: "active",
        },
      },
      true,
    ],
    [
      ["account-session", 2],
      {
        status: "success",
        data: {
          accountId: other,
          subjectId: other,
          sessionId: other,
          method: "email_code",
          state: "active",
        },
      },
      false,
    ],
    [
      ["account-session", 3],
      {
        status: "success",
        data: {
          accountId,
          subjectId: other,
          sessionId: other,
          method: "email_code",
          state: "active",
        },
      },
      false,
    ],
    [
      ["account-session", 4],
      { status: "fail", data: { session: "unauthorized" } },
      false,
    ],
    [
      ["account-session", 5],
      { status: "success", data: { accountId, subjectId } },
      false,
    ],
  ];
  for (const [key, data, expected] of cases) {
    client.setQueryData(key, data);
    const query = client.getQueryCache().find({ queryKey: key, exact: true });
    if (!query) throw new Error("Expected test query");
    expect(
      ownsDeletionQuery(query, {
        accountId,
        subjectId,
        apiOrigin: "https://api.example",
      }),
    ).toBe(expected);
  }
  client.clear();
});

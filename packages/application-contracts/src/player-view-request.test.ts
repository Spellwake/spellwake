import { expect, test } from "bun:test";
import { playerViewRequestSchema } from "./player-views.ts";

const request = {
  gameId: "00000000-0000-0000-0000-000000000001",
  contextId: "00000000-0000-0000-0000-000000000002",
};
test("view requests contain canonical scope only, without player authority or private artifacts", () => {
  expect(playerViewRequestSchema.parse(request)).toEqual(request);
  for (const extra of [
    { viewerSeatId: request.gameId },
    { disclosure: "completed" },
    { stateRef: "private:never-client-authority" },
    { basis: { epoch: "0", revision: "0" } },
  ]) {
    expect(
      playerViewRequestSchema.safeParse({ ...request, ...extra }).success,
    ).toBe(false);
  }
  for (const invalid of [
    null,
    [],
    {},
    { ...request, contextId: null },
    { ...request, gameId: "00000000-0000-0000-0000-000000000000" },
    { ...request, contextId: "invalid" },
  ]) {
    expect(playerViewRequestSchema.safeParse(invalid).success).toBe(false);
  }
});

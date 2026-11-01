import { expect, test } from "bun:test";
import {
  createTableInteraction,
  type TableDropIntent,
} from "./table-interaction";

test("preview requires explicit fixture opt-in; missing integration never previews", async () => {
  const intent = { presentationCardId: "mesh-1" };
  expect(await createTableInteraction().drop(intent)).toEqual({
    status: "unavailable",
  });
  expect(
    await createTableInteraction({ mode: "integration" }).drop(intent),
  ).toEqual({ status: "unavailable" });
  let previews = 0;
  const fixture = createTableInteraction({
    mode: "fixture-preview",
    onPreview: () => {
      previews++;
    },
  });
  expect(await fixture.drop(intent)).toEqual({ status: "preview" });
  expect(previews).toBe(1);
});

test("host gets an immutable snapshot with only explicitly supplied identity", async () => {
  const received: TableDropIntent[] = [];
  const interaction = createTableInteraction({
    mode: "integration",
    onIntent: (intent) => {
      received.push(intent);
    },
  });
  const input = {
    presentationCardId: "mesh-1",
    sourceId: "opaque-current-object",
  };
  const result = interaction.drop(input);
  input.sourceId = "changed";
  expect(await result).toEqual({ status: "requested" });
  expect(received[0]).toEqual({
    presentationCardId: "mesh-1",
    sourceId: "opaque-current-object",
  });
  expect(Object.isFrozen(received[0])).toBe(true);
  await interaction.drop({ presentationCardId: "mesh-2" });
  expect(received[1]).toEqual({ presentationCardId: "mesh-2" });
  expect(Object.hasOwn(received[1] ?? {}, "sourceId")).toBe(false);
});

test("an unresolved request suppresses duplicate drops; rejection allows retry", async () => {
  let rejectRequest: (error: unknown) => void = () => {};
  let calls = 0;
  const interaction = createTableInteraction({
    mode: "integration",
    onIntent: () => {
      calls++;
      return new Promise<void>((_resolve, reject) => {
        rejectRequest = reject;
      });
    },
  });
  const first = interaction.drop({ presentationCardId: "mesh-1" });
  expect(interaction.isPending).toBe(true);
  expect(await interaction.drop({ presentationCardId: "mesh-1" })).toEqual({
    status: "busy",
  });
  expect(calls).toBe(1);
  const error = new Error("host rejected");
  rejectRequest(error);
  expect(await first).toEqual({ status: "failed", error });
  expect(interaction.isPending).toBe(false);
  const retry = interaction.drop({ presentationCardId: "mesh-1" });
  await Promise.resolve();
  expect(calls).toBe(2);
  interaction.cancel();
  expect(await retry).toEqual({ status: "cancelled" });
});

test("cancel aborts promptly and late settlement cannot clear a newer request", async () => {
  const signals: AbortSignal[] = [];
  const finish: (() => void)[] = [];
  const interaction = createTableInteraction({
    mode: "integration",
    onIntent: (_intent, signal) => {
      signals.push(signal);
      return new Promise<void>((resolve) => {
        finish.push(resolve);
      });
    },
  });
  const first = interaction.drop({ presentationCardId: "mesh-1" });
  await Promise.resolve();
  interaction.cancel();
  expect(signals[0]?.aborted).toBe(true);
  const second = interaction.drop({ presentationCardId: "mesh-2" });
  expect(await first).toEqual({ status: "cancelled" });
  finish[0]?.();
  await Promise.resolve();
  expect(interaction.isPending).toBe(true);
  finish[1]?.();
  expect(await second).toEqual({ status: "requested" });
  expect(interaction.isPending).toBe(false);
});

test("cancel before dispatch skips the callback; synchronous failure remains a result", async () => {
  let calls = 0;
  const error = new Error("synchronous rejection");
  const interaction = createTableInteraction({
    mode: "integration",
    onIntent: () => {
      calls++;
      throw error;
    },
  });
  const cancelled = interaction.drop({ presentationCardId: "mesh-1" });
  interaction.cancel();
  expect(await cancelled).toEqual({ status: "cancelled" });
  expect(calls).toBe(0);
  expect(await interaction.drop({ presentationCardId: "mesh-1" })).toEqual({
    status: "failed",
    error,
  });
});

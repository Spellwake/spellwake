import { describe, expect, test } from "bun:test";
import { createViewportResource } from "./viewport-resource";

type Handle = { dispose: () => void; open: () => void };

function deferred<T>(): { promise: Promise<T>; resolve: (value: T) => void } {
  let resolve: (value: T) => void = () => {};
  const promise = new Promise<T>((complete) => {
    resolve = complete;
  });
  return { promise, resolve };
}

const flush = async (): Promise<void> => {
  await new Promise<void>((resolve) => setTimeout(resolve, 0));
};

describe("viewport renderer lifetime", () => {
  test("leaving during a load disposes its late result without opening it", async () => {
    const result = deferred<Handle>();
    let disposed = 0;
    let opened = 0;
    let signal: AbortSignal | undefined;
    const owner = createViewportResource({
      load: async (request) => {
        signal = request;
        return result.promise;
      },
      onLoading: () => {},
      onReady: (handle) => handle.open(),
      onError: () => {
        throw new Error("Unexpected renderer failure");
      },
    });
    owner.setVisible(true);
    await flush();
    owner.setVisible(false);
    expect(signal?.aborted).toBe(true);
    result.resolve({
      dispose: () => {
        disposed++;
      },
      open: () => {
        opened++;
      },
    });
    await flush();
    expect(disposed).toBe(1);
    expect(opened).toBe(0);
    expect(owner.current()).toBeUndefined();
    owner.dispose();
  });

  test("queued offscreen loads are skipped and visible reloads are serialized", async () => {
    const firstResult = deferred<Handle>();
    const events: string[] = [];
    const handle = (name: string): Handle => ({
      dispose: () => {
        events.push(`dispose:${name}`);
      },
      open: () => {
        events.push(`open:${name}`);
      },
    });
    const first = createViewportResource({
      load: async () => {
        events.push("load:first");
        return firstResult.promise;
      },
      onLoading: () => {},
      onReady: (value) => value.open(),
      onError: () => {},
    });
    const second = createViewportResource({
      load: async () => {
        events.push("load:second");
        return handle("second");
      },
      onLoading: () => {},
      onReady: (value) => value.open(),
      onError: () => {},
    });
    first.setVisible(true);
    second.setVisible(true);
    await flush();
    expect(events).toEqual(["load:first"]);
    second.setVisible(false);
    firstResult.resolve(handle("first"));
    await flush();
    expect(events).toEqual(["load:first", "open:first"]);
    first.setVisible(false);
    second.setVisible(true);
    await flush();
    expect(events).toEqual([
      "load:first",
      "open:first",
      "dispose:first",
      "load:second",
      "open:second",
    ]);
    second.setVisible(false);
    second.setVisible(true);
    await flush();
    expect(events.filter((event) => event === "load:second")).toHaveLength(2);
    first.dispose();
    second.dispose();
  });

  test("failed loads do not block later boxes and disposal suppresses late readiness", async () => {
    const failure = deferred<Handle>();
    let errors = 0;
    let ready = 0;
    const broken = createViewportResource<Handle>({
      load: async () => {
        throw new Error("Model unavailable");
      },
      onLoading: () => {},
      onReady: () => {},
      onError: () => {
        errors++;
      },
    });
    const next = createViewportResource({
      load: async () => failure.promise,
      onLoading: () => {},
      onReady: () => {
        ready++;
      },
      onError: () => {
        errors++;
      },
    });
    broken.setVisible(true);
    next.setVisible(true);
    await flush();
    expect(errors).toBe(1);
    next.dispose();
    let disposed = false;
    failure.resolve({
      dispose: () => {
        disposed = true;
      },
      open: () => {},
    });
    await flush();
    expect(disposed).toBe(true);
    expect(ready).toBe(0);
    expect(errors).toBe(1);
    broken.dispose();
  });
});

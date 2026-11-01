/** Mount-scoped ownership for listeners, scheduling and disposable renderer resources. */
export function createSceneLifecycle(signal?: AbortSignal) {
  const controller = new AbortController();
  const cleanups: (() => void)[] = [];
  let disposed = false;
  function dispose(): void {
    if (disposed) return;
    disposed = true;
    controller.abort();
    for (const cleanup of cleanups.splice(0).reverse()) {
      try {
        cleanup();
      } catch (error: unknown) {
        console.error("Scene cleanup failed", error);
      }
    }
  }
  if (signal) {
    signal.addEventListener("abort", dispose, { once: true });
    cleanups.push(() => signal.removeEventListener("abort", dispose));
    if (signal.aborted) dispose();
  }
  return {
    signal: controller.signal,
    get disposed(): boolean {
      return disposed;
    },
    dispose,
    /** A late async allocation is released immediately after an aborted mount. */
    add(cleanup: () => void): void {
      if (disposed) cleanup();
      else cleanups.push(cleanup);
    },
    assertActive(): void {
      controller.signal.throwIfAborted();
    },
  };
}

/** Resolve public resources relative to a host-owned base, including packaged protocols. */
export function sceneAssetUrl(path: string, base: string | URL): string {
  return new URL(path.replace(/^\/+/, ""), base).href;
}

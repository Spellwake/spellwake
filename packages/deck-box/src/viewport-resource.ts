/** A renderer's lifetime follows visibility, including queued and in-flight loads. */
export type ViewportResource<T> = {
  setVisible: (visible: boolean) => void;
  current: () => T | undefined;
  dispose: () => void;
};

let loading: Promise<void> = Promise.resolve();

/** Serialize dense model preparation while invalidating work that leaves the viewport. */
export function createViewportResource<
  T extends { dispose: () => void },
>(options: {
  load: (signal: AbortSignal) => Promise<T>;
  onLoading: () => void;
  onReady: (resource: T) => void;
  onError: (error: unknown) => void;
}): ViewportResource<T> {
  let visible = false;
  let disposed = false;
  let pending: AbortController | undefined;
  let resource: T | undefined;

  function release(): void {
    pending?.abort();
    pending = undefined;
    const previous = resource;
    resource = undefined;
    previous?.dispose();
  }

  return {
    current: () => resource,
    setVisible(next) {
      if (disposed || next === visible) return;
      visible = next;
      release();
      if (!next) return;
      options.onLoading();
      const request = new AbortController();
      pending = request;
      loading = loading.then(async () => {
        if (request.signal.aborted) return;
        try {
          const result = await options.load(request.signal);
          if (request.signal.aborted) {
            result.dispose();
            return;
          }
          resource = result;
          options.onReady(result);
        } catch (error: unknown) {
          if (!request.signal.aborted) options.onError(error);
        }
      });
    },
    dispose() {
      disposed = true;
      visible = false;
      release();
    },
  };
}

/** Presentation intent only. This module knows no card rules, native IDs or wire format. */
export type TableDropIntent = Readonly<{
  presentationCardId: string;
  /** Opaque identity supplied by the host projection, never inferred from sample artwork. */
  sourceId?: string;
}>;

export type TableInteractionOptions =
  | Readonly<{
      mode: "fixture-preview";
      onPreview: (intent: TableDropIntent) => void;
    }>
  | Readonly<{
      mode: "integration";
      onIntent?: (
        intent: TableDropIntent,
        signal: AbortSignal,
      ) => void | Promise<void>;
    }>;

export type TableDropResult =
  | Readonly<{
      status: "preview" | "requested" | "unavailable" | "busy" | "cancelled";
    }>
  | Readonly<{ status: "failed"; error: unknown }>;

/**
 * Dispatches one gesture at a time without moving cards. Only the explicit fixture
 * branch runs a preview. A completed host callback means delivery, never rules acceptance;
 * connected rendering must follow the next host projection instead.
 */
export function createTableInteraction(options?: TableInteractionOptions) {
  let pending: { controller: AbortController; cancel: () => void } | null =
    null;

  async function drop(input: TableDropIntent): Promise<TableDropResult> {
    if (pending) return { status: "busy" };
    if (!options) return { status: "unavailable" };
    const intent: TableDropIntent = Object.freeze(
      input.sourceId === undefined
        ? { presentationCardId: input.presentationCardId }
        : {
            presentationCardId: input.presentationCardId,
            sourceId: input.sourceId,
          },
    );
    if (options.mode === "fixture-preview") {
      try {
        options.onPreview(intent);
        return { status: "preview" };
      } catch (error: unknown) {
        return { status: "failed", error };
      }
    }
    const onIntent = options.onIntent;
    if (!onIntent) return { status: "unavailable" };

    const controller = new AbortController();
    let cancel = (): void => {};
    const cancelled = new Promise<TableDropResult>((resolve) => {
      cancel = () => resolve({ status: "cancelled" });
    });
    const request = { controller, cancel };
    pending = request;
    try {
      // Schedule after installing pending so a synchronous callback cannot reenter dispatch.
      const delivered = Promise.resolve().then(
        async (): Promise<TableDropResult> => {
          if (controller.signal.aborted) return { status: "cancelled" };
          try {
            await onIntent(intent, controller.signal);
            return { status: "requested" };
          } catch (error: unknown) {
            return { status: "failed", error };
          }
        },
      );
      return await Promise.race([delivered, cancelled]);
    } finally {
      if (pending === request) pending = null;
    }
  }

  return {
    drop,
    /**
     * Cancels local waiting. The host must honor the signal to stop its own work;
     * this cannot undo an already submitted or accepted game action.
     */
    cancel(): void {
      const request = pending;
      if (!request) return;
      pending = null;
      request.cancel();
      request.controller.abort();
    },
    get isPending(): boolean {
      return pending !== null;
    },
  };
}

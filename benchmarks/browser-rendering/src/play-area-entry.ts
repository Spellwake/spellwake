/** Explicit browser presentation fixture. Native/connected hosts use mountPlayArea directly. */
import { mountPlayArea } from "./play-area";
import { SAMPLE_PLAY_SETUP, playSetupSchema } from "./play-setup";
import { z } from "zod";
const lifetime = new AbortController();
window.addEventListener("pagehide", () => lifetime.abort(), {
  once: true,
  signal: lifetime.signal,
});
async function start(): Promise<void> {
  const query = new URL(location.href).searchParams;
  const fixtureCount = (name: string): number | null => {
    const raw = query.get(name);
    return raw === null
      ? null
      : z.coerce.number().int().min(0).max(200).parse(raw);
  };
  const count = fixtureCount("hand");
  const opponentCount = fixtureCount("opponentHand");
  const setup =
    count === null && opponentCount === null
      ? SAMPLE_PLAY_SETUP
      : playSetupSchema.parse({
          ...SAMPLE_PLAY_SETUP,
          players: SAMPLE_PLAY_SETUP.players.map((player, index) =>
            index === SAMPLE_PLAY_SETUP.viewerSeat - 1 && count !== null
              ? { ...player, hand: undefined, handCount: count }
              : index !== SAMPLE_PLAY_SETUP.viewerSeat - 1 &&
                  opponentCount !== null
                ? {
                    ...player,
                    handCount: opponentCount,
                    hand: Array.from(
                      { length: opponentCount },
                      (_, cardIndex) =>
                        player.hand?.[cardIndex] ?? {
                          key: null,
                          faceUp: false,
                        },
                    ),
                  }
                : player,
          ),
        });
  await mountPlayArea(document, setup, {
    playMode: "fixture-preview",
    signal: lifetime.signal,
  });
}
start().catch((error: unknown) => {
  if (lifetime.signal.aborted) return;
  console.error("Play area failed", error);
  const status = document.getElementById("play-status");
  if (status) {
    status.classList.remove("sr-only");
    status.textContent = "Unable to load the table. Reload to retry.";
  }
});

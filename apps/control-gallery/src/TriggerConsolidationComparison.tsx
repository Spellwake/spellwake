import {
  CompactSpellStack,
  CustomCardFrame,
  type StackEntry,
} from "@spellwake/ui";

const makeSpecimenArt = (top: string, bottom: string, glow: string): string =>
  `data:image/svg+xml;charset=utf-8,${encodeURIComponent(
    `<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 894 654"><defs><linearGradient id="g" x1="0" y1="0" x2="1" y2="1"><stop offset="0%" stop-color="${top}"/><stop offset="100%" stop-color="${bottom}"/></linearGradient><radialGradient id="r" cx="50%" cy="42%" r="52%"><stop offset="0%" stop-color="${glow}" stop-opacity="0.72"/><stop offset="100%" stop-color="${bottom}" stop-opacity="0"/></radialGradient></defs><rect width="894" height="654" fill="url(#g)"/><rect width="894" height="654" fill="url(#r)"/><circle cx="447" cy="292" r="168" fill="none" stroke="${glow}" stroke-opacity="0.4" stroke-width="3"/><polygon points="447,128 548,292 447,456 346,292" fill="${glow}" fill-opacity="0.22" stroke="${glow}" stroke-width="4"/></svg>`,
  )}`;

const wardenArt = makeSpecimenArt("#383123", "#12151d", "#f0d79b");
const primordialArt = makeSpecimenArt("#13304c", "#0c1420", "#6cc4ee");

/**
 * Isolated reviewable consolidation candidate comparing the untouched current
 * `TriggeredAbilityFace` (`extended-art-frame.svg`) side-by-side against the
 * unified 1062x1482 procedural `CustomCardFrame` (`kind: "stack_trigger"`).
 *
 * `packages/ui/src/stack/extended-art-frame.svg` and
 * `packages/ui/src/stack/TriggeredAbilityFace.tsx` remain untouched pending
 * user approval of this visual comparison.
 */
export function TriggerConsolidationComparison() {
  const shortEntry: StackEntry[] = [
    {
      kind: "ability",
      id: "warden-before",
      name: "Soul Warden",
      art: <img src={wardenArt} alt="" draggable={false} />,
      effect: "Gain 1 life.",
      typeLabel: "Triggered ability",
      controller: "Sam",
      portrait: <span aria-hidden="true" />,
    },
  ];
  const longEntry: StackEntry[] = [
    {
      kind: "ability",
      id: "primordial-before",
      name: "Diluvian Primordial",
      art: <img src={primordialArt} alt="" draggable={false} />,
      effect:
        "When this creature enters, for each opponent, you may cast up to one target instant or sorcery card from that player's graveyard without paying its mana cost. If a spell cast this way would be put into a graveyard, exile it instead.",
      typeLabel: "Triggered ability",
      controller: "Sam",
      portrait: <span aria-hidden="true" />,
    },
  ];

  return (
    <main
      style={{
        minHeight: "100vh",
        background: "radial-gradient(circle at 50% 0%, #17202e 0%, #090d14 70%)",
        color: "#f4eee4",
        padding: "22px 44px 30px",
        fontFamily: 'Baskerville, "Times New Roman", Georgia, serif',
      }}
    >
      <header style={{ maxWidth: "1180px", margin: "0 auto 18px" }}>
        <div
          style={{
            fontSize: "12px",
            letterSpacing: "0.1em",
            textTransform: "uppercase",
            color: "#d4b57c",
            marginBottom: "6px",
            fontFamily: "system-ui, sans-serif",
          }}
        >
          Reviewable Consolidation Candidate &middot; Pending Approval
        </div>
        <h1 style={{ margin: 0, fontSize: "30px", fontWeight: 500, color: "#f8eed9" }}>
          TriggeredAbilityFace: Current vs. Unified 300-DPI Card Frame Engine
        </h1>
        <p
          style={{
            margin: "8px 0 0",
            fontSize: "15px",
            color: "#b7afa0",
            fontFamily: "system-ui, sans-serif",
            maxWidth: "820px",
            lineHeight: 1.5,
          }}
        >
          Comparing the untouched current stack ability face (<code>extended-art-frame.svg</code>) against the candidate unified procedural 1062x1482 <code>CustomCardFrame</code> (<code>kind=&quot;stack_trigger&quot;</code>). Existing stack components remain unchanged until approved.
        </p>
      </header>

      <div
        style={{
          maxWidth: "1180px",
          margin: "0 auto",
          display: "grid",
          gridTemplateColumns: "1fr 1fr",
          gap: "24px",
        }}
      >
        {/* Short Trigger Row */}
        <section
          style={{
            background: "rgba(15, 20, 29, 0.86)",
            border: "1px solid rgba(159, 146, 121, 0.28)",
            borderRadius: "14px",
            padding: "22px",
          }}
        >
          <div
            style={{
              fontSize: "12px",
              textTransform: "uppercase",
              letterSpacing: "0.08em",
              color: "#9f998e",
              marginBottom: "16px",
              fontFamily: "system-ui, sans-serif",
            }}
          >
            Before &middot; Current TriggeredAbilityFace (Soul Warden)
          </div>
          <div style={{ display: "flex", justifyContent: "center", padding: "12px 0" }}>
            <CompactSpellStack
              items={shortEntry}
              selectedId={null}
              onSelect={() => {}}
            />
          </div>
        </section>

        <section
          style={{
            background: "rgba(15, 20, 29, 0.86)",
            border: "1px solid rgba(212, 181, 124, 0.42)",
            borderRadius: "14px",
            padding: "22px",
          }}
        >
          <div
            style={{
              fontSize: "12px",
              textTransform: "uppercase",
              letterSpacing: "0.08em",
              color: "#d4b57c",
              marginBottom: "16px",
              fontFamily: "system-ui, sans-serif",
            }}
          >
            After Candidate &middot; Unified CustomCardFrame (Soul Warden)
          </div>
          <div style={{ display: "flex", flexDirection: "column", alignItems: "center", gap: "10px", padding: "10px 0" }}>
            <div style={{ width: "184px", borderRadius: "10px", overflow: "hidden", boxShadow: "0 14px 32px rgba(0,0,0,0.55)" }}>
              <CustomCardFrame
                input={{
                  kind: "stack_trigger",
                  name: "Soul Warden",
                  typeLabel: "Triggered ability",
                  effect: "Gain 1 life.",
                  artUrl: wardenArt,
                  illustrated: true,
                  frameAccent: "W",
                }}
              />
            </div>
            <div style={{ fontSize: "11.5px", color: "#9e978a", fontFamily: "system-ui, sans-serif" }}>
              Unified 1062&times;1482 (5:7 card ratio, scaled to match stack width)
            </div>
          </div>
        </section>

        {/* Long Trigger Row */}
        <section
          style={{
            background: "rgba(15, 20, 29, 0.86)",
            border: "1px solid rgba(159, 146, 121, 0.28)",
            borderRadius: "14px",
            padding: "22px",
          }}
        >
          <div
            style={{
              fontSize: "12px",
              textTransform: "uppercase",
              letterSpacing: "0.08em",
              color: "#9f998e",
              marginBottom: "16px",
              fontFamily: "system-ui, sans-serif",
            }}
          >
            Before &middot; Current TriggeredAbilityFace (Diluvian Primordial)
          </div>
          <div style={{ display: "flex", justifyContent: "center", padding: "12px 0" }}>
            <CompactSpellStack
              items={longEntry}
              selectedId={null}
              onSelect={() => {}}
            />
          </div>
        </section>

        <section
          style={{
            background: "rgba(15, 20, 29, 0.86)",
            border: "1px solid rgba(212, 181, 124, 0.42)",
            borderRadius: "14px",
            padding: "22px",
          }}
        >
          <div
            style={{
              fontSize: "12px",
              textTransform: "uppercase",
              letterSpacing: "0.08em",
              color: "#d4b57c",
              marginBottom: "16px",
              fontFamily: "system-ui, sans-serif",
            }}
          >
            After Candidate &middot; Unified CustomCardFrame (Diluvian Primordial)
          </div>
          <div style={{ display: "flex", flexDirection: "column", alignItems: "center", gap: "10px", padding: "10px 0" }}>
            <div style={{ width: "184px", borderRadius: "10px", overflow: "hidden", boxShadow: "0 14px 32px rgba(0,0,0,0.55)" }}>
              <CustomCardFrame
                input={{
                  kind: "stack_trigger",
                  name: "Diluvian Primordial",
                  typeLabel: "Triggered ability",
                  effect:
                    "When this creature enters, for each opponent, you may cast up to one target instant or sorcery card from that player's graveyard without paying its mana cost. If a spell cast this way would be put into a graveyard, exile it instead.",
                  artUrl: primordialArt,
                  illustrated: true,
                  frameAccent: "U",
                }}
              />
            </div>
            <div style={{ fontSize: "11.5px", color: "#9e978a", fontFamily: "system-ui, sans-serif" }}>
              Adaptive text-only mode (&gt;100 chars) &middot; 1062&times;1482 (5:7 ratio)
            </div>
          </div>
        </section>
      </div>
    </main>
  );
}

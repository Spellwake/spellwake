import { ChevronRight } from "lucide-react";
import {
  AttentionBadge,
  PlayerAvatar,
  PlayerIdentity,
  type PlayerAvatarProps,
} from "@spellwake/ui";
import { originalUrl, references, type View } from "./references";
import type { ControlUse } from "./UsageMap";

const rail = "MSG-D01:identity",
  requests = "MSG-D02:identity",
  standings = "ST-01:identity",
  profile = "PP-01:identity",
  otherProfile = "PP-02:identity",
  ownBoard = "NP-01:identity",
  alexBoard = "NP-02:identity";
export const identityUses: ControlUse[] = [
  {
    id: "profile-avatar",
    name: "Player avatar",
    approvedCount: 21,
    description:
      "Portraits in messaging, standings, account settings and public profiles.",
    screens: [rail, requests, standings, profile, otherProfile],
  },
  {
    id: "player-identity",
    name: "Player identity",
    approvedCount: 116,
    description:
      "Portrait and name in social lists, profiles, standings, lobbies, trade and match seats.",
    screens: [
      requests,
      "MSG-D03:identity",
      "MSG-D04:identity",
      standings,
      profile,
      otherProfile,
      ownBoard,
      alexBoard,
    ],
  },
];
/** The gallery samples only portrait artwork. Rim, text, row surface, status and focus are rendered separately. */
function Portrait({
  source,
  art,
  diameter,
  ring,
}: {
  source: View;
  art: readonly [number, number, number];
  diameter: number;
  ring?: PlayerAvatarProps["ring"];
}) {
  const [x, y, side] = art;
  const [width, height] = references[source].size;
  return (
    <PlayerAvatar diameter={diameter} ring={ring ?? "subtle"}>
      <svg aria-hidden viewBox={[x, y, side, side].join(" ")}>
        <image href={originalUrl(source)} width={width} height={height} />
      </svg>
    </PlayerAvatar>
  );
}

function MatchSeats({
  view,
  navigate,
}: {
  view: View;
  navigate: (next: View) => void;
}) {
  const watching = view === alexBoard;
  return (
    <div className="identity-seats">
      {["You", "Alex"].map((name, i) => {
        const own = i === 0,
          selected = own ? !watching : watching;
        return (
          <PlayerIdentity
            key={name}
            presentation="seat"
            name={name}
            life={40}
            actionLabel={
              own
                ? "View your board, 40 life, your turn"
                : "View Alex’s board, 40 life"
            }
            onActivate={() => navigate(own ? ownBoard : alexBoard)}
            pressed={selected}
            takingTurn={own}
            viewingOpponent={!own && selected}
            portrait={
              <Portrait
                source={ownBoard}
                art={own ? [261, 17, 37] : [446, 17, 36]}
                diameter={40}
              />
            }
          />
        );
      })}
    </div>
  );
}
function RequestList({ navigate }: { navigate: (v: View) => void }) {
  return (
    <div className="identity-requests">
      {(
        [
          {
            name: "Arden",
            detail: "Friend request",
            y: 164,
            target: "MSG-D03:identity",
          },
          {
            name: "Nia",
            detail: "Good game. Want a rematch?",
            y: 253,
            target: "MSG-D04:identity",
          },
        ] as const
      ).map((p, i) => (
        <PlayerIdentity
          key={p.name}
          presentation="request"
          name={p.name}
          detail={p.detail}
          actionLabel={`Open ${p.name}’s conversation, ${p.detail}${i ? ", unread message" : ""}`}
          onActivate={() => navigate(p.target)}
          portrait={
            <Portrait source={requests} art={[1207, p.y, 60]} diameter={64} />
          }
          trailing={
            <span className="request-end" aria-hidden>
              <AttentionBadge unread={i === 1} />
              <ChevronRight size={24} strokeWidth={1.4} />
            </span>
          }
        />
      ))}
    </div>
  );
}
export function IdentitySpecimen({
  view,
  navigate,
}: {
  view: View;
  navigate: (v: View) => void;
}) {
  const [, , width, height] = references[view].crop;
  const match = view === ownBoard || view === alexBoard;
  const publicHeader = view === profile || view === otherProfile,
    league = publicHeader || view === standings;
  return (
    <div className="popup-stage">
      <div
        data-specimen
        id="identity-preview"
        tabIndex={-1}
        role="group"
        aria-label={references[view].state}
        className={`popup-scene identity-scene ${league ? "league" : match ? "match" : "messaging"}`}
        style={{ width, height }}
      >
        {match ? (
          <MatchSeats view={view} navigate={navigate} />
        ) : view === requests ? (
          <RequestList navigate={navigate} />
        ) : publicHeader ? (
          <div className="identity-profile">
            <PlayerIdentity
              presentation="profile"
              name="EmberFox"
              portrait={
                <Portrait
                  source={view}
                  art={[355, 200, 204]}
                  diameter={210}
                  ring="gold"
                />
              }
            />
          </div>
        ) : view === standings ? (
          <div className="identity-standings">
            <PlayerIdentity
              presentation="standings"
              name="EmberFox"
              actionLabel="View EmberFox’s public profile"
              onActivate={() => navigate(profile)}
              portrait={
                <Portrait
                  source={standings}
                  art={[412, 381, 68]}
                  diameter={73}
                  ring="gold"
                />
              }
            />
          </div>
        ) : view === rail ? (
          <>
            <div className="identity-rail">
              <Portrait
                source={rail}
                art={[1102, 132, 51]}
                diameter={58}
                ring="selected"
              />
            </div>
            <div className="identity-friend">
              <PlayerIdentity
                presentation="heading"
                name="Mira"
                detail="Friend"
              />
            </div>
          </>
        ) : (
          <div
            className={
              view === "MSG-D03:identity" ? "identity-arden" : "identity-nia"
            }
          >
            <PlayerIdentity
              presentation="heading"
              name={view === "MSG-D03:identity" ? "Arden" : "Nia"}
              detail={
                view === "MSG-D03:identity" ? "Not a friend yet" : "Player"
              }
            />
          </div>
        )}
      </div>
    </div>
  );
}

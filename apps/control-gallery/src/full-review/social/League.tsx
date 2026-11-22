import { Artboard, Box, type Area } from "./Artboard";
import { X } from "lucide-react";
import { LoadingShape } from "@spellwake/ui";
import { useLayoutEffect, useRef, useState, type ReactNode } from "react";
import { RankIdentity, RankProgress } from "@spellwake/ui";
import {
  Avatar,
  Board,
  Btn,
  Coins,
  Notice,
  Shell,
  Skeleton,
  Window,
  type Go,
} from "./shared";
import type { ScreenProps } from "../../review-types";
import {
  formats as standingFormats,
  longPlayer,
  ranksFor,
  recordFor,
  standingsFor,
  readStandingsLocation,
  type StandingsLocation,
} from "./league-standings";
const badges = import.meta.glob<string>(
  "../../../../../assets/rank-icons/*.svg",
  { eager: true, query: "?inline", import: "default" },
);
const tiers = ["Bronze", "Silver", "Gold", "Platinum", "Diamond", "Mythic"];
const upper = [1100, 1300, 1500, 1700, 1900],
  lower = [0, 1100, 1300, 1500, 1700];
type Format = [string, number, number, string, number | null];
const normal: Format[] = [
  ["Modern", 1, 1240, "12 wins · 8 losses", 250],
  ["Standard", 2, 1420, "18 wins · 12 losses", 500],
  ["Commander", 0, 1040, "6 wins · 6 losses", 0],
  ["Legacy", 4, 1820, "20 wins · 5 losses", 1000],
];
const special: Format[] = [
  ["Modern", 5, 2040, "12 wins · 7 losses · 1 draw", 1500],
  ["Standard", 1, 1120, "No matches", null],
  ["Commander", 0, 1000, "No matches", null],
];
const more: Format[] = [
  normal[3]!,
  ["Vintage", 2, 1360, "7 wins · 5 losses", 500],
  ["Pioneer", 1, 1180, "5 wins · 2 losses", 250],
  ["Pauper", 0, 1020, "2 wins · 3 losses", 0],
  ["Historic", 3, 1620, "16 wins · 10 losses", 750],
];
function Rank({
  tier,
  rating,
  mode,
}: {
  tier: number;
  rating: number;
  mode: "season" | "standings" | "profile";
}) {
  const name = tiers[tier]!;
  return (
    <RankIdentity
      tier={name}
      emblemSrc={
        badges[
          "../../../../../assets/rank-icons/" + name.toLowerCase() + ".svg"
        ]!
      }
      presentation={mode}
      {...(mode === "season"
        ? { ratingLabel: "Rating " + rating.toLocaleString("en-US") }
        : {})}
    />
  );
}
function Table({
  heads,
  rows,
  own,
}: {
  heads: string[];
  rows: ReactNode[][];
  own?: number;
}) {
  return (
    <div className="soc-tablewrap">
      <table className="soc-table">
        <thead>
          <tr>
            {heads.map((h) => (
              <th key={h}>{h}</th>
            ))}
          </tr>
        </thead>
        <tbody>
          {rows.map((r, i) => (
            <tr key={i} className={i === own ? "soc-own" : undefined}>
              {r.map((c, j) => (
                <td key={j}>{c}</td>
              ))}
            </tr>
          ))}
        </tbody>
      </table>
    </div>
  );
}
function Season({ id }: { id: string }) {
  const formats =
    id === "LS-02"
      ? special
      : id === "LS-03"
        ? normal.map<Format>((row) => [row[0], 0, 1000, "No matches", null])
        : id === "LS-04"
          ? more
          : normal;
  return (
    <Table
      heads={["Format", "Rank", "Progress", "Record", "Season reward"]}
      rows={formats.map(([name, tier, rating, rec, reward]) => [
        name,
        <Rank mode="season" tier={tier} rating={rating} />,
        tier === 5 ? (
          <RankProgress highest label="Highest rank" />
        ) : (
          <RankProgress
            rating={rating}
            lowerBound={lower[tier]!}
            nextBound={upper[tier]!}
            label={upper[tier]! - rating + " to " + tiers[tier + 1]}
          />
        ),
        rec,
        reward === null ? "—" : <Coins n={reward} />,
      ])}
    />
  );
}
function Standings({
  id,
  go,
  location,
  variant = "",
  behindProfile = false,
}: {
  id: string;
  go: ScreenProps["go"];
  location?: StandingsLocation | undefined;
  variant?: string;
  behindProfile?: boolean;
}) {
  const restored = location?.panel === variant ? location : undefined;
  const [format, set] = useState(
    restored?.format ?? (id === "ST-02" ? "Standard" : "Modern"),
  );
  const root = useRef<HTMLDivElement>(null);
  const tableTop = restored?.tableTop ?? 0,
    tabsLeft = restored?.tabsLeft ?? 0;
  const focusPlayer = behindProfile ? undefined : restored?.player;
  useLayoutEffect(() => {
    const table = root.current?.querySelector(".soc-tablewrap");
    const tabs = root.current?.querySelector(".soc-tabs");
    if (table) table.scrollTop = tableTop;
    if (tabs) tabs.scrollLeft = tabsLeft;
    if (focusPlayer)
      root.current
        ?.querySelector<HTMLButtonElement>(
          `[data-player="${CSS.escape(focusPlayer)}"]`,
        )
        ?.focus({ preventScroll: true });
  }, [tableTop, tabsLeft, focusPlayer]);
  const unplayed = variant === "unplayed" && format === "Modern";
  const empty = variant === "empty" && format === "Modern";
  const selected = standingsFor(format, id === "ST-03");
  const people = empty ? [] : unplayed ? selected.slice(0, 2) : selected;
  function openProfile(name: string) {
    const origin = {
      screen: id,
      player: name,
      format,
      panel: variant,
      tableTop: root.current?.querySelector(".soc-tablewrap")?.scrollTop ?? 0,
      tabsLeft: root.current?.querySelector(".soc-tabs")?.scrollLeft ?? 0,
    };
    // Restore the invoking history entry on browser Back, including tabs chosen in place.
    const history = window.parent.history;
    history.replaceState({ ...history.state, screenData: origin }, "");
    go(
      name === "EmberFox" ? "PP-01" : name === longPlayer ? "PP-03" : "PP-02",
      origin,
    );
  }
  const playerLink = (name: string) => (
    <button
      className="soc-person"
      data-player={name}
      onClick={() => openProfile(name)}
    >
      <Avatar size={58} name={name} />
      <span className="soc-standing-name">{name}</span>
    </button>
  );
  const rows = people.map<ReactNode[]>(({ place, name, rank }) => [
    place,
    playerLink(name),
    <Rank tier={rank.tier} rating={rank.rating} mode="standings" />,
    rank.rating.toLocaleString("en-US"),
    recordFor(rank, true),
  ]);
  if (empty || unplayed)
    rows.push([
      "—",
      playerLink("You"),
      <Rank tier={0} rating={1000} mode="standings" />,
      "1,000",
      "No matches",
    ]);
  return (
    <div className="soc-frame" ref={root}>
      <div className="soc-tabs">
        {(id === "ST-03" ? standingFormats : standingFormats.slice(0, 4)).map(
          (f) => (
            <button
              key={f}
              aria-pressed={f === format}
              onClick={() => {
                const table = root.current?.querySelector(".soc-tablewrap");
                if (table && f !== format) table.scrollTop = 0;
                set(f);
                if (id === "ST-01" && f === "Standard") go("ST-02");
                else if (id === "ST-02" && f === "Modern") go("ST-01");
              }}
            >
              {f}
            </button>
          ),
        )}
      </div>
      {empty && <p className="soc-quiet">No ranked matches yet</p>}
      <Table
        heads={["Place", "Player", "Rank", "Rating", "Record"]}
        rows={rows}
        own={
          empty || unplayed
            ? rows.length - 1
            : people.findIndex((p) => p.name === "You")
        }
      />
    </div>
  );
}
const summaries = ["Format", "Rank", "Rating", "Record"];
function PublicBody({
  id,
  go,
  location,
}: {
  id: string;
  go: Go;
  location?: StandingsLocation | undefined;
}) {
  const empty = id === "PP-06",
    long = id === "PP-03";
  const player = location?.player;
  const name =
    player ?? (empty ? "Moonwatcher" : long ? longPlayer : "EmberFox");
  if (id === "PP-05") return <p className="soc-quiet">Player unavailable</p>;
  const ranks =
    name === "You" &&
    location?.screen === "ST-04" &&
    location.format === "Modern" &&
    (location.panel === "empty" || location.panel === "unplayed")
      ? [{ format: location.format, tier: 0, rating: 1000, wins: 0, losses: 0 }]
      : ranksFor(name);
  // Direct entry shows the approved long-content snapshot. A linked profile uses this season's actual row.
  const shown =
    long && !player
      ? ranks.map((row) =>
          row.format === "Standard"
            ? { ...row, tier: 2, rating: 1420, wins: 18, losses: 12 }
            : row,
        )
      : ranks;
  return (
    <>
      <div className="soc-profile">
        <Avatar name={name === longPlayer ? "emberfox" : name} size={196} />
        <h1>{name}</h1>
        {(id === "PP-01" || long) && (
          <Btn
            onClick={() => {
              sessionStorage.setItem("sw-review-contact", name);
              go("MSG-D01");
            }}
          >
            Message
          </Btn>
        )}
      </div>
      <p>Season Oct–Dec 2026</p>
      <Table
        heads={summaries}
        rows={
          empty
            ? []
            : shown.map((rank) => [
                rank.format,
                <Rank tier={rank.tier} rating={rank.rating} mode="profile" />,
                rank.rating.toLocaleString("en-US"),
                recordFor(rank),
              ])
        }
      />
      {empty && <p className="soc-quiet">No ranked matches yet</p>}
    </>
  );
}
export function League({
  id,
  go,
  data,
}: {
  id: string;
  go: ScreenProps["go"];
  data?: unknown;
}) {
  const location = readStandingsLocation(data);
  const profile = id.startsWith("PP"),
    season = id.startsWith("LS"),
    read = id === "LS-05" || id === "ST-05" || id === "PP-04",
    title = profile ? "profile" : season ? "your ranks" : "standings";
  if (id === "PP-04") {
    const labels = ["Loading", "Initial load failed", "Refresh failed"];
    const regions: Area[] = [
      [194, 8, 547, 162],
      [194, 180, 547, 146],
      [194, 335, 547, 175],
    ];
    return (
      <Artboard width={840} height={526}>
        {regions.map((a, i) => (
          <div key={labels[i]}>
            <Box area={[46, a[1] + a[3] * 0.4, 132, 38]}>
              <div className="soc-profile-read-label">{labels[i]}</div>
            </Box>
            <Box area={a} frame className="soc-read-profile">
              <button
                className="soc-x"
                aria-label="Close profile"
                onClick={() => go("ST-01")}
              >
                <X size={16} />
              </button>
              {i !== 1 && (
                <div className="soc-read-person">
                  {i === 0 ? (
                    <>
                      <LoadingShape shape="portrait" width={55} height={55} />
                      <LoadingShape width={135} height={19} />
                    </>
                  ) : (
                    <>
                      <Avatar name="emberfox" size={55} />
                      <h2>EmberFox</h2>
                    </>
                  )}
                </div>
              )}
              <p>Season Oct–Dec 2026</p>
              <Table
                heads={summaries}
                rows={
                  i === 1
                    ? []
                    : i === 0
                      ? ["Modern", "Standard", "Commander"].map((f) => [
                          f,
                          ...[0, 1, 2].map((k) => (
                            <LoadingShape key={k} width={50} height={6} />
                          )),
                        ])
                      : [
                          [
                            "Modern",
                            <Rank mode="profile" tier={5} rating={2216} />,
                            "2,216",
                            "40 wins · 12 losses",
                          ],
                          [
                            "Standard",
                            <Rank mode="profile" tier={2} rating={1420} />,
                            "1,420",
                            "18 wins · 12 losses",
                          ],
                        ]
                }
              />
              {i > 0 && (
                <Notice>
                  {i === 1
                    ? "Couldn’t load profile."
                    : "Couldn’t refresh profile."}
                </Notice>
              )}
            </Box>
          </div>
        ))}
      </Artboard>
    );
  }
  if (read)
    return (
      <Board
        title=""
        scenic
        cols={1}
        panels={[
          ["Loading", <Skeleton count={3} />],
          ["Initial load failed", <Notice>Couldn’t load {title}.</Notice>],
          [
            "Refresh failed",
            <>
              {profile ? (
                <PublicBody id="PP-02" go={go} />
              ) : season ? (
                <Season id="LS-01" />
              ) : (
                <Standings id="ST-05" go={go} location={location} />
              )}
              <Notice>Couldn’t refresh {title}.</Notice>
            </>,
          ],
        ]}
      />
    );
  if (id === "ST-04")
    return (
      <Board
        title=""
        scenic
        cols={1}
        panels={[
          [
            "No ranked matches in this format",
            <Standings id={id} go={go} location={location} variant="empty" />,
          ],
          [
            "You have not played this format",
            <Standings
              id={id}
              go={go}
              location={location}
              variant="unplayed"
            />,
          ],
        ]}
      />
    );
  const board = (
    <>
      <h1>{season ? "League" : "Standings"}</h1>
      <p>Season Oct–Dec 2026 · Ends Jan 1 UTC</p>
      {season ? (
        <Season id={id} />
      ) : (
        <Standings
          id={profile ? (location?.screen ?? "ST-01") : id}
          go={go}
          location={location}
          behindProfile={profile}
          variant={profile ? (location?.panel ?? "") : ""}
        />
      )}
    </>
  );
  return (
    <Shell area="League" current={season ? "LS-01" : "ST-01"} go={go}>
      {board}
      {profile && (
        <Window
          wide={id !== "PP-05"}
          title="Public player profile"
          close={() => {
            if (location?.player) window.parent.history.back();
            else go("ST-01");
          }}
        >
          <PublicBody id={id} go={go} location={location} />
        </Window>
      )}
    </Shell>
  );
}

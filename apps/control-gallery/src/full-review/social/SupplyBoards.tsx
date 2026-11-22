import { useState, type ReactNode } from "react";
import { PackageOpen, X } from "lucide-react";
import { SectionRail } from "../SectionRail";
import { ReviewHeader } from "../ReviewHeader";
import { art, Btn, Coins, Notice, Search, Skeleton, type Go } from "./shared";

import { Artboard, Box, type Area } from "./Artboard";
function Tagline({ children }: { children: ReactNode }) {
  return <h3 className="soc-board-label">{children}</h3>;
}
function Nav({
  go,
  rail = true,
  width = 840,
}: {
  go: Go;
  rail?: boolean;
  width?: number;
}) {
  return (
    <>
      <Box area={[0, 0, width, 48]} scale={0.58}>
        <ReviewHeader active="Market" go={go} coins={8000} />
      </Box>
      {rail && (
        <Box area={[0, 50, 136, 136]} scale={0.58}>
          <SectionRail area="Market" current="SP-01" go={go} />
        </Box>
      )}
    </>
  );
}
function Banner({ go }: { go: Go }) {
  return (
    <div className="soc-banner">
      <PackageOpen size={38} />
      <div>
        <h2>Choose the next supply drop</h2>Voting closes Dec 25 · Next drop Jan
        1
      </div>
      <Btn onClick={() => go("SP-06")}>Vote for next drop</Btn>
    </div>
  );
}
function Product({
  collector = false,
  pending = false,
  go,
}: {
  collector?: boolean;
  pending?: boolean;
  go: Go;
}) {
  return (
    <div className="soc-board-product">
      <img alt="" src={art.foundations} />
      <div>
        <h3>Foundations</h3>
        <p>{collector ? "Collector" : "Play"} Booster</p>
        <p>
          {pending
            ? "Checking availability…"
            : (collector ? 40 : 420) + " remaining"}
        </p>
        {!pending && <Coins n={collector ? 3000 : 1000} />}
        <Btn disabled={pending} onClick={() => go("SP-03")}>
          Buy
        </Btn>
      </div>
    </div>
  );
}
function ReferenceControls({ go }: { go: Go }) {
  return (
    <div className="soc-control-legend">
      <label>
        <input type="checkbox" defaultChecked /> FDN
        <br />
        Foundations
      </label>
      <label>
        <input type="checkbox" /> BLB
        <br />
        Bloomburrow
      </label>
      <input aria-label="Search control" placeholder="Search sets…" />
      <input aria-label="Disabled search" disabled placeholder="Search sets…" />
      <Btn onClick={() => go("SP-06")}>Save vote</Btn>
    </div>
  );
}
type SupplyBoardProps = {
  id: string;
  go: Go;
  buy: (v: string) => ReactNode;
  vote: (v: string) => ReactNode;
};
export function SupplyBoards({ id, go, buy, vote }: SupplyBoardProps) {
  const [query, set] = useState("found"),
    [empty, setEmpty] = useState("zzzz");
  const heading = (area: Area, title: string, below?: string) => (
    <Box area={area}>
      <h2>{title}</h2>
      {below && <small>{below}</small>}
    </Box>
  );
  if (id === "SP-02")
    return (
      <Artboard width={840} height={1120}>
        <Nav go={go} />
        {heading([165, 76, 655, 66], "Spellwake Supply · UI Stateboard")}
        <Box area={[12, 190, 356, 330]} frame>
          <Tagline>A. Availability Menu</Tagline>
          <p>Availability filter (open)</p>
          <label>
            Availability: All
            <select size={4} aria-label="Availability">
              {["All", "Available", "Sold out", "Unavailable"].map((n) => (
                <option key={n}>{n}</option>
              ))}
            </select>
          </label>
        </Box>
        <Box area={[382, 190, 445, 330]} frame>
          <Tagline>B. Matching &amp; Multiple Products</Tagline>
          <p>Search match with multiple product tiles (same set)</p>
          <Search label="Search sets" value={query} set={set} />
          <div className="soc-supply-pair">
            {query
              .toLowerCase()
              .split(" ")
              .every((q) => "foundations".includes(q)) && (
              <>
                <Product go={go} />
                <Product go={go} collector />
              </>
            )}
          </div>
        </Box>
        <Box area={[12, 536, 362, 545]} frame>
          <Tagline>C. Read States</Tagline>
          <p>Loading, price read failure, and dismissible error toast</p>
          <Skeleton count={2} />
          <Product pending go={go} />
          <Notice>Couldn’t load price.</Notice>
        </Box>
        <Box area={[387, 536, 441, 545]} frame>
          <Tagline>D. Empty &amp; No-Match</Tagline>
          <p>
            Search no-match, verified empty catalogue, and read failure toast
          </p>
          <Search label="Search sets" value={empty} set={setEmpty} />
          <div className="soc-quiet">
            No matching sets
            <br />
            <Btn
              onClick={() => {
                setEmpty("");
                go("SP-01");
              }}
            >
              Clear filters
            </Btn>
          </div>
          <hr />
          <p className="soc-quiet">No sets available</p>
          <Notice>Couldn’t load sets.</Notice>
        </Box>
      </Artboard>
    );
  if (id === "SP-04") {
    const labels = [
        "Normal (Computed Total)",
        "New Quote Loading",
        "Insufficient Quantity",
        "Changed Terms Requiring Fresh Consent",
      ],
      notes = [
        "Quantity 2, 420 available, total 2003, Buy enabled.",
        "Quantity 2, new total loading, Buy disabled.",
        "Quantity 3, only 2 available. Buy disabled.",
        "Quantity 2, 419 available. New price requires consent.",
      ];
    return (
      <Artboard width={747} height={1120}>
        <Nav width={747} go={go} />
        <Box area={[132, 58, 596, 57]} scale={0.56}>
          <Banner go={go} />
        </Box>
        {heading(
          [170, 128, 560, 38],
          "Spellwake Supply · Buy Packs Stateboard",
        )}
        {["normal", "loading", "short", "changed"].map((v, i) => (
          <div key={v}>
            <Box
              area={[
                24 + (i % 2) * 365,
                177 + Math.floor(i / 2) * 440,
                344,
                75,
              ]}
              className="soc-quote-caption"
            >
              <Tagline>
                {String.fromCharCode(65 + i)}. {labels[i]}
              </Tagline>
              <p>{notes[i]}</p>
            </Box>
            <Box
              area={[
                24 + (i % 2) * 365,
                255 + Math.floor(i / 2) * 440,
                337,
                344,
              ]}
              scale={0.65}
              className="soc-buy-excerpt"
            >
              {buy(v)}
            </Box>
          </div>
        ))}
      </Artboard>
    );
  }
  if (id === "SP-05") {
    const refrow = (name: string, node: ReactNode) => (
      <div className="soc-op-row">
        <small>{name}</small>
        <div>{node}</div>
      </div>
    );
    return (
      <Artboard width={840} height={1120}>
        <Box area={[25, 8, 795, 42]}>
          <div className="soc-brand-board">
            SPELLWAKE │ Supply · Use Case UI
            Stateboard
          </div>
        </Box>
        {heading(
          [200, 55, 520, 31],
          "Spellwake Supply · Key Use Cases & States",
        )}
        <Box area={[17, 90, 386, 454]} frame scale={0.72}>
          <Tagline>A. Purchase Operation</Tagline>
          {refrow("A1. Pending", buy("pending"))}
          {refrow("A2. Unknown", buy("unknown"))}
        </Box>
        <Box area={[426, 90, 397, 454]} frame scale={0.72}>
          <Tagline>B. Vote Operation</Tagline>
          {refrow("B1. Pending", vote("pending"))}
          {refrow("B2. Unknown", vote("unknown"))}
        </Box>
        <Box area={[17, 554, 405, 397]} frame scale={0.76}>
          <Tagline>C. Confirmed Success</Tagline>
          {refrow("Header balance", <Coins n={7000} />)}
          {refrow("Product tile", <Product go={go} />)}
          {refrow("Accepted", <Notice>Bought 1 Foundations pack.</Notice>)}
          {refrow(
            "Saved vote",
            <Btn onClick={() => go("SP-06")}>Edit vote</Btn>,
          )}
          {refrow("Feedback", <Notice>Vote saved.</Notice>)}
        </Box>
        <Box area={[434, 554, 389, 397]} frame scale={0.72}>
          <Tagline>D. Known Rejection</Tagline>
          {refrow("D1. Buy packs", buy("rejected"))}
          {refrow("D2. Voting closed", <Notice>Voting is closed.</Notice>)}
          {refrow(
            "D3. Product unavailable",
            <Notice>Product no longer available.</Notice>,
          )}
        </Box>
        <Box area={[22, 968, 798, 136]}>
          <h3>Component excerpts</h3>
          <div className="soc-control-legend">
            <img width={44} src={art.foundations} alt="Foundations pack" />
            <Coins n={1000} />
            <span>− 1 ＋</span>
            <Btn onClick={() => go("SP-03")}>Buy</Btn>
            <Btn disabled>Buy</Btn>
            <Btn>Cancel</Btn>
            <X />
            <input type="checkbox" defaultChecked aria-label="Selected" />
            <input type="checkbox" aria-label="Unselected" />
          </div>
        </Box>
      </Artboard>
    );
  }
  if (id === "SP-07") {
    const descriptions = [
        "Up to 10 selections. All ten selected candidates remain removable. The eleventh is disabled.",
        "The Innistrad search keeps both offscreen choices selected.",
        "The unmatched query keeps the existing selection count.",
        "All candidate tiles are unchecked. Saving clears the previous choices.",
      ],
      bounds: Area[] = [
        [218, 62, 505, 341],
        [218, 412, 505, 196],
        [218, 618, 505, 152],
        [218, 784, 505, 264],
      ];
    return (
      <Artboard width={747} height={1120}>
        <Nav go={go} rail={false} width={747} />
        {heading(
          [24, 63, 190, 69],
          "Spellwake Supply",
          "Supply Vote UI Stateboard",
        )}
        {["limit", "search", "nomatch", "zero"].map((v, i) => {
          const a = bounds[i]!;
          return (
            <div key={v}>
              <Box
                area={[
                  24,
                  i === 0 ? 143 : a[1] + 3,
                  178,
                  i === 0 ? 220 : a[3] - 15,
                ]}
              >
                <Tagline>
                  {String.fromCharCode(65 + i)}
                  {
                    [
                      "Selection Limit",
                      "Search Retains Other Choices",
                      "No Matches",
                      "Clear All Saved Choices",
                    ][i]
                  }
                </Tagline>
                <p>{descriptions[i]}</p>
              </Box>
              <Box area={a} scale={0.75} className="soc-ballot-excerpt">
                {vote(v)}
              </Box>
            </div>
          );
        })}
      </Artboard>
    );
  }
  if (id === "SP-08") {
    return (
      <Artboard width={840} height={1120}>
        <Nav go={go} rail={false} />
        {heading(
          [36, 62, 780, 53],
          "Usecase UI Stateboard · Spellwake Supply",
          "SUPPLY VOTE MODAL · KEY STATES",
        )}
        <Box area={[18, 125, 804, 126]} frame>
          <ReferenceControls go={go} />
        </Box>
        {["loading", "none", "locked", "failed"].map((v, i) => {
          const x = 12 + (i % 2) * 417,
            y = 277 + Math.floor(i / 2) * 410;
          return (
            <div key={v}>
              <Box area={[x, y, 399, 64]} frame>
                <Tagline>
                  {String.fromCharCode(65 + i)}.
                  {
                    [
                      "Initial read",
                      "No current ballot",
                      "Not yet eligible",
                      "Read failure",
                    ][i]
                  }
                </Tagline>
              </Box>
              <Box
                area={[x, y + 72, 399, 308]}
                scale={0.7}
                className="soc-ballot-excerpt"
              >
                {vote(v)}
              </Box>
            </div>
          );
        })}
      </Artboard>
    );
  }
  return (
    <Artboard width={747} height={1120}>
      <Nav go={go} width={747} />
      {heading(
        [157, 68, 560, 65],
        "Spellwake Supply · Usecase UI Stateboard",
        "USECASE: SUPPLY VOTING",
      )}
      <Box area={[17, 232, 103, 745]}>
        <p>Component Excerpts</p>
        <hr />
        <p>Modal Frame</p>
        <div className="soc-outline" />
        <p>Checkbox Tile</p>
        <label>
          <input type="checkbox" defaultChecked /> FDN
        </label>
        <p>Table Row</p>
        <hr />
        <p>Search Control</p>
        <p>Close Control</p>
        <X size={21} />
      </Box>
      <Box area={[150, 157, 576, 39]}>
        <Tagline>A Closed · not published</Tagline>
      </Box>
      <Box
        area={[150, 202, 575, 316]}
        scale={0.82}
        className="soc-ballot-excerpt"
      >
        {vote("closed")}
      </Box>
      <Box area={[150, 570, 576, 36]}>
        <Tagline>B Published results</Tagline>
      </Box>
      <Box
        area={[150, 613, 575, 384]}
        scale={0.82}
        className="soc-ballot-excerpt"
      >
        {vote("published")}
      </Box>
    </Artboard>
  );
}

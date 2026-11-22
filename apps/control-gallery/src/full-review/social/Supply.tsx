import { SupplyBoards } from "./SupplyBoards";
import { useState } from "react";
import { PackageOpen } from "lucide-react";
import {
  art,
  Board,
  Btn,
  Coins,
  Notice,
  Search,
  Shell,
  Skeleton,
  Window,
  type Go,
} from "./shared";
import { Stepper } from "./Listings";
const products = [
  ["foundations", "Foundations", 420, 1000],
  ["horizons", "Modern Horizons 3", 280, 1400],
  ["bloomburrow", "Bloomburrow", 160, 1200],
  ["duskmourn", "Duskmourn", 0, 0],
  ["P15A", "15th Anniversary Cards", -1, 0],
  ["PHtR", "2016 Heroes of Realm", -1, 0],
] as const;
const sets = [
  "Foundations",
  "Modern Horizons 3",
  "Bloomburrow",
  "Duskmourn",
  "Dominaria Remastered",
  "Innistrad",
  "Khans of Tarkir",
  "Kaldheim",
  "Throne of Eldraine",
  "Theros Beyond Death",
  "The Brothers’ War",
];
const codes = [
  "FDN",
  "MH3",
  "BLB",
  "DSK",
  "DMR",
  "ISD",
  "KTK",
  "KHM",
  "ELD",
  "THB",
  "BRO",
];
function remaining(key: string, initial: number) {
  return initial < 0
    ? initial
    : Math.max(
        0,
        initial -
          Number(sessionStorage.getItem("sw-supply-bought-" + key) || 0),
      );
}
function balance() {
  return Number(sessionStorage.getItem("sw-supply-balance") ?? 8000);
}
function Buy({
  go,
  variant = "",
  mini = false,
}: {
  go: Go;
  variant?: string;
  mini?: boolean;
}) {
  const p =
    products.find(
      (x) => x[0] === sessionStorage.getItem("sw-supply-product"),
    ) ?? products[0];
  const [qty, set] = useState(
    (
      {
        invalid: "1.5",
        maximum: "1000",
        short: "3",
        changed: "2",
        normal: "2",
        loading: "2",
        failed: "2",
      } as Record<string, string>
    )[variant] ?? "1",
  );
  const [checked, check] = useState(false);
  const n = Number(qty),
    stock =
      variant === "sold"
        ? 0
        : variant === "short"
          ? 2
          : variant === "maximum"
            ? 1200
            : variant === "changed"
              ? p[2] - 1
              : p[2];
  const invalid = !Number.isInteger(n) || n < 1 || n > 1000,
    short = n > stock && stock > 0,
    unknown = variant === "unknown",
    pending = variant === "pending";
  const total =
    invalid || short || !stock
      ? null
      : n * p[3] + (variant === "changed" ? 8 : n > 1 ? 3 : 0);
  function cancel() {
    if (!pending && !unknown) go("SP-01");
  }
  return (
    <Window title="Buy packs" close={cancel} embedded={mini}>
      <div className="soc-product-summary">
        <img src={art[p[0]]} alt="" />
        <div>
          <h3>{p[1]}</h3>Play Booster<p>{stock} available</p>
        </div>
      </div>
      <div className="soc-line">
        Quantity{" "}
        <Stepper
          qty={qty}
          set={set}
          max={Math.min(1000, stock)}
          disabled={pending || unknown || !stock}
        />
      </div>
      {invalid && (
        <small className="soc-error">
          Enter a whole number from 1 to 1000.
        </small>
      )}
      {short && <small className="soc-error">Only {stock} available.</small>}
      <div className="soc-line">
        Total {variant === "loading" ? "Updating total…" : <Coins n={total} />}
      </div>
      {variant === "changed" && <small>Price updated.</small>}
      {variant === "failed" && <Notice>Couldn’t load price.</Notice>}
      {variant === "rejected" && (
        <p className="soc-error">Not enough available coins.</p>
      )}
      {unknown && (
        <p>{checked ? "Checking status…" : "Purchase not confirmed."}</p>
      )}
      <div className="soc-actions">
        <Btn onClick={cancel} disabled={pending || unknown}>
          Cancel
        </Btn>
        <Btn
          disabled={
            invalid ||
            short ||
            !stock ||
            pending ||
            (!unknown && total !== null && total > balance()) ||
            ["loading", "failed", "rejected"].includes(variant)
          }
          onClick={() => {
            if (unknown) {
              check(true);
              return;
            }
            if (total === null || n < 1 || n > stock || total > balance())
              return;
            sessionStorage.setItem(
              "sw-supply-bought-" + p[0],
              String(
                Number(
                  sessionStorage.getItem("sw-supply-bought-" + p[0]) || 0,
                ) + n,
              ),
            );
            sessionStorage.setItem(
              "sw-supply-balance",
              String(balance() - total),
            );
            sessionStorage.setItem(
              "sw-supply-receipt",
              "Bought " + qty + " " + p[1] + " pack" + (n === 1 ? "." : "s."),
            );
            go("SP-01");
          }}
        >
          {unknown ? "Check status" : stock === 0 ? "Sold out" : "Buy"}
        </Btn>
      </div>
    </Window>
  );
}
function Ballot({
  go,
  variant = "",
  mini = false,
}: {
  go: Go;
  variant?: string;
  mini?: boolean;
}) {
  const [q, search] = useState(
    variant === "search" ? "inn" : variant === "nomatch" ? "zzzz" : "",
  );
  const [selected, set] = useState(
    new Set(
      variant === "limit"
        ? sets.slice(0, 10)
        : variant === "zero" || variant === "locked"
          ? []
          : (JSON.parse(
              sessionStorage.getItem("sw-supply-ballot") ||
                '["Foundations","Modern Horizons 3"]',
            ) as string[]),
    ),
  );
  const closed = variant === "closed",
    locked = ["locked", "failed", "pending", "unknown", "closed"].includes(
      variant,
    );
  const [checking, check] = useState(false);
  return (
    <Window
      title={variant === "published" ? "Supply results" : "Supply vote"}
      embedded={mini}
      close={() => {
        if (variant !== "pending" && variant !== "unknown") go("SP-01");
      }}
    >
      {variant === "none" || variant === "account" ? (
        <p className="soc-quiet">
          {variant === "none" ? "No vote available" : "Account unavailable"}
        </p>
      ) : variant === "published" ? (
        <>
          <p>5 winning sets</p>
          <table className="soc-table">
            <thead>
              <tr>
                <th>Set</th>
                <th>Votes</th>
                <th>Packs</th>
              </tr>
            </thead>
            <tbody>
              {sets.slice(0, 5).map((n, i) => (
                <tr key={n}>
                  <td>{n}</td>
                  <td>{[320, 290, 250, 210, 190][i]}</td>
                  <td>1200</td>
                </tr>
              ))}
            </tbody>
          </table>
          <p>Next drop Jan 1</p>
        </>
      ) : (
        <>
          <div className="soc-line">
            {closed ? "Voting closed" : "Choose up to 10 sets"}{" "}
            <small>Closes Dec 25</small>
          </div>
          <Search value={q} set={search} label="Search sets" />
          {variant === "loading" ? (
            <div className="soc-ballot-skeleton">
              {sets.slice(0, 10).map((n) => (
                <span key={n} />
              ))}
            </div>
          ) : (
            <div className="soc-ballot-grid">
              {(variant === "closed"
                ? sets.slice(0, 5)
                : ["pending", "unknown"].includes(variant)
                  ? sets.slice(0, 3)
                  : variant === "limit"
                    ? sets
                    : sets.slice(0, 10)
              )
                .filter((n) => n.toLowerCase().includes(q.toLowerCase()))
                .map((n) => (
                  <label key={n} data-checked={selected.has(n)}>
                    <input
                      type="checkbox"
                      checked={selected.has(n)}
                      disabled={
                        locked || (selected.size === 10 && !selected.has(n))
                      }
                      onChange={() => {
                        const next = new Set(selected);
                        if (next.has(n)) next.delete(n);
                        else next.add(n);
                        set(next);
                      }}
                    />
                    <small>{codes[sets.indexOf(n)]}</small>
                    {n}
                  </label>
                ))}
              {!sets.some((n) => n.toLowerCase().includes(q.toLowerCase())) && (
                <p>No matching sets</p>
              )}
            </div>
          )}
          {variant === "failed" && <Notice>Couldn’t load voting.</Notice>}
          <div className="soc-line">
            {variant === "locked"
              ? "Complete a qualifying game to vote."
              : selected.size + " selected"}
            {closed ? (
              <span>Results pending</span>
            ) : variant === "locked" ? (
              <Btn onClick={() => go("DL-05")}>Open Play</Btn>
            ) : (
              <Btn
                disabled={["loading", "failed", "pending"].includes(variant)}
                onClick={() => {
                  if (variant === "unknown") {
                    check(true);
                    return;
                  }
                  sessionStorage.setItem(
                    "sw-supply-ballot",
                    JSON.stringify([...selected]),
                  );
                  sessionStorage.setItem("sw-supply-receipt", "Vote saved.");
                  go("SP-01");
                }}
              >
                {variant === "unknown"
                  ? checking
                    ? "Checking…"
                    : "Check status"
                  : "Save vote"}
              </Btn>
            )}
          </div>
        </>
      )}
    </Window>
  );
}
export function Supply({ id, go }: { id: string; go: Go }) {
  const [q, set] = useState(""),
    [available, filter] = useState("All");
  const names = ["All", "Available", "Sold out", "Unavailable"];
  const [receipt, clear] = useState(
    () => sessionStorage.getItem("sw-supply-receipt") || "",
  );
  const buy = (v: string) => <Buy variant={v} go={go} mini />,
    vote = (v: string) => <Ballot variant={v} go={go} mini />;
  if (["SP-02", "SP-04", "SP-05", "SP-07", "SP-08", "SP-09"].includes(id))
    return <SupplyBoards id={id} go={go} buy={buy} vote={vote} />;
  const boards: Record<string, [string, React.ReactNode][]> = {
    "SP-10": [
      ["Invalid quantity", buy("invalid")],
      ["Maximum quantity", buy("maximum")],
      ["Stock exhausted", buy("sold")],
      ["Quote read failed", buy("failed")],
      ["Account unavailable", vote("account")],
      ["Append loading", <Skeleton />],
    ],
  };
  if (boards[id])
    return (
      <Board
        title={
          id === "SP-09"
            ? "Supply vote"
            : id === "SP-10"
              ? "Supply · boundary states"
              : "Spellwake Supply"
        }
        panels={boards[id]!}
        cols={id === "SP-09" || id === "SP-07" ? 1 : id === "SP-10" ? 3 : 2}
      />
    );
  const matching = products.filter(
    (p) =>
      p[1].toLowerCase().includes(q.toLowerCase()) &&
      (available === "All" ||
        available ===
          (p[2] < 0
            ? "Unavailable"
            : remaining(p[0], p[2]) === 0
              ? "Sold out"
              : "Available")),
  );
  return (
    <Shell area="Market" current="SP-01" go={go} balance={balance()}>
      <div className="soc-banner">
        <PackageOpen size={48} />
        <div>
          <h2>Choose the next supply drop</h2>Voting closes Dec 25 · Next drop
          Jan 1
        </div>
        <Btn onClick={() => go("SP-06")}>
          {sessionStorage.getItem("sw-supply-ballot")
            ? "Edit vote"
            : "Vote for next drop"}
        </Btn>
      </div>
      <div className="soc-tools">
        <Search label="Search all sets" value={q} set={set} />
        <select
          aria-label="Availability"
          value={available}
          onChange={(e) => filter(e.target.value)}
        >
          {names.map((n) => (
            <option key={n} value={n}>
              Availability: {n}
            </option>
          ))}
        </select>
      </div>
      <div className="soc-products">
        {matching.map(([key, name, stock, price]) => (
          <article key={key}>
            {stock >= 0 ? (
              <img src={art[key]} alt="" />
            ) : (
              <strong>{key}</strong>
            )}
            <div>
              <h2>{name}</h2>
              {stock >= 0 && <p>Play Booster</p>}
              {stock > 0 && (
                <>
                  <p>{remaining(key, stock)} remaining</p>
                  <Coins n={price} />
                </>
              )}
              <Btn
                disabled={remaining(key, stock) <= 0}
                onClick={() => {
                  sessionStorage.setItem("sw-supply-product", key);
                  go("SP-03");
                }}
              >
                {stock < 0
                  ? "Unavailable"
                  : remaining(key, stock) === 0
                    ? "Sold out"
                    : "Buy"}
              </Btn>
            </div>
          </article>
        ))}
      </div>
      {!matching.length && <p className="soc-quiet">No matching sets</p>}
      {id === "SP-03" && <Buy go={go} />}
      {id === "SP-06" && <Ballot go={go} />}
      {id === "SP-01" && receipt && (
        <div className="soc-toast">
          <Notice>
            {receipt}
            <button
              aria-label="Dismiss"
              onClick={() => {
                clear("");
                sessionStorage.removeItem("sw-supply-receipt");
              }}
            >
              ×
            </button>
          </Notice>
        </div>
      )}
    </Shell>
  );
}

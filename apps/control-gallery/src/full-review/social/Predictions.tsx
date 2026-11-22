import { Artboard, Box } from "./Artboard";
import { useState, type ReactNode } from "react";
import { CalendarDays } from "lucide-react";
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
const base = [
  {
    key: "bolt",
    name: "Lightning Bolt",
    set: "M11",
    price: "3.50",
    date: "Jan 31, 2027",
  },
  {
    key: "counter",
    name: "Counterspell",
    set: "MH2",
    price: "4",
    date: "Jan 31, 2027",
  },
  {
    key: "ring",
    name: "Sol Ring",
    set: "CMM",
    price: "2",
    date: "Feb 28, 2027",
  },
];
type Question = (typeof base)[number];
function current() {
  return (
    base.find(
      (x) => x.key === sessionStorage.getItem("sw-prediction-question"),
    ) ?? base[0]!
  );
}
const title = (q: Question) =>
  "Will " + q.name + " reach $" + q.price + " by " + q.date.split(",")[0] + "?";
function Subject({ q, question = false }: { q: Question; question?: boolean }) {
  return (
    <div className="soc-subject">
      <img src={art[q.key + "-thumb"]} alt="" />
      <div>
        <h3>{question ? title(q) : q.name}</h3>
        <small>
          {q.set} · Nonfoil{question ? " · " + q.date : ""}
        </small>
      </div>
    </div>
  );
}
function Tile({
  q,
  go,
  variant = "",
}: {
  q: Question;
  go: Go;
  variant?: string;
}) {
  const accepted = sessionStorage.getItem("sw-bet-" + q.key);
  const closed = variant === "resolved" || variant === "closed";
  const start = () => {
    sessionStorage.setItem("sw-prediction-question", q.key);
    go(
      closed
        ? variant === "closed"
          ? "RS-03"
          : "RS-01"
        : accepted
          ? "RS-08"
          : "BF-01",
    );
  };
  return (
    <button className="soc-pred-tile" onClick={start}>
      <Subject q={q} />
      <h2>{title(q)}</h2>
      <small>
        <CalendarDays size={17} /> {q.date}
      </small>
      {variant === "resolved" && <span>Result: Yes</span>}
      <footer>
        {accepted || closed ? "View prediction" : "Make a prediction"} →
      </footer>
    </button>
  );
}
function PersonalRow({
  q,
  go,
  state = "Active",
  created = false,
}: {
  q: Question;
  go: Go;
  state?: string;
  created?: boolean;
}) {
  const amount = q.key === "counter" ? 120 : 200;
  return (
    <article className={"soc-personal " + (created ? " compact" : "")}>
      <Subject q={q} />
      <span className="soc-pill">
        {created ? "Betting open" : q.key === "counter" ? "No" : "Yes"}
      </span>
      {!created && <small>{state}</small>}
      <div className="soc-question">
        {title(q)}
        <small>▣ {q.date}</small>
      </div>
      <footer>
        {!created && (
          <>
            Your bet
            <Coins n={amount} />
            {state !== "Awaiting result" && (
              <>
                → {state === "Active" ? "If you win" : "Returned"}
                <Coins
                  n={state === "Lost" ? 0 : state === "Void" ? amount : 500}
                />
              </>
            )}
          </>
        )}
        <button
          className="soc-link"
          onClick={() => {
            sessionStorage.setItem("sw-prediction-question", q.key);
            if (created && !sessionStorage.getItem("sw-bet-" + q.key))
              go("BF-01");
            else
              go(
                state === "Active"
                  ? "RS-08"
                  : state === "Void"
                    ? "RS-04"
                    : state === "Awaiting result"
                      ? "RS-03"
                      : "RS-01",
              );
          }}
        >
          View →
        </button>
      </footer>
    </article>
  );
}
function PredictionHome({
  go,
  id,
  mini = false,
  tab = "",
}: {
  go: Go;
  id: string;
  mini?: boolean;
  tab?: string;
}) {
  const personal = tab === "personal" || id.startsWith("PM"),
    initial =
      id === "EX-02"
        ? "bolt"
        : sessionStorage.getItem(
            "sw-prediction-query-" + (personal ? "PM-01" : "EX-01"),
          ) || "";
  const [q, search] = useState(initial);
  const [notice, notify] = useState(
    () => sessionStorage.getItem("sw-prediction-notice") || "",
  );
  const matching = base.filter((r) =>
    (title(r) + " " + r.name).toLowerCase().includes(q.toLowerCase()),
  );
  const empty = id === "PM-03",
    mine = empty ? [] : matching.slice(0, 2),
    created = empty ? [] : matching.filter((r) => r.key !== "counter");
  const start = (next: string) => {
    sessionStorage.setItem(
      "sw-prediction-origin",
      personal ? "PM-01" : "EX-01",
    );
    go(next);
  };
  return (
    <>
      <div className="soc-pred-title">
        <h1>Predictions</h1>
        <Search
          label="Search predictions"
          value={q}
          set={(value) => {
            search(value);
            sessionStorage.setItem(
              "sw-prediction-query-" + (personal ? "PM-01" : "EX-01"),
              value,
            );
          }}
        />
        <Btn onClick={() => start("PC-01")}>＋ Create prediction</Btn>
      </div>
      <div className="soc-tabbar">
        <button aria-pressed={!personal} onClick={() => go("EX-01")}>
          Explore
        </button>
        <button aria-pressed={personal} onClick={() => go("PM-01")}>
          My predictions
        </button>
      </div>
      {personal ? (
        <>
          <h2>Your bets</h2>
          <div className="soc-personal-grid">
            {mine.map((r, i) => (
              <PersonalRow
                key={r.key}
                q={r}
                go={start}
                state={i ? "Won" : "Active"}
              />
            ))}
          </div>
          {!mine.length && (
            <div className="soc-empty">
              <h3>{q ? "No matching bets" : "No bets yet"}</h3>
              {!q && (
                <button onClick={() => go("EX-01")}>
                  Explore predictions →
                </button>
              )}
            </div>
          )}
          <h2>Created by you</h2>
          {created.map((r) => (
            <PersonalRow key={r.key} q={r} go={start} created />
          ))}
          {!created.length && (
            <div className="soc-empty">
              <h3>
                {q ? "No matching predictions" : "No predictions created"}
              </h3>
              {!q && (
                <button onClick={() => start("PC-01")}>
                  Create prediction →
                </button>
              )}
            </div>
          )}
        </>
      ) : (
        <div className="soc-pred-grid">
          {matching.map((r) => (
            <Tile key={r.key} q={r} go={start} />
          ))}
          {!matching.length && <p>No matching predictions</p>}
        </div>
      )}
      {!mini && notice && (
        <div className="soc-toast">
          <Notice>
            {notice}{" "}
            <button
              aria-label="Dismiss notice"
              onClick={() => {
                notify("");
                sessionStorage.removeItem("sw-prediction-notice");
              }}
            >
              ×
            </button>
          </Notice>
        </div>
      )}
    </>
  );
}
function Create({
  id,
  go,
  variant = "",
  mini = false,
}: {
  id: string;
  go: Go;
  variant?: string;
  mini?: boolean;
}) {
  const [q, select] = useState(current),
    [query, search] = useState("Lightning Bolt"),
    [card, pick] = useState(id === "PC-02");
  const [price, setPrice] = useState(
      variant === "missing" ? "" : variant === "invalid" ? "0" : "3.50",
    ),
    [date, setDate] = useState(
      variant === "missing"
        ? ""
        : variant === "early"
          ? "2026-11-13"
          : "2027-01-31",
    ),
    [finish, setFinish] = useState("Nonfoil"),
    [chosen, choose] = useState(variant !== "missing"),
    [kind, setKind] = useState(0);
  const pending = variant === "publishing",
    bad =
      Number(price) <= 0 ||
      date < "2026-11-14" ||
      !chosen ||
      ["source", "checking"].includes(variant);
  function close() {
    go(sessionStorage.getItem("sw-prediction-origin") || "EX-01");
  }
  return (
    <Window
      title={card ? "Choose card" : "Create prediction"}
      close={close}
      embedded={mini}
    >
      {card ? (
        <>
          <Search label="Search cards" value={query} set={search} />
          {base
            .filter((r) => r.name.toLowerCase().includes(query.toLowerCase()))
            .flatMap((r) =>
              ["Nonfoil", "Foil"].map((f) => (
                <button
                  key={r.key + f}
                  className="soc-choice"
                  aria-pressed={r.key === q.key && f === finish}
                  onClick={() => {
                    select(r);
                    setFinish(f);
                  }}
                >
                  <Subject q={r} />
                  <small>{f}</small>
                </button>
              )),
            )}
          <div className="soc-actions">
            <Btn onClick={() => pick(false)}>Back</Btn>
            <Btn
              onClick={() => {
                choose(true);
                pick(false);
              }}
            >
              Select
            </Btn>
          </div>
        </>
      ) : (
        <>
          <div className="soc-line">
            {chosen ? <Subject q={q} /> : <p>No card selected</p>}
            <Btn onClick={() => pick(true)}>
              {chosen ? "Change" : "Choose card"}
            </Btn>
          </div>
          {variant === "source" && (
            <small className="soc-error">
              Price unavailable for this printing.
            </small>
          )}
          <div className="soc-form-grid">
            <label>
              Target price
              <input
                aria-label="Target price"
                inputMode="decimal"
                value={price}
                onChange={(e) => setPrice(e.target.value)}
              />
            </label>
            <label>
              End date
              <input
                type="date"
                value={date}
                min="2026-11-14"
                onChange={(e) => setDate(e.target.value)}
              />
            </label>
          </div>
          {price && Number(price) <= 0 && (
            <small className="soc-error">Enter a price above $0.</small>
          )}
          {date && date < "2026-11-14" && (
            <small className="soc-error">Choose Nov 14 or later.</small>
          )}
          <label>
            Prediction type
            <select
              value={kind}
              size={variant === "types" ? 2 : 1}
              onChange={(e) => setKind(Number(e.target.value))}
            >
              <option value="0">Reaches this price by the end date</option>
              <option value="1">At least this price on the end date</option>
            </select>
          </label>
          <small>Betting closes Nov 13, 2026 · UTC</small>
          {variant === "checking" && <Notice>Checking price…</Notice>}
          <Btn
            disabled={bad || pending}
            onClick={() => {
              sessionStorage.setItem("sw-publication", q.name);
              sessionStorage.setItem(
                "sw-prediction-notice",
                "Prediction created.",
              );
              close();
            }}
          >
            Create prediction
          </Btn>
          {variant === "failed" && <Notice>Couldn’t create prediction.</Notice>}
        </>
      )}
    </Window>
  );
}
function Ticket({
  id,
  go,
  variant = "",
  mini = false,
}: {
  id: string;
  go: Go;
  variant?: string;
  mini?: boolean;
}) {
  const q = current();
  const [side, choose] = useState(
    id === "BF-02" ? "Yes" : id === "BF-09" ? "No" : "",
  );
  const [amount, set] = useState(
    variant === "empty"
      ? ""
      : variant === "fraction"
        ? "200.5"
        : variant === "balance"
          ? "9000"
          : side === "No" || variant === "quote-no"
            ? "300"
            : "200",
  );
  const n = Number(amount);
  const unavailable =
      id === "BF-03" ||
      ["expired", "checking", "failed", "no-side"].includes(variant),
    invalid =
      n < 1 || !Number.isInteger(n) || n > 8000 || variant === "liquidity";
  const returnTo = () =>
    go(sessionStorage.getItem("sw-prediction-origin") || "EX-01");
  function place(s: string) {
    if (side !== s) {
      choose(s);
      return;
    }
    if (sessionStorage.getItem("sw-bet-" + q.key)) {
      go("RS-08");
      return;
    }
    sessionStorage.setItem("sw-bet-" + q.key, JSON.stringify({ side: s, n }));
    sessionStorage.setItem("sw-prediction-notice", "Bet placed.");
    returnTo();
  }
  const blocked: Record<string, string> = {
    closed: "Betting closed.",
    gone: "Prediction unavailable.",
    account: "Betting unavailable for your account.",
    inactive: "This prediction is not accepting bets.",
  };
  const warning =
    n < 1
      ? "Enter an amount."
      : !Number.isInteger(n)
        ? "Enter a whole number."
        : n > 8000
          ? "Not enough coins."
          : variant === "liquidity"
            ? "That amount is unavailable. Try a smaller bet."
            : undefined;
  return (
    <Window title="Make a prediction" close={returnTo} embedded={mini}>
      <Subject q={q} question />
      {blocked[variant] ? (
        <p>{blocked[variant]}</p>
      ) : (
        <>
          <div className="soc-payout">
            <label>
              Your bet
              <input
                value={amount}
                inputMode="numeric"
                aria-invalid={invalid}
                onChange={(e) => {
                  set(e.target.value);
                  choose("");
                }}
              />
            </label>
            <span>→</span>
            <div>
              If you win
              <h3>
                {unavailable || invalid ? (
                  ["expired", "checking"].includes(variant) ? (
                    "Updating…"
                  ) : (
                    "Unavailable"
                  )
                ) : (
                  <Coins n={variant === "changed" ? 400 : 500} />
                )}
              </h3>
            </div>
          </div>
          {warning && <small className="soc-error">{warning}</small>}
          {id === "BF-03" && <p>No bets available right now.</p>}
          {variant === "no-side" && <p>Yes bets unavailable.</p>}
          {variant === "failed" && <Notice>Could not update winnings</Notice>}
          {variant === "changed" && <p>Winnings changed. Choose again.</p>}
          {variant === "expired" && <p>Winnings expired.</p>}
          <div className="soc-sides">
            {["Yes", "No"].map((s) => (
              <Btn
                key={s}
                disabled={
                  invalid ||
                  (unavailable && !(variant === "no-side" && s === "No"))
                }
                onClick={() => place(s)}
              >
                {side === s ? "Confirm" : s}
              </Btn>
            ))}
          </div>
        </>
      )}
    </Window>
  );
}
function Result({
  id,
  go,
  variant = "",
  mini = false,
}: {
  id: string;
  go: Go;
  variant?: string;
  mini?: boolean;
}) {
  const q = current(),
    pending = id === "RS-03" || variant === "pending",
    voided = id === "RS-04" || variant === "void",
    open = id === "RS-08",
    publicOnly = id === "RS-05",
    lost = variant === "loss" || variant === "no-loss",
    no = variant.startsWith("no"),
    unknown = ["account", "unverified", "gone", "owner", "read"].includes(
      variant,
    );
  let bet = { side: "Yes", n: 200 };
  try {
    const stored = sessionStorage.getItem("sw-bet-" + q.key);
    if (stored) bet = JSON.parse(stored);
  } catch {
    /* A stale review fixture can be replaced by the example. */
  }
  return (
    <Window
      title={open ? "Prediction" : "Prediction result"}
      embedded={mini}
      close={() =>
        go(sessionStorage.getItem("sw-prediction-origin") || "EX-01")
      }
    >
      {variant !== "owner" && <Subject q={q} question />}
      {variant === "loading" ? (
        <Skeleton count={2} />
      ) : unknown ? (
        <>
          <p className="soc-quiet">
            {variant === "owner" || variant === "read"
              ? "Result unavailable."
              : variant === "gone"
                ? "Prediction unavailable."
                : "Your result unavailable."}
          </p>
          {variant === "account" && <p>Your account is unavailable.</p>}
          {variant === "read" && <Notice>Could not load result</Notice>}
        </>
      ) : (
        <>
          <div className="soc-line">
            {open ? "" : "Result"}
            <span className="soc-pill">
              {open
                ? "Betting open"
                : pending
                  ? "Pending"
                  : voided
                    ? "Void"
                    : no
                      ? "No"
                      : "Yes"}
            </span>
          </div>
          {!publicOnly && (
            <div className="soc-payout">
              <div>
                Your bet {open ? "· " + bet.side : ""}
                <h3>
                  <Coins n={bet.n} />
                </h3>
              </div>
              {!pending && (
                <>
                  <span>→</span>
                  <div>
                    {open ? "If you win" : "Returned"}
                    <h3>
                      <Coins n={voided ? bet.n : lost ? 0 : 500} />
                    </h3>
                  </div>
                </>
              )}
            </div>
          )}
          <div className="soc-result">
            <h2>
              {open
                ? "Bet placed"
                : pending
                  ? "Awaiting result"
                  : voided
                    ? publicOnly
                      ? ""
                      : "Bet returned"
                    : publicOnly
                      ? ""
                      : lost
                        ? "You lost"
                        : "You won"}
            </h2>
            <p>
              {open
                ? "You can bet once per prediction."
                : pending
                  ? "Betting is closed."
                  : voided
                    ? "Price data was incomplete."
                    : variant === "price"
                      ? "Price unavailable."
                      : no
                        ? "Jan 31 price: $3.25"
                        : "Jan 31 price: $3.75"}
            </p>
          </div>
          {variant === "price" && <Notice>Could not load price</Notice>}
          {variant === "refresh" && <Notice>Could not update result</Notice>}
        </>
      )}
    </Window>
  );
}
export function Predictions({ id, go }: { id: string; go: Go }) {
  const form = (v: string) => <Create id="PC-01" go={go} mini variant={v} />,
    ticket = (v: string) => <Ticket id="BF-01" go={go} mini variant={v} />,
    result = (v: string, group = "RS-01") => (
      <Result id={group} go={go} mini variant={v} />
    );
  const row = (state: string) => (
      <PersonalRow q={base[0]!} go={go} state={state} />
    ),
    created = <PersonalRow created q={base[2]!} go={go} />;
  const boards: Record<string, [string, ReactNode][]> = {
    "EX-03": [
      ["No matching predictions", <p>No matching predictions</p>],
      ["Empty catalogue", <p>No predictions yet</p>],
      ["Loading", <Skeleton />],
      ["Read failed", <Notice>Could not load predictions</Notice>],
      ["Loading more", <Tile q={base[0]!} go={go} />],
      ["Closed and resolved", <Tile q={base[1]!} variant="resolved" go={go} />],
    ],
    "PM-02": [
      ...["Active", "Awaiting result", "Won", "Lost", "Void"].map<
        [string, ReactNode]
      >((s) => [s, row(s)]),
      ["Created by you", created],
    ],
    "PM-04": [
      [
        "No bets, created questions remain",
        <>
          <p>No bets yet</p>
          {created}
        </>,
      ],
      [
        "Bets remain, no created questions",
        <>
          {row("Active")}
          <p>No predictions created</p>
        </>,
      ],
    ],
    "PM-05": [
      ["Initial loading", <Skeleton />],
      [
        "Refresh failed",
        <>
          {row("Active")}
          <Notice>Could not update predictions</Notice>
        </>,
      ],
      [
        "One section unavailable",
        <>
          {row("Active")}
          <p>Could not load created predictions</p>
        </>,
      ],
      [
        "Loading more",
        <>
          {row("Active")}
          <Skeleton count={1} />
        </>,
      ],
      ["Initial read failed", <Notice>Could not load predictions</Notice>],
      ["Account unavailable", <p>Predictions unavailable</p>],
    ],
    "PM-06": [
      [
        "Personal search · ring",
        <>
          <p>No matching bets</p>
          {created}
        </>,
      ],
      [
        "Personal search · zzzz",
        <>
          <p>No matching bets</p>
          <p>No matching predictions</p>
        </>,
      ],
      ["Keyboard focus", row("Active")],
    ],
    "PC-03": [
      ["Prediction type", form("types")],
      ["End date", form("calendar")],
      ["Date limits", form("early")],
      ["Editing and focus", form("edit")],
    ],
    "PC-04": [
      ["No card selected", form("missing")],
      ["Checking price", form("checking")],
      ["Price unavailable", form("source")],
      ["Invalid target", form("invalid")],
      ["End date too early", form("early")],
      [
        "Read feedback",
        <>
          <Skeleton />
          <p>No matching cards</p>
          <Notice>Couldn’t load cards.</Notice>
        </>,
      ],
    ],
    "PC-05": [
      ["Publishing", form("publishing")],
      [
        "Created",
        <>
          <Tile q={base[0]!} go={go} />
          <Notice>Prediction created.</Notice>
        </>,
      ],
      ["Creation failed", form("failed")],
    ],
    "BF-04": [
      ["Checking winnings", ticket("checking")],
      ["Changed winnings", ticket("changed")],
      ["Expired winnings", ticket("expired")],
      ["Read failed", ticket("failed")],
      ["No side", ticket("quote-no")],
      ["Selected side unavailable", ticket("no-side")],
    ],
    "BF-05": [
      ["Empty or zero", ticket("empty")],
      ["Invalid amount", ticket("fraction")],
      ["Insufficient coins", ticket("balance")],
      ["Insufficient available amount", ticket("liquidity")],
    ],
    "BF-06": [
      ["Betting closed", ticket("closed")],
      ["Prediction unavailable", ticket("gone")],
      ["Account unavailable", ticket("account")],
      ["Bets unavailable", ticket("inactive")],
    ],
    "BF-08": [
      [
        "Opened from Explore",
        <>
          <Tile q={current()} go={go} />
          <Notice>Bet placed</Notice>
        </>,
      ],
      [
        "Opened from My predictions",
        <>
          {row("Active")}
          {created}
          <Notice>Bet placed</Notice>
        </>,
      ],
      [
        "Accepted, refresh failed",
        <>
          <Tile q={current()} go={go} />
          <Notice>Bet placed</Notice>
          <Notice>Could not update predictions</Notice>
        </>,
      ],
    ],
    "RS-02": [
      ["Yes outcome, lost", result("loss")],
      ["No outcome, won", result("no-win")],
      ["No outcome, lost", result("no-loss")],
      ["At least on end date", result("win")],
    ],
    "RS-05": [
      ["Result Yes", result("yes", "RS-05")],
      ["Result No", result("no", "RS-05")],
      ["Void", result("void", "RS-05")],
      ["Pending", result("pending", "RS-05")],
    ],
    "RS-06": [
      ["Loading result", result("loading")],
      ["Result read failed", result("read")],
      ["Loading personal result", <Skeleton />],
      ["Personal result failed", result("unverified")],
      ["Price read failed", result("price")],
      ["Refresh failed", result("refresh")],
    ],
    "RS-07": [
      ["Account unavailable", result("account")],
      ["Prediction unavailable", result("gone")],
      ["Personal result unverified", result("unverified")],
      ["Owner changed", result("owner")],
    ],
  };
  if (id === "PC-04")
    return (
      <Artboard width={860} height={612}>
        <Box area={[44, 3, 650, 43]}>
          <h2>Spellwake — Predictions Creation Details</h2>
          <small>Component State Board (A – F)</small>
        </Box>
        <Box area={[554, 8, 264, 27]} className="soc-use-case">
          <small>Use case: Create prediction (no bet placed)</small>
        </Box>
        {boards[id]!.map(([label, panel], i) => {
          const x = 44 + (i % 3) * 264,
            y = 50 + Math.floor(i / 3) * 252;
          return (
            <div key={label}>
              <Box area={[x, y, 242, 22]}>
                <b>
                  {String.fromCharCode(65 + i)}. {label}
                </b>
              </Box>
              <Box
                area={[x, y + 21, 242, 218]}
                className={
                  "soc-create-excerpt" + (i === 5 ? " soc-source-feedback" : "")
                }
                frame={i === 5}
                scale={0.51}
              >
                {i === 5 ? (
                  <>
                    <section>
                      <b>F1. No matches</b>
                      <Window
                        title="Choose card"
                        close={() => go("PC-02")}
                        embedded
                      >
                        <Search
                          label="Search cards"
                          value="zzzz"
                          set={() => go("PC-02")}
                        />
                        <p>No matching cards.</p>
                        <Btn disabled>Select</Btn>
                      </Window>
                    </section>
                    <section>
                      <b>F2. Loading</b>
                      <Window
                        title="Choose card"
                        close={() => go("PC-02")}
                        embedded
                      >
                        <Skeleton count={2} />
                        <Btn disabled>Select</Btn>
                      </Window>
                    </section>
                    <section>
                      <b>F3. Read failure</b>
                      <Notice>Couldn't load cards.</Notice>
                    </section>
                  </>
                ) : (
                  panel
                )}
              </Box>
            </div>
          );
        })}
      </Artboard>
    );
  if (id === "BF-04")
    return (
      <Artboard width={860} height={538}>
        <Box area={[205, 3, 460, 46]} className="soc-winnings-heading">
          <h1>Winnings and availability</h1>
        </Box>
        {boards[id]!.map(([label, panel], i) => {
          const x = i % 2 ? 452 : 21,
            y = 62 + Math.floor(i / 2) * 146;
          return (
            <div key={label}>
              <Box
                area={[x - 5, y + 7, 76, 114]}
                className="soc-winnings-label"
              >
                <b>{String.fromCharCode(65 + i)}</b>
                <p>{label}</p>
              </Box>
              <Box
                area={[x + 72, y, 337, 135]}
                scale={0.64}
                className="soc-winnings-excerpt"
              >
                {panel}
              </Box>
            </div>
          );
        })}
        <Box area={[255, 514, 420, 23]} className="soc-winnings-heading">
          <small>Illustrative quotes · no bet submitted</small>
        </Box>
      </Artboard>
    );
  if (boards[id])
    return (
      <Board
        scenic
        title={
          (
            {
              "PM-02": "Personal prediction states",
              "BF-04": "Winnings and availability",
              "BF-05": "Bet amount feedback",
              "PC-03": "Creation controls",
            } as Record<string, string>
          )[id] ?? ""
        }
        panels={boards[id]!}
        cols={["PM-04", "PM-06", "BF-08"].includes(id) ? 1 : 2}
      />
    );
  const isModal = /^(PC|BF|RS)-/.test(id);
  return (
    <Shell area="Market" current="EX-01" go={go}>
      <PredictionHome
        id={id}
        go={go}
        tab={
          isModal && sessionStorage.getItem("sw-prediction-origin") === "PM-01"
            ? "personal"
            : ""
        }
      />
      {id.startsWith("PC-") ? (
        <Create id={id} go={go} />
      ) : id.startsWith("BF-") ? (
        <Ticket id={id} go={go} />
      ) : id.startsWith("RS-") ? (
        <Result id={id} go={go} />
      ) : null}
    </Shell>
  );
}

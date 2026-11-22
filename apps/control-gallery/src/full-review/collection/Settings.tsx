import { useState } from "react";
import { RangeSlider, RadioGroup } from "@spellwake/ui";
import { Box, Dices, Layers, Palette, PawPrint, Scroll } from "lucide-react";
import {
  Art,
  Board,
  Btn,
  Example,
  Modal,
  Search,
  Tabs,
  type Jump,
  type Rect,
} from "./common";
import { DeckLibrary } from "./Library";
const categories = ["Sleeves", "Card backs", "Dice", "Playmats", "Pets"];
const glyphs = [Layers, Box, Dices, Scroll, PawPrint];
const names: Record<string, string[]> = {
  Sleeves: [
    "Classic",
    "Midnight",
    "Ember",
    "Moonstone",
    "Forest",
    "Storm",
    "Twilight",
  ],
  "Card backs": ["Classic", "Moon", "Ember"],
  Dice: ["Ivory", "Midnight", "Ember"],
  Playmats: ["None", "Moonlit", "Forest"],
  Pets: ["None", "Owl", "Fox"],
};
const priced = ["$4.99", "$2.99", "$4.99", "$2.99", "$4.99", "$2.99"];
const alternateRects: Record<string, readonly Rect[]> = {
  "Card backs": [
    [244, 186, 73, 126],
    [344, 185, 79, 133],
    [451, 185, 75, 133],
  ],
  Dice: [
    [1028, 226, 64, 72],
    [1134, 226, 65, 72],
    [1240, 226, 65, 72],
  ],
  Playmats: [
    [244, 680, 73, 107],
    [345, 674, 80, 125],
    [447, 674, 79, 125],
  ],
  Pets: [
    [1040, 680, 62, 102],
    [1133, 673, 76, 121],
    [1232, 673, 76, 121],
  ],
};
function CosmeticArt({
  category,
  index,
  display = false,
}: {
  category: string;
  index: number;
  display?: boolean;
}) {
  if (category === "Sleeves") {
    if (display && index === 0)
      return (
        <Art
          source="DS-01"
          rect={[1013, 370, 346, 317]}
          alt="Classic sleeves"
        />
      );
    if (display && index === 1)
      return (
        <Art
          source="CC-01"
          rect={[985, 358, 350, 234]}
          alt="Midnight sleeves"
        />
      );
    if (index === 6)
      return <Art source="CC-01" rect={[796, 600, 119, 137]} alt="Twilight" />;
    if (index === 0)
      return <Art source="DS-02" rect={[601, 177, 120, 174]} alt="Classic" />;
    const i = index % 6;
    return (
      <Art
        source="DS-01"
        rect={
          [
            [474, 319, 121, 161],
            [649, 317, 123, 163],
            [822, 317, 123, 163],
            [471, 546, 128, 164],
            [646, 546, 130, 164],
            [822, 546, 125, 164],
          ][i] as unknown as Rect
        }
        alt={names.Sleeves?.[index] ?? ""}
      />
    );
  }
  const rect = alternateRects[category]?.[index % 3];
  return rect ? <Art source="DS-02" rect={rect} /> : null;
}
export function CosmeticsDialog({
  kind,
  go,
  initial = "Sleeves",
  ownership = false,
  scenario = "",
  onClose,
}: {
  kind: "cosmetics" | "defaults" | "appearance";
  go: Jump;
  initial?: string;
  ownership?: boolean;
  scenario?: string;
  onClose?: () => void;
}) {
  const [category, setCategory] = useState(initial),
    [owned, setOwned] = useState(ownership),
    [selected, select] = useState(kind === "cosmetics" ? 1 : 0),
    [q, search] = useState(scenario === "No matches" ? "zzzz" : ""),
    [notice, setNotice] = useState("");
  const shop = kind === "cosmetics" && !owned,
    options = names[category] ?? [],
    close = onClose ?? (() => go(kind === "appearance" ? "DB-01" : "DL-01"));
  const titled =
    kind === "appearance"
      ? "Deck appearance"
      : kind === "defaults"
        ? "Default settings"
        : "Cosmetics";
  return (
    <Modal
      title={titled}
      {...(kind === "defaults"
        ? { subtitle: "Automatically applies to all new decks." }
        : {})}
      close={close}
      className="cs-cosmetics"
      {...(kind !== "cosmetics"
        ? {
            footer: (
              <>
                <Btn onClick={close}>Cancel</Btn>
                <Btn primary onClick={close}>
                  {kind === "appearance" ? "Apply" : "Save"}
                </Btn>
              </>
            ),
          }
        : {})}
    >
      {kind === "cosmetics" && (
        <Tabs
          labels={["Your cosmetics", "Shop"]}
          value={owned ? "Your cosmetics" : "Shop"}
          change={(v) => setOwned(v === "Your cosmetics")}
        />
      )}
      <div className="cs-cos-columns">
        <nav>
          {categories.map((v, i) => {
            const Icon = glyphs[i] ?? Palette;
            return (
              <button
                key={v}
                aria-current={category === v ? "page" : undefined}
                onClick={() => {
                  setCategory(v);
                  select(0);
                  search("");
                }}
              >
                <Icon />
                {v}
              </button>
            );
          })}
        </nav>
        <div className="cs-cos-products">
          {kind === "cosmetics" && (
            <Search label="Search cosmetics" value={q} onChange={search} />
          )}
          <div className="cs-cos-grid">
            {(scenario === "Empty category" ? [] : options).map((name, i) =>
              name.toLowerCase().includes(q.toLowerCase()) &&
              (!shop || i > 0) &&
              !(category === "Sleeves" && !shop && i === 6) ? (
                <button
                  key={name}
                  aria-pressed={i === selected}
                  onClick={() => select(i)}
                >
                  {scenario === "Loading" ? (
                    <div className="cs-skeleton" />
                  ) : (
                    <CosmeticArt category={category} index={i} />
                  )}
                  <span>{name}</span>
                  {shop && <small>{priced[(i - 1) % 6]}</small>}
                </button>
              ) : null,
            )}
          </div>
          {(!options.some((n) => n.toLowerCase().includes(q.toLowerCase())) ||
            scenario === "Empty category") && (
            <p className="cs-center">No cosmetics found.</p>
          )}
        </div>
        <aside className="cs-cos-preview">
          <CosmeticArt category={category} index={selected} display />
          {kind === "cosmetics" && (
            <>
              <h2>{options[selected]}</h2>
              <i>{category === "Sleeves" ? "Card sleeve" : category}</i>
              {shop ? (
                <>
                  <h3>{priced[Math.max(0, selected - 1) % 6]} USD</h3>
                  <Btn primary onClick={() => setNotice("Opening checkout…")}>
                    Buy
                  </Btn>
                </>
              ) : (
                <small>Owned</small>
              )}
            </>
          )}
        </aside>
      </div>
      {notice && <p className="cs-notice">{notice}</p>}
    </Modal>
  );
}
export function CosmeticScreen({ id, go }: { id: string; go: Jump }) {
  const defaults = id.startsWith("DS");
  const board = defaults ? id !== "DS-01" : !["CC-01", "CC-02"].includes(id);
  if (!board)
    return (
      <>
        <DeckLibrary id="DL-01" go={go} />
        <CosmeticsDialog
          kind={defaults ? "defaults" : "cosmetics"}
          go={go}
          ownership={id === "CC-02"}
        />
      </>
    );
  const labels =
    id === "CC-04"
      ? ["Loading", "Matches", "No matches", "Empty category", "Large library"]
      : id === "CC-05"
        ? [
            "Begin checkout",
            "Checkout opened",
            "Checking purchase",
            "Ownership accepted",
            "Known failure",
            "Unknown confirmation",
          ]
        : id === "DS-03"
          ? [
              "Account entry",
              "Opening defaults",
              "Save availability",
              "Saving defaults",
              "Save failed",
              "Unconfirmed",
            ]
          : categories.slice(1);
  return (
    <Board>
      {labels.map((v, i) => (
        <Example key={v} title={v} width={1120} height={660}>
          {id === "DS-03" && i === 0 ? (
            <div className="cs-center">
              <div className="cs-panel cs-account-entry">
                <h2>Planeswalker</h2>
                <p>View profile</p>
                {["Cosmetics", "Default settings", "Sign out"].map(
                  (label, j) => (
                    <Btn
                      key={label}
                      onClick={() =>
                        go(["CC-01", "DS-01", "AE-01"][j] ?? "DS-01")
                      }
                    >
                      {label}
                    </Btn>
                  ),
                )}
              </div>
            </div>
          ) : id === "DS-03" && i === 2 ? (
            <div className="cs-center">
              <div className="cs-toolbar">
                <Btn disabled>Save</Btn>
                <Btn onClick={() => go("DS-01")}>Save</Btn>
                <Btn disabled>Save</Btn>
              </div>
            </div>
          ) : (
            <>
              <CosmeticsDialog
                kind={defaults ? "defaults" : "cosmetics"}
                go={go}
                ownership={!defaults && id !== "CC-05"}
                initial={categories.includes(v) ? v : "Sleeves"}
                scenario={v}
              />
              {(id === "CC-05" || (id === "DS-03" && i > 2)) && (
                <span className="cs-toast">
                  {id === "CC-05"
                    ? [
                        "Opening checkout…",
                        "Checkout opened.",
                        "Checking purchase…",
                        "Cosmetic added.",
                        "Couldn’t open checkout.",
                        "Couldn’t confirm purchase.",
                      ][i]
                    : [
                        "Saving defaults…",
                        "Couldn’t save defaults.",
                        "Couldn’t confirm defaults.",
                      ][i - 3]}
                </span>
              )}
            </>
          )}
        </Example>
      ))}
    </Board>
  );
}
const accountTabs = ["Profile", "Patreon", "Billing & purchases", "Account"];
export function AccountSettings({ id, go }: { id: string; go: Jump }) {
  const number = Number(id.slice(4)),
    active =
      number < 5 || number === 11 ? 0 : number < 7 ? 1 : number === 7 ? 2 : 3;
  const [q, search] = useState(number === 11 ? "zzzz" : "dragon"),
    [zoom, setZoom] = useState(145),
    [badge, setBadge] = useState("none"),
    [drag, move] = useState([0, 0]),
    [notice, setNotice] = useState("");
  const tabgo = (i: number) =>
    go(["AS-R01", "AS-R05", "AS-R07", "AS-R08"][i] ?? "AS-R01");
  return (
    <>
      <DeckLibrary id="DL-01" go={go} />
      <Modal
        title="Account settings"
        close={() => go("DL-01")}
        className="cs-account"
      >
        <div className="cs-account-columns">
          <nav>
            {accountTabs.map((v, i) => (
              <button
                key={v}
                aria-current={active === i ? "page" : undefined}
                onClick={() => tabgo(i)}
              >
                {v}
              </button>
            ))}
          </nav>
          <div className="cs-account-content">
            {active === 0 ? (
              <div className="cs-person">
                <Art
                  source="AS-R01"
                  rect={[627, 412, 169, 168]}
                  className="cs-avatar"
                />
                <div>
                  <h2>Planeswalker</h2>
                  <Btn onClick={() => go("AS-R02")}>Change picture</Btn>{" "}
                  <Btn onClick={() => go("AS-R04")}>Change badge</Btn>
                </div>
              </div>
            ) : active === 1 ? (
              <>
                <h2>◯ Patreon</h2>
                <p>{number === 6 ? "Linked" : "Not linked."}</p>
                {number === 6 && <p>✓ Active supporter</p>}
                <Btn
                  onClick={() =>
                    setNotice(
                      number === 6 ? "Manage on Patreon" : "Connect Patreon",
                    )
                  }
                >
                  {number === 6 ? "Manage on Patreon" : "Connect Patreon"}
                </Btn>
              </>
            ) : active === 2 ? (
              <>
                <header>
                  <h2>Billing & purchases</h2>
                  <Btn onClick={() => setNotice("Manage billing")}>
                    Manage billing
                  </Btn>
                </header>
                <h3>Purchases</h3>
                <table>
                  <thead>
                    <tr>
                      {["Item", "Date", "Amount", "Status"].map((v) => (
                        <th key={v}>{v}</th>
                      ))}
                    </tr>
                  </thead>
                  <tbody>
                    {[
                      "Midnight sleeve",
                      "Stormglass dice",
                      "Forest sleeve",
                      "Moonstone back",
                    ].map((v, i) => (
                      <tr key={v}>
                        <td>{v}</td>
                        <td>Nov {i === 0 ? 1 : i === 3 ? 3 : 2}, 2026</td>
                        <td>{["$2.99", "$3.00", "$1.00", "$1.00"][i]} USD</td>
                        <td>
                          {["Paid", "Pending", "Refunded", "Reversed"][i]}
                        </td>
                      </tr>
                    ))}
                  </tbody>
                </table>
              </>
            ) : number === 10 ? (
              <>
                <h2>Deletion requested</h2>
                <p>Waiting for connected activity to finish.</p>
                <Btn onClick={() => go("DL-01")}>Close</Btn>
              </>
            ) : (
              <>
                <h2>Account</h2>
                <div className="cs-toolbar">
                  <p>Delete account</p>
                  <Btn onClick={() => go("AS-R09")}>Delete account</Btn>
                </div>
              </>
            )}
            {notice && <p>{notice}</p>}
          </div>
        </div>
      </Modal>
      {(number === 2 || number === 11) && (
        <Modal
          title="Profile picture"
          close={() => go("AS-R01")}
          className="cs-picture"
        >
          <Search label="Search artwork" value={q} onChange={search} />
          <div className="cs-picture-grid">
            {[
              "Shivan Dragon",
              "Dragon Whelp",
              "Goldspan Dragon",
              "Shivan Devastator",
            ].map((name, i) =>
              (name + " dragon").toLowerCase().includes(q.toLowerCase()) ? (
                <button key={name} onClick={() => go("AS-R03")}>
                  <Dragon i={i} />
                  {name}
                </button>
              ) : null,
            )}
          </div>
          {![
            "shivan dragon",
            "dragon whelp",
            "goldspan dragon",
            "shivan devastator",
          ].some((n) => n.includes(q.toLowerCase())) && (
            <div className="cs-center">
              <h2>No artwork found.</h2>
              <p>Try another search.</p>
            </div>
          )}
        </Modal>
      )}
      {number === 3 && (
        <Modal
          title="Profile picture"
          close={() => go("AS-R01")}
          className="cs-picture"
          footer={
            <>
              <Btn onClick={() => go("AS-R02")}>Cancel</Btn>
              <Btn primary onClick={() => go("AS-R01")}>
                Save
              </Btn>
            </>
          }
        >
          <p>Drag to reposition.</p>
          <div
            className="cs-crop-picture"
            onPointerDown={(e) =>
              e.currentTarget.setPointerCapture(e.pointerId)
            }
            onPointerMove={(e) => {
              if (e.currentTarget.hasPointerCapture(e.pointerId))
                move((old) => [
                  (old[0] ?? 0) + e.movementX,
                  (old[1] ?? 0) + e.movementY,
                ]);
            }}
          >
            <div
              style={{
                transform:
                  "translate(" +
                  drag[0] +
                  "px," +
                  drag[1] +
                  "px) scale(" +
                  zoom / 100 +
                  ")",
              }}
            >
              <Dragon i={0} />
            </div>
          </div>
          <small>Shivan Dragon · Donato Giancola</small>
          <RangeSlider
            label="Zoom"
            min={100}
            max={200}
            step={1}
            value={zoom}
            valueText={zoom + "%"}
            showStepButtons
            onValueChange={setZoom}
          />
        </Modal>
      )}
      {number === 4 && (
        <Modal
          title="Badge"
          close={() => go("AS-R01")}
          className="cs-compact-modal"
          footer={
            <>
              <Btn onClick={() => go("AS-R01")}>Cancel</Btn>
              <Btn onClick={() => go("AS-R01")}>Save</Btn>
            </>
          }
        >
          <RadioGroup
            label="Badge"
            options={[
              { value: "none", label: "None" },
              { value: "supporter", label: "Supporter ✦" },
            ]}
            value={badge}
            onValueChange={setBadge}
          />
        </Modal>
      )}
      {number === 9 && (
        <Modal
          title="Delete your account?"
          close={() => go("AS-R08")}
          className="cs-compact-modal"
          footer={
            <>
              <Btn onClick={() => go("AS-R08")}>Cancel</Btn>
              <Btn onClick={() => go("AS-R10")}>Delete account</Btn>
            </>
          }
        >
          <p>
            This cannot be undone. Your cards, packs, and coins will be removed.
          </p>
        </Modal>
      )}
    </>
  );
}
function Dragon({ i }: { i: number }) {
  return (
    <Art
      source="AS-R02"
      rect={
        [
          [484, 325, 297, 168],
          [806, 325, 300, 168],
          [483, 550, 300, 160],
          [807, 550, 298, 160],
        ][i] as unknown as Rect
      }
    />
  );
}

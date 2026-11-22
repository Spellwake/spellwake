import { ChevronDown, CircleUserRound, MessageSquare } from "lucide-react";
import coin from "../../../../assets/ui-study/coin.svg?inline";
import "./review-header.css";

const destinations = {
  "Open Play": "DL-05",
  Collection: "DL-01",
  Market: "ML-01",
  League: "LS-01",
} as const;
export function ReviewHeader({
  active,
  go,
  coins = 1000,
}: {
  active?: keyof typeof destinations;
  go: (id: string) => void;
  coins?: number;
}) {
  return (
    <header className="sw-review-header">
      <span className="sw-review-wordmark">Spellwake</span>
      <nav aria-label="Application">
        {(Object.keys(destinations) as (keyof typeof destinations)[]).map(
          (label) => (
            <button
              key={label}
              aria-current={label === active ? "page" : undefined}
              onClick={() => go(destinations[label])}
            >
              {label}
            </button>
          ),
        )}
      </nav>
      <span className="sw-review-balance">
        <img src={coin} alt="Coins" />
        {coins.toLocaleString("en-US")}
      </span>
      <button aria-label="Messages" onClick={() => go("MSG-D01")}>
        <MessageSquare size={31} />
      </button>
      <button
        className="sw-review-person"
        aria-label="Account settings"
        onClick={() => go("AS-R01")}
      >
        <CircleUserRound size={36} aria-hidden="true" />
        <ChevronDown size={22} />
      </button>
    </header>
  );
}

import { useCallback, type ReactElement } from "react";
import { useLocation, useNavigate, useParams } from "react-router-dom";
import { useAccount } from "../auth/AccountProvider";
import { TradePage } from "../routes/trade/TradePage";
import { useConnectedTrade } from "./connected-trade";
import { useDemoTrade, useForgetDemoTrade } from "./demo-trade";

/** Where Back and Done go when the room was opened directly, not from another surface. */
const FALLBACK = "/collection/decks";
/** The surface that opened the room, such as a Messages thread over Decks. */
function useOrigin(): string {
  const location = useLocation();
  const state: unknown = location.state;
  return state &&
    typeof state === "object" &&
    "from" in state &&
    typeof state.from === "string" &&
    state.from.startsWith("/")
    ? state.from
    : FALLBACK;
}

function PreviewTrade({ partnerId }: { partnerId: string }): ReactElement {
  const navigate = useNavigate();
  const origin = useOrigin();
  const forget = useForgetDemoTrade();
  const leave = useCallback(() => void navigate(origin), [navigate, origin]);
  const finish = useCallback(() => {
    forget(partnerId);
    void navigate(origin);
  }, [forget, partnerId, navigate, origin]);
  return (
    <TradePage
      source={useDemoTrade(partnerId)}
      onBack={leave}
      onFinish={finish}
    />
  );
}
function AccountTrade(): ReactElement {
  const navigate = useNavigate();
  const origin = useOrigin();
  const leave = useCallback(() => void navigate(origin), [navigate, origin]);
  return (
    <TradePage source={useConnectedTrade()} onBack={leave} onFinish={leave} />
  );
}

/**
 * The trade room with one person. Leaving keeps the room; only Cancel trade
 * closes it. A build with no application API uses the local preview room.
 */
export function TradeRoute(): ReactElement {
  const { personId = "" } = useParams();
  return useAccount().state.status === "unconfigured" ? (
    <PreviewTrade key={personId} partnerId={personId} />
  ) : (
    <AccountTrade />
  );
}

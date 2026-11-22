import type { ReactElement } from "react";
import { useAccount } from "../auth/AccountProvider";
import { ListingsPage } from "../routes/market/ListingsPage";
import { SupplyPage } from "../routes/market/SupplyPage";
import { PredictionsPage } from "../routes/market/PredictionsPage";
import { useConnectedPredictions } from "./connected-predictions";
import { usePreviewPredictions } from "./demo-predictions";
import { useDemoSupply } from "./demo-supply";
import { useConnectedListings } from "./connected-listings";
import { useDemoListings } from "./demo-listings";
import { useConnectedSupply } from "./connected-supply";

function PreviewListings(): ReactElement {
  return <ListingsPage source={useDemoListings()} />;
}
function AccountListings(): ReactElement {
  return <ListingsPage source={useConnectedListings()} />;
}

/**
 * Market Listings. A build that names an application API uses the account's
 * market; only a build with none uses the local preview market.
 */
export function ListingsRoute(): ReactElement {
  return useAccount().state.status === "unconfigured" ? (
    <PreviewListings />
  ) : (
    <AccountListings />
  );
}

function PreviewSupply(): ReactElement {
  return <SupplyPage source={useDemoSupply()} />;
}
function AccountSupply(): ReactElement {
  return <SupplyPage source={useConnectedSupply()} />;
}
/** Market Supply, from the account where an application API is named, else the local preview. */
export function SupplyRoute(): ReactElement {
  return useAccount().state.status === "unconfigured" ? (
    <PreviewSupply />
  ) : (
    <AccountSupply />
  );
}

function PreviewPredictions(): ReactElement {
  return <PredictionsPage source={usePreviewPredictions()} />;
}
function AccountPredictions(): ReactElement {
  return <PredictionsPage source={useConnectedPredictions()} />;
}
/** Market Predictions, from the account where an application API is named, else the local preview. */
export function PredictionsRoute(): ReactElement {
  return useAccount().state.status === "unconfigured" ? (
    <PreviewPredictions />
  ) : (
    <AccountPredictions />
  );
}

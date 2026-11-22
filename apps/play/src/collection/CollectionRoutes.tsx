import type { ReactElement } from "react";
import { useAccount } from "../auth/AccountProvider";
import { CardsPage } from "../routes/cards/CardsPage";
import { PacksPage } from "../routes/packs/PacksPage";
import { useConnectedCards } from "./connected-cards";
import { useConnectedPacks } from "./connected-packs";
import { useDemoCards } from "./demo-cards";
import { useDemoPacks } from "./demo-packs";

/**
 * Chooses where Cards and Packs get their data, as the deck screens do: the
 * account in any build that names an application API, and the local preview
 * only in a build with none.
 */
const useIsPreview = (): boolean =>
  useAccount().state.status === "unconfigured";

function AccountCards(): ReactElement {
  return <CardsPage source={useConnectedCards()} />;
}
function PreviewCards(): ReactElement {
  return <CardsPage source={useDemoCards()} />;
}
export function CardsRoute(): ReactElement {
  return useIsPreview() ? <PreviewCards /> : <AccountCards />;
}

function PreviewPacks(): ReactElement {
  return <PacksPage source={useDemoPacks()} />;
}
function AccountPacks(): ReactElement {
  return <PacksPage source={useConnectedPacks()} />;
}
export function PacksRoute(): ReactElement {
  return useIsPreview() ? <PreviewPacks /> : <AccountPacks />;
}

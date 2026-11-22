import type { ScreenProps } from "../../review-types";
import { AccountEntry } from "./AccountEntry";
import { League } from "./League";
import { Listings } from "./Listings";
import { Messaging } from "./Messaging";
import { Predictions } from "./Predictions";
import { Supply } from "./Supply";
import "./social.css";

const renderers = {
  "account-entry": AccountEntry,
  league: League,
  "public-profile": League,
  market: Listings,
  messaging: Messaging,
  "market-predictions": Predictions,
  "market-supply": Supply,
};
/** Standalone, local review fixtures. No service, identity or economic operation is invoked. */
export default function SocialScreen({ section, view, go, data }: ScreenProps) {
  const Component = renderers[section as keyof typeof renderers];
  return (
    <div className="soc-scene" data-social-view={view.id}>
      <Component key={view.id} id={view.id} go={go} data={data} />
    </div>
  );
}

import type { ReactElement, SVGProps } from "react";
import { Boxes, Tag } from "lucide-react";

type IconProps = SVGProps<SVGSVGElement>;
const frame = {
  viewBox: "0 0 24 24",
  fill: "none",
  stroke: "currentColor",
} as const;

/** Section glyphs drawn to the approved desktop rail references. Stroke width comes from the consumer. */
export function DecksIcon(props: IconProps): ReactElement {
  return (
    <svg {...frame} {...props}>
      <rect x="4" y="2.5" width="16" height="19" rx="2" />
      <path d="M4 12h4l2.2 2.5h3.6L16 12h4" />
    </svg>
  );
}
export function CardsIcon(props: IconProps): ReactElement {
  return (
    <svg {...frame} {...props}>
      <rect
        x="4"
        y="5"
        width="12"
        height="16"
        rx="2"
        transform="rotate(-19 10 13)"
      />
      <path d="m13 3 3.6-.8a2 2 0 0 1 2.4 1.5l2.1 9.7a2 2 0 0 1-1.6 2.4" />
    </svg>
  );
}
export function PacksIcon(props: IconProps): ReactElement {
  return (
    <svg {...frame} strokeLinejoin="round" {...props}>
      <path d="m4 2 3 1 3-1 3 1 3-1 4 1-1 3v12l1 4-4-1-3 1-3-1-3 1-3-1 1-3V6zM5 6h14M5 18h14" />
    </svg>
  );
}
export function ListingsIcon(props: IconProps): ReactElement {
  return <Tag {...props} />;
}
export function PredictionsIcon(props: IconProps): ReactElement {
  return (
    <svg {...frame} strokeWidth="1.35" {...props}>
      <circle cx="12" cy="10" r="8.5" />
      <path d="M12 4a6 6 0 0 1 5 3M7 18l-1.6 3h13.2L17 18" />
    </svg>
  );
}
export function SupplyIcon(props: IconProps): ReactElement {
  return <Boxes {...props} />;
}
export function SeasonIcon(props: IconProps): ReactElement {
  return (
    <svg {...frame} strokeLinejoin="round" {...props}>
      <path d="M6 2.5h12v7c0 3.4-2.2 5.1-4.5 5.9v2.2c0 1.4 1.3 2.4 2.6 3H7.9c1.3-.6 2.6-1.6 2.6-3v-2.2C8.2 14.6 6 12.9 6 9.5zM6 4.5H2.5v3.2c0 2.3 1.3 3.6 3.8 3.9M18 4.5h3.5v3.2c0 2.3-1.3 3.6-3.8 3.9M7.5 21.6h9" />
    </svg>
  );
}
export function StandingsIcon(props: IconProps): ReactElement {
  return (
    <svg {...frame} {...props}>
      <path
        strokeLinejoin="round"
        strokeLinecap="round"
        d="M1.5 21.5h21M3.5 21v-7.5c0-1.3 3-1.3 3 0V21M9.7 21V3.6c0-1.7 3.6-1.7 3.6 0V21m-3.6-15h3.6m3 15V10.2c0-1.5 3.6-1.5 3.6 0V21"
      />
    </svg>
  );
}

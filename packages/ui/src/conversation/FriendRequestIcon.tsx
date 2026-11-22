import type { SVGProps } from "react";

export function FriendRequestIcon(props: SVGProps<SVGSVGElement>) {
  return (
    <svg viewBox="0 0 40 35" fill="none" stroke="currentColor" {...props}>
      <circle cx="14" cy="10" r="7" />
      <path d="M2 31v-3c0-5 5-8 12-8s12 3 12 8v3ZM31 13v12m-6-6h12" />
    </svg>
  );
}

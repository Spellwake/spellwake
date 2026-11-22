import { useEffect, useState, type ReactElement } from "react";
import { Navigate } from "react-router-dom";
import { useSetAtom } from "jotai";
import { patreonReturnAtom } from "../../account/connected-patreon";
import { parsePatreonReturn } from "../../data/patreon-link";

/**
 * Where Patreon sends a person back after its authorisation. This screen
 * shows nothing of its own: it takes the code and state out of the address,
 * keeps them in memory, and reopens Account settings at Patreon, which
 * finishes the link. The address is replaced at once, so the code is never
 * left in history or shared by a copied link.
 */
export function PatreonReturnRoute(): ReactElement {
  const hold = useSetAtom(patreonReturnAtom);
  const [returned] = useState(() => parsePatreonReturn(window.location.search));
  useEffect(() => {
    if (returned) hold(returned);
  }, [returned, hold]);
  return <Navigate to="/collection/decks?account=patreon" replace />;
}

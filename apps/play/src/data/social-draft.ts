import { useEffect, useMemo, useRef, useState } from "react";
import { atom, useAtomValue, useStore } from "jotai";
import { socialMessageBodySchema } from "@spellwake/application-contracts";
import { useAccount } from "../auth/AccountProvider";
import { createSocialDraft } from "./social-journal";

const emptyDraftAtom = atom({ status: "empty" } as const);
type LocalDraft = { key: string; text: string };

/** Immediate typing with fenced durable writes; a confirmed send clears only its unchanged text. */
export function useSocialDraft(conversationId: string | null) {
  const { state, services, isCurrentSession } = useAccount();
  const owner =
    state.status === "ready" && state.session.state !== "needs_registration"
      ? state.session
      : null;
  const atoms = useStore();
  const cell = useMemo(
    () =>
      owner && services && conversationId
        ? createSocialDraft(
            services.api.origin,
            owner.accountId,
            owner.subjectId,
            conversationId,
            () => window.localStorage,
          )
        : null,
    [services, owner?.accountId, owner?.subjectId, conversationId],
  );
  const durable = useAtomValue(cell?.stateAtom ?? emptyDraftAtom);
  const key = cell?.key ?? "";
  const [local, setLocal] = useState<LocalDraft | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [saving, setSaving] = useState<string | null>(null);
  const pending = useRef(new Map<string, number>());
  useEffect(() => {
    if (durable.status === "unavailable") return;
    if ((pending.current.get(key) ?? 0) === 0)
      setLocal({
        key,
        text: durable.status === "saved" ? durable.value.text : "",
      });
  }, [key, durable]);
  const text =
    local?.key === key
      ? local.text
      : durable.status === "saved"
        ? durable.value.text
        : "";
  function change(text: string) {
    if (!cell || !owner || !isCurrentSession(owner)) return;
    if (Array.from(text).length > 4000) return;
    setLocal({ key, text });
    setError(null);
    pending.current.set(key, (pending.current.get(key) ?? 0) + 1);
    setSaving(key);
    void atoms
      .set(cell.saveAtom, text)
      .then((value) => {
        if (!value) throw new Error("Draft write was not confirmed.");
      })
      .catch(() => {
        console.warn("social_draft code=save_unconfirmed");
        if (isCurrentSession(owner)) setError(key);
      })
      .finally(() => {
        const remaining = (pending.current.get(key) ?? 1) - 1;
        pending.current.set(key, remaining);
        if (remaining !== 0 || !isCurrentSession(owner)) return;
        setSaving((current) => (current === key ? null : current));
        const latest = atoms.get(cell.stateAtom);
        if (latest.status === "saved")
          setLocal((current) =>
            current?.key === key ? { key, text: latest.value.text } : current,
          );
      });
  }
  async function clearSent(expected: string) {
    if (!cell || !owner || !isCurrentSession(owner)) return;
    const cleared = await atoms.set(cell.clearIfTextAtom, expected);
    if (cleared && isCurrentSession(owner))
      setLocal((current) =>
        current?.key === key && current.text === expected
          ? { key, text: "" }
          : current,
      );
  }
  return {
    text,
    change,
    clearSent,
    isSaving: saving === key && key !== "",
    isUnavailable: durable.status === "unavailable" || error === key,
    isValid: socialMessageBodySchema.safeParse(text).success,
  };
}

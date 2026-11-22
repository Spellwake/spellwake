import { motion } from "motion/react";
import { ActionButton } from "../button/ActionButton";
import { controlMotion } from "../control-motion";
import { useAllowsControlMotion } from "../useAllowsControlMotion";
import * as s from "./account.css";

export interface ProviderChoice<T extends string> {
  id: T;
  label: string;
  iconSrc: string;
}
export interface ProviderActionsProps<T extends string> {
  providers: readonly ProviderChoice<T>[];
  disabled?: boolean;
  onChoose: (provider: T) => void;
}
/** The authentication host supplies its enabled methods, marks and handoff command. */
export function ProviderActions<T extends string>({
  providers,
  disabled = false,
  onChoose,
}: ProviderActionsProps<T>) {
  const animate = useAllowsControlMotion();
  return (
    <div className={s.providers} role="group" aria-label="Sign-in methods">
      {providers.map((provider) => (
        <ActionButton
          key={provider.id}
          disabled={disabled}
          onClick={() => onChoose(provider.id)}
          className={s.provider}
        >
          <motion.img
            alt=""
            src={provider.iconSrc}
            draggable={false}
            initial={animate ? { opacity: 0.6, y: 1 } : false}
            animate={{ opacity: 1, y: 0 }}
            transition={{
              duration: animate ? controlMotion.feedback : 0,
              ease: controlMotion.ease,
            }}
          />
          {provider.label}
        </ActionButton>
      ))}
    </div>
  );
}

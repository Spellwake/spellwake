import { useState, type ReactElement } from "react";
import {
  ActionButton,
  Dialog,
  FeedbackMessages,
  RadioGroup,
} from "@spellwake/ui";
import type {
  AccountSettingsSource,
  BadgeChoice,
  Portrait as PortraitData,
} from "../../account/account-sources";
import { Portrait } from "../../account/Portrait";
import { useCommand, useNotice } from "./operation";
import * as s from "./account.css";

type Badges = NonNullable<
  Extract<AccountSettingsSource, { status: "ready" }>["badges"]
>;

function Mark(): ReactElement {
  return (
    <svg className={s.badgeMark} viewBox="0 0 24 24">
      <path
        fill="currentColor"
        d="M12 1.5l2 6.2 5.4-3.1-3.1 5.4 6.2 2-6.2 2 3.1 5.4-5.4-3.1-2 6.2-2-6.2-5.4 3.1 3.1-5.4-6.2-2 6.2-2-3.1-5.4L10 7.7z"
      />
    </svg>
  );
}

/**
 * Chooses the badge worn on the picture. The preview shows the choice on the
 * portrait itself. Saving is separate from every other setting, and Save stays
 * off until the choice differs from the saved one.
 */
export function BadgeEditor({
  portrait,
  badge,
  badges,
  onClose,
}: {
  portrait: PortraitData | null;
  badge: BadgeChoice;
  badges: Badges;
  onClose: () => void;
}): ReactElement {
  const [choice, setChoice] = useState<BadgeChoice>(badge);
  const command = useCommand();
  const notice = useNotice();
  // A badge the account no longer qualifies for can still be taken off.
  const offersSupporter = badges.canUseSupporter || badge === "supporter";
  const save = (): void => {
    notice.say("Saving badge…", "working");
    void command
      .run(() => badges.save(choice))
      .then((outcome) => {
        if (outcome?.status === "success") onClose();
        else if (outcome) notice.say(outcome.message);
      });
  };
  return (
    <Dialog
      open
      title="Badge"
      size="compact"
      busy={command.isBusy}
      onClose={onClose}
      actions={
        <>
          <ActionButton
            className={s.action}
            disabled={command.isBusy}
            onClick={onClose}
          >
            Cancel
          </ActionButton>
          <ActionButton
            className={s.action}
            emphasis="primary"
            disabled={choice === badge}
            pending={command.isBusy}
            onClick={save}
          >
            Save
          </ActionButton>
        </>
      }
    >
      <div className={s.badgeBody}>
        <Portrait portrait={portrait} badge={choice} diameter={120} />
        <span className={s.badgeRule} />
        <RadioGroup
          label="Badge"
          value={choice}
          disabled={command.isBusy}
          options={[
            { value: "none", label: "None" },
            ...(offersSupporter
              ? [
                  {
                    value: "supporter",
                    label: "Supporter",
                    decoration: <Mark />,
                    disabled: !badges.canUseSupporter,
                  },
                ]
              : []),
          ]}
          onValueChange={(value) =>
            setChoice(value === "supporter" ? "supporter" : "none")
          }
        />
      </div>
      <div className={s.childNotice}>
        <FeedbackMessages
          label="Badge messages"
          presentation="toast"
          items={notice.items}
        />
      </div>
    </Dialog>
  );
}

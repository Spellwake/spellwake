import { type ReactNode, type ReactElement } from "react";
import { Accordion as RAccordion } from "radix-ui";
import { ChevronRight } from "lucide-react";
import * as s from "../controls.css";

export function Accordion({
  items,
}: {
  items: { id: string; title: string; children: ReactNode }[];
}): ReactElement {
  return (
    <RAccordion.Root type="multiple" className={s.accordion}>
      {items.map((item) => (
        <RAccordion.Item
          key={item.id}
          value={item.id}
          className={s.accordionItem}
        >
          <RAccordion.Header className={s.accordionHeader}>
            <RAccordion.Trigger className={s.accordionTrigger}>
              {item.title}
              <ChevronRight className={s.chevron} size={18} />
            </RAccordion.Trigger>
          </RAccordion.Header>
          <RAccordion.Content className={s.accordionContent}>
            <div className={s.accordionInner}>{item.children}</div>
          </RAccordion.Content>
        </RAccordion.Item>
      ))}
    </RAccordion.Root>
  );
}

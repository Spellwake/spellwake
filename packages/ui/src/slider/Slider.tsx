import { type ReactElement } from "react";
import { Slider as RSlider } from "radix-ui";
import * as s from "../controls.css";

export function Slider({
  label,
  value,
  onValueChange,
}: {
  label: string;
  value: number;
  onValueChange: (value: number) => void;
}): ReactElement {
  return (
    <RSlider.Root
      className={s.sliderRoot}
      value={[value]}
      onValueChange={(values) => {
        const first = values[0];
        if (first !== undefined) onValueChange(first);
      }}
      min={0}
      max={100}
      step={1}
    >
      <RSlider.Track className={s.sliderTrack}>
        <RSlider.Range className={s.sliderRange} />
      </RSlider.Track>
      <RSlider.Thumb aria-label={label} className={s.sliderThumb} />
    </RSlider.Root>
  );
}

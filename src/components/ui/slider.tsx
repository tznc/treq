import * as React from "react";
import { Slider as SliderPrimitive } from "@base-ui/react/slider";
import { cn } from "../../lib/utils";

const Slider = ({
  className,
  ref,
  ...props
}: React.ComponentProps<typeof SliderPrimitive.Root>) => (
  <SliderPrimitive.Root
    ref={ref}
    className={cn(
      "relative flex w-full touch-none select-none items-center",
      className,
    )}
    {...props}
  >
    <SliderPrimitive.Control className="relative flex w-full items-center">
      <SliderPrimitive.Track className="relative h-2 w-full grow rounded-full bg-secondary">
        <SliderPrimitive.Indicator className="absolute h-full rounded-full bg-primary" />
        <SliderPrimitive.Thumb className="block h-5 w-5 rounded-full border-2 border-primary bg-background ring-offset-background transition-colors focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring focus-visible:ring-offset-2 disabled:pointer-events-none disabled:opacity-50" />
      </SliderPrimitive.Track>
    </SliderPrimitive.Control>
  </SliderPrimitive.Root>
);
Slider.displayName = "Slider";

export { Slider };

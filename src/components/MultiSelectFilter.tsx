import { ChevronDown } from "lucide-react";
import { Button } from "./ui/button";
import {
  DropdownMenu,
  DropdownMenuCheckboxItem,
  DropdownMenuContent,
  DropdownMenuTrigger,
} from "./ui/dropdown-menu";

interface MultiSelectOption {
  value: string;
  label: string;
}

interface Props {
  options: MultiSelectOption[];
  /** Selected values; empty means no filter. */
  value: string[];
  onChange: (values: string[]) => void;
  /** Trigger label when nothing is ticked, e.g. "All levels". */
  allLabel: string;
  /** Plural noun for several ticked values, e.g. "levels" gives "2 levels". */
  pluralNoun: string;
  ariaLabel: string;
  testId: string;
  /** Extra classes for the trigger label and each item. */
  labelClassName?: string;
  contentClassName?: string;
}

/** Dropdown of checkbox items. Nothing ticked means no filter. */
export function MultiSelectFilter({
  options,
  value,
  onChange,
  allLabel,
  pluralNoun,
  ariaLabel,
  testId,
  labelClassName,
  contentClassName = "w-40",
}: Props) {
  function toggle(option: string) {
    onChange(
      value.includes(option)
        ? value.filter((v) => v !== option)
        : [...value, option],
    );
  }

  const label =
    value.length === 0
      ? allLabel
      : value.length === 1
        ? (options.find((o) => o.value === value[0])?.label ?? value[0])
        : `${value.length} ${pluralNoun}`;

  return (
    <DropdownMenu>
      <DropdownMenuTrigger asChild>
        <Button
          size="sm"
          variant="outline"
          aria-label={ariaLabel}
          data-testid={testId}
          className="min-w-[130px] justify-between"
        >
          <span className={labelClassName}>{label}</span>
          <ChevronDown className="h-3 w-3 ml-1 opacity-60" />
        </Button>
      </DropdownMenuTrigger>
      <DropdownMenuContent align="start" className={contentClassName}>
        {options.map((option) => (
          <DropdownMenuCheckboxItem
            key={option.value}
            checked={value.includes(option.value)}
            // Radix closes the menu on select by default; multi-select
            // should stay open so several values can be ticked at once.
            onSelect={(event) => event.preventDefault()}
            onCheckedChange={() => toggle(option.value)}
            className={labelClassName}
          >
            {option.label}
          </DropdownMenuCheckboxItem>
        ))}
      </DropdownMenuContent>
    </DropdownMenu>
  );
}

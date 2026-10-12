import { ChevronDown } from "lucide-react";
import { Button } from "./ui/button";
import {
  DropdownMenu,
  DropdownMenuCheckboxItem,
  DropdownMenuContent,
  DropdownMenuTrigger,
} from "./ui/dropdown-menu";

const LOG_LEVELS = ["info", "warning", "error"] as const;

interface Props {
  /** Selected levels; empty means no filter (all levels shown). */
  value: string[];
  onChange: (levels: string[]) => void;
}

/** Multi-select for log levels. Nothing ticked means "All levels". */
export function LogLevelFilter({ value, onChange }: Props) {
  function toggle(level: string) {
    onChange(
      value.includes(level)
        ? value.filter((l) => l !== level)
        : [...value, level],
    );
  }

  const label =
    value.length === 0
      ? "All levels"
      : value.length === 1
        ? value[0]
        : `${value.length} levels`;

  return (
    <DropdownMenu>
      <DropdownMenuTrigger asChild>
        <Button
          size="sm"
          variant="outline"
          aria-label="Filter by level"
          data-testid="log-level-filter"
          className="min-w-[130px] justify-between"
        >
          <span className="capitalize">{label}</span>
          <ChevronDown className="h-3 w-3 ml-1 opacity-60" />
        </Button>
      </DropdownMenuTrigger>
      <DropdownMenuContent align="start" className="w-40">
        {LOG_LEVELS.map((level) => (
          <DropdownMenuCheckboxItem
            key={level}
            checked={value.includes(level)}
            // Radix closes the menu on select by default; multi-select
            // should stay open so several levels can be ticked at once.
            onSelect={(event) => event.preventDefault()}
            onCheckedChange={() => toggle(level)}
            className="capitalize"
          >
            {level}
          </DropdownMenuCheckboxItem>
        ))}
      </DropdownMenuContent>
    </DropdownMenu>
  );
}

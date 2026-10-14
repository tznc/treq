import { MultiSelectFilter } from "./MultiSelectFilter";

const LOG_LEVELS = ["info", "warning", "error"].map((level) => ({
  value: level,
  label: level,
}));

interface Props {
  /** Selected levels; empty means no filter (all levels shown). */
  value: string[];
  onChange: (levels: string[]) => void;
}

/** Multi-select for log levels. Nothing ticked means "All levels". */
export function LogLevelFilter({ value, onChange }: Props) {
  return (
    <MultiSelectFilter
      options={LOG_LEVELS}
      value={value}
      onChange={onChange}
      allLabel="All levels"
      pluralNoun="levels"
      ariaLabel="Filter by level"
      testId="log-level-filter"
      labelClassName="capitalize"
    />
  );
}

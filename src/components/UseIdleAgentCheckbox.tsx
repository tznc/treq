interface UseIdleAgentCheckboxProps {
  checked: boolean;
  onCheckedChange: (checked: boolean) => void;
  disabled?: boolean;
}

/**
 * Opt-in for sending a finished review to an idle agent terminal. Unchecked,
 * the review opens a new agent terminal with fresh context.
 */
export function UseIdleAgentCheckbox({
  checked,
  onCheckedChange,
  disabled,
}: UseIdleAgentCheckboxProps) {
  return (
    <label className="flex items-center gap-1.5 text-xs text-muted-foreground cursor-pointer">
      <input
        type="checkbox"
        className="size-3.5 accent-primary"
        data-testid="use-idle-agent"
        checked={checked}
        disabled={disabled}
        onChange={(event) => onCheckedChange(event.target.checked)}
      />
      Send to idle agent if one is available
    </label>
  );
}

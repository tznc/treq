interface CmdkFooterProps {
  actions?: Array<{
    key: string;
    label: string;
  }>;
}

export const CmdkFooter: React.FC<CmdkFooterProps> = ({ actions }) => {
  const defaultActions = [
    { key: "↑↓", label: "Navigate" },
    { key: "↵", label: "Select" },
    { key: "Esc", label: "Close" },
  ];

  const displayActions = actions || defaultActions;

  return (
    <div className="border-t border-border px-3 py-2 flex items-center justify-between text-sm text-muted-foreground">
      <div className="flex items-center gap-3">
        {displayActions.map((action, index) => (
          <span key={index}>
            <kbd className="px-1.5 py-0.5 bg-muted rounded text-xs">
              {action.key}
            </kbd>{" "}
            {action.label}
          </span>
        ))}
      </div>
    </div>
  );
};

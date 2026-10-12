import { useEffect, useState } from "react";
import { RotateCw } from "lucide-react";
import { Button } from "../ui/button";
import { Input } from "../ui/input";
import { cn } from "../../lib/utils";
import { isAllowedBrowserUrl } from "./utils";

export interface AddressBarProps {
  url: string;
  onNavigate: (url: string) => void;
  onReload: () => void;
  disabled?: boolean;
}

export function AddressBar({
  url,
  onNavigate,
  onReload,
  disabled,
}: AddressBarProps) {
  const [draft, setDraft] = useState(url);
  const isValid =
    draft.trim().length === 0 || isAllowedBrowserUrl(draft.trim());

  useEffect(() => {
    setDraft(url);
  }, [url]);

  const handleNavigate = () => {
    const trimmed = draft.trim();
    if (!trimmed || !isAllowedBrowserUrl(trimmed)) return;
    onNavigate(trimmed);
  };

  return (
    <div className="flex items-center gap-2 flex-1 min-w-0">
      <div className="relative flex-1 min-w-0">
        <Input
          value={draft}
          onChange={(e) => setDraft(e.target.value)}
          onKeyDown={(e) => {
            if (e.key === "Enter") {
              e.preventDefault();
              handleNavigate();
            }
          }}
          placeholder="http://localhost:3000 or file:///path/to/index.html"
          aria-label="Browser URL"
          className={cn(
            "pr-8",
            !isValid && "border-destructive focus-visible:ring-destructive",
          )}
          disabled={disabled}
        />
        <Button
          type="button"
          variant="ghost"
          size="icon-xs"
          aria-label="Reload page"
          onClick={onReload}
          disabled={disabled}
          className="absolute right-1 top-1/2 -translate-y-1/2"
        >
          <RotateCw className="w-3.5 h-3.5" />
        </Button>
      </div>
      {!isValid && draft.trim().length > 0 && (
        <span className="text-xs text-destructive whitespace-nowrap truncate">
          Only http://localhost, http://127.0.0.1 and file:// URLs are supported
        </span>
      )}
    </div>
  );
}

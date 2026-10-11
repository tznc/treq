import {
  type ChangeEvent,
  type KeyboardEvent as ReactKeyboardEvent,
  useImperativeHandle,
  useRef,
  useState,
} from "react";
import { ChevronDown, Loader2 } from "lucide-react";
import { Button } from "../ui/button";
import { Textarea } from "../ui/textarea";
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuItem,
  DropdownMenuTrigger,
} from "../ui/dropdown-menu";
import { cn } from "../../lib/utils";
import type { CommitInputProps } from "./types";

const draftStorageKey = (workspacePath: string) =>
  `treq.commitDraft.${workspacePath}`;

const readDraft = (workspacePath: string): string => {
  try {
    return sessionStorage.getItem(draftStorageKey(workspacePath)) ?? "";
  } catch {
    return "";
  }
};

const writeDraft = (workspacePath: string, message: string) => {
  try {
    if (message) {
      sessionStorage.setItem(draftStorageKey(workspacePath), message);
    } else {
      sessionStorage.removeItem(draftStorageKey(workspacePath));
    }
  } catch {
    // ignore storage failures (e.g. disabled/full storage)
  }
};

const CommitInput = ({
  onCommit,
  onCommitAndPush,
  onCommitAndCreatePR,
  disabled,
  pending,
  pendingAction,
  canCreatePr = false,
  hasPr = false,
  selectedFileCount = 0,
  workspacePath,
  ref,
}: CommitInputProps) => {
  const [message, setMessage] = useState(() => readDraft(workspacePath));
  const [error, setError] = useState<string | null>(null);
  const textareaRef = useRef<HTMLTextAreaElement>(null);

  const updateMessage = (value: string) => {
    setMessage(value);
    writeDraft(workspacePath, value);
  };

  useImperativeHandle(
    ref,
    () => ({
      focus: () => {
        requestAnimationFrame(() => {
          if (textareaRef.current) {
            textareaRef.current.focus();
            textareaRef.current.select();
          }
        });
      },
    }),
    [],
  );

  const runAction = (action: (message: string) => void) => {
    const trimmed = message.trim();
    if (!trimmed) {
      setError("Enter a commit message.");
      requestAnimationFrame(() => {
        textareaRef.current?.focus();
      });
      return;
    }
    setError(null);
    action(trimmed);
    updateMessage("");
  };

  const handleMessageChange = (event: ChangeEvent<HTMLTextAreaElement>) => {
    updateMessage(event.target.value);
    if (error) {
      setError(null);
    }
  };

  const handleKeyDown = (event: ReactKeyboardEvent<HTMLTextAreaElement>) => {
    if (event.metaKey || event.ctrlKey) {
      const key = event.key.toLowerCase();
      if (["a", "c", "x", "v", "z", "y"].includes(key)) {
        // Stop propagation so global shortcuts (e.g. select-all-files)
        // don't hijack standard text editing in this input.
        event.stopPropagation();
      }
    }

    if ((event.metaKey || event.ctrlKey) && event.key === "Enter") {
      event.preventDefault();
      runAction(onCommit);
    }
  };

  const commitLabel =
    selectedFileCount > 0
      ? `Commit ${selectedFileCount} file${selectedFileCount !== 1 ? "s" : ""}`
      : "Commit";

  const pendingLabel =
    pendingAction === "push"
      ? "Pushing…"
      : pendingAction === "pr"
        ? "Creating PR…"
        : "Committing…";

  return (
    <div className="px-4 py-3 border-b border-border space-y-2">
      <Textarea
        ref={textareaRef}
        placeholder="Message"
        value={message}
        onChange={handleMessageChange}
        onKeyDown={handleKeyDown}
        disabled={disabled || pending}
        aria-invalid={error ? true : undefined}
        aria-describedby={error ? "commit-message-error" : undefined}
        className={cn(
          "resize-none overflow-hidden",
          error && "border-destructive focus-visible:ring-destructive",
        )}
        style={{ minHeight: "24px" }}
      />
      {error && (
        <p
          id="commit-message-error"
          className="text-sm text-destructive"
          role="alert"
        >
          {error}
        </p>
      )}
      <div className="flex w-full">
        <Button
          className="flex-1 text-sm !h-auto py-1.5 rounded-r-none gap-1.5"
          disabled={disabled || pending}
          onClick={() => runAction(onCommit)}
          size="sm"
        >
          {pending ? (
            <>
              <Loader2 className="w-4 h-4 animate-spin" />
              {pendingLabel}
            </>
          ) : (
            commitLabel
          )}
        </Button>
        <DropdownMenu>
          <DropdownMenuTrigger asChild>
            <Button
              className="!h-auto py-1.5 px-1.5 rounded-l-none border-l border-primary-foreground/20"
              disabled={disabled || pending}
              size="sm"
              aria-label="More commit options"
            >
              <ChevronDown className="w-4 h-4" />
            </Button>
          </DropdownMenuTrigger>
          <DropdownMenuContent align="end" sideOffset={4}>
            <DropdownMenuItem
              disabled={disabled || pending}
              onSelect={() => runAction(onCommitAndPush)}
            >
              Commit and push
            </DropdownMenuItem>
            {!hasPr && (
              <DropdownMenuItem
                disabled={disabled || pending || !canCreatePr}
                onSelect={() => runAction(onCommitAndCreatePR)}
              >
                Commit and create PR
              </DropdownMenuItem>
            )}
          </DropdownMenuContent>
        </DropdownMenu>
      </div>
    </div>
  );
};
CommitInput.displayName = "CommitInput";

export { CommitInput };

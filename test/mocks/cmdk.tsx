import * as React from "react";

interface CommandProps extends React.HTMLAttributes<HTMLDivElement> {
  children?: React.ReactNode;
}

interface CommandItemProps extends React.HTMLAttributes<HTMLDivElement> {
  value?: string;
  onSelect?: () => void;
  children?: React.ReactNode;
}

interface CommandDialogProps {
  open?: boolean;
  onOpenChange?: (open: boolean) => void;
  children?: React.ReactNode;
  className?: string;
}

interface CommandGroupProps extends React.HTMLAttributes<HTMLDivElement> {
  heading?: string;
  children?: React.ReactNode;
}

const CommandRoot: React.FC<CommandProps> = ({
  children,
  className,
  ...props
}) => (
  <div className={className} {...props}>
    {children}
  </div>
);

interface CommandInputProps
  extends Omit<React.InputHTMLAttributes<HTMLInputElement>, "onChange"> {
  onValueChange?: (value: string) => void;
}

const Input: React.FC<CommandInputProps> = ({ onValueChange, ...props }) => (
  <input {...props} onChange={(e) => onValueChange?.(e.target.value)} />
);

const List: React.FC<React.HTMLAttributes<HTMLDivElement>> = ({
  children,
  className,
  ...props
}) => (
  <div className={className} {...props}>
    {children}
  </div>
);

const Empty: React.FC<React.HTMLAttributes<HTMLDivElement>> = ({
  children,
  ...props
}) => <div {...props}>{children}</div>;

const Item: React.FC<CommandItemProps> = ({
  children,
  className,
  onSelect,
  value: _value,
  ...props
}) => (
  <div
    className={className}
    onClick={() => onSelect?.()}
    role="option"
    {...props}
  >
    {children}
  </div>
);

const Dialog: React.FC<CommandDialogProps> = ({
  open,
  onOpenChange,
  children,
  className,
}) => {
  if (!open) return null;
  return (
    <div className={className}>
      <div cmdk-overlay="" onClick={() => onOpenChange?.(false)} />
      <div cmdk-dialog="">{children}</div>
    </div>
  );
};

const Group: React.FC<CommandGroupProps> = ({
  heading,
  children,
  className,
  ...props
}) => (
  <div className={className} {...props}>
    {heading && <div>{heading}</div>}
    {children}
  </div>
);

export const Command = Object.assign(CommandRoot, {
  Input,
  List,
  Empty,
  Item,
  Dialog,
  Group,
});

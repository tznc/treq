import {
  capabilityNoticeMessages,
  type RemoteCapabilities,
} from "../../lib/remote-capabilities";

export function RemoteCapabilityNotice({
  capabilities,
}: {
  capabilities: RemoteCapabilities;
}) {
  const messages = capabilityNoticeMessages(capabilities);

  if (messages.length === 0) return null;

  return (
    <div
      data-testid="remote-capability-notice"
      className="border-b px-4 py-1.5 text-xs text-muted-foreground"
    >
      {messages.map((message) => (
        <p key={message}>{message}</p>
      ))}
    </div>
  );
}

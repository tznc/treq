import { convertFileSrc } from "@tauri-apps/api/core";

export type TreqSendMediaType = "image" | "text" | "browser";

export interface TreqSendPayload {
  kind: "send";
  request_id: string;
  repo: string;
  pty_session_id?: string | null;
  media_type: TreqSendMediaType | string;
  path: string;
  title?: string | null;
}

export interface TreqSendAsset {
  id: string;
  repo: string;
  ptySessionId: string | null;
  mediaType: TreqSendMediaType;
  path: string;
  title: string;
  receivedAt: number;
}

export const TREQ_SEND_EVENT = "treq-send-received";

export function normalizeSendMediaType(
  value: string | undefined | null,
): TreqSendMediaType {
  if (value === "image") return "image";
  if (value === "browser") return "browser";
  return "text";
}

export function parseTreqSendPayload(payload: unknown): TreqSendAsset | null {
  if (!payload || typeof payload !== "object") return null;
  const raw = payload as Partial<TreqSendPayload>;
  if (typeof raw.request_id !== "string" || !raw.request_id) return null;
  if (typeof raw.path !== "string" || !raw.path) return null;
  if (typeof raw.repo !== "string") return null;

  const title =
    (typeof raw.title === "string" && raw.title.trim()) ||
    raw.path.split(/[/\\]/).pop() ||
    raw.path;

  return {
    id: raw.request_id,
    repo: raw.repo,
    ptySessionId:
      typeof raw.pty_session_id === "string" && raw.pty_session_id.trim()
        ? raw.pty_session_id
        : null,
    mediaType: normalizeSendMediaType(raw.media_type),
    path: raw.path,
    title,
    receivedAt: Date.now(),
  };
}

/** Convert a local filesystem path into a WebView-loadable URL. */
export function treqSendFileSrc(path: string): string {
  try {
    return convertFileSrc(path);
  } catch {
    return path;
  }
}

export function assetsForPtySession(
  assets: TreqSendAsset[],
  ptySessionId: string,
  isActive: boolean,
): TreqSendAsset[] {
  return assets.filter(
    (asset) =>
      asset.ptySessionId === ptySessionId ||
      (asset.ptySessionId == null && isActive),
  );
}

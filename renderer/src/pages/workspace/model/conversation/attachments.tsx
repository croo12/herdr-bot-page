/** Extracts trailing (attached: <path>) lines from the message text for attachment previews. */

const ATTACHMENT_LINE_RE = /^\(attached: (.+)\)$/;

export interface SplitAttachments {
  /** The message text with every trailing `(attached: ...)` line (and the blank line that separates them
   * from the real content, per `withAttachments`) removed. */
  readonly text: string;
  readonly attachmentPaths: readonly string[];
}

/** Strips trailing `(attached: <path>)` lines from `rawText`. Only looks at a trailing run of such lines
 * (plus the blank separator line before them) so a message that happens to contain that literal text
 * mid-paragraph is left alone. */
export function splitAttachments(rawText: string): SplitAttachments {
  const lines = rawText.split("\n");
  const paths: string[] = [];
  let end = lines.length;
  while (end > 0) {
    const line = lines[end - 1]!;
    const match = ATTACHMENT_LINE_RE.exec(line);
    if (match == null) break;
    paths.unshift(match[1]!);
    end -= 1;
  }
  if (paths.length === 0) return { text: rawText, attachmentPaths: [] };
  // Drop the blank separator line `withAttachments` always inserts between content and the attachment list.
  if (end > 0 && lines[end - 1] === "") end -= 1;
  return { text: lines.slice(0, end).join("\n"), attachmentPaths: paths };
}

export type AttachmentKind = "image" | "video" | "audio" | "file";

const EXTENSION_KIND: Record<string, AttachmentKind> = {
  png: "image",
  jpg: "image",
  jpeg: "image",
  gif: "image",
  webp: "image",
  svg: "image",
  bmp: "image",
  mp4: "video",
  mov: "video",
  webm: "video",
  mp3: "audio",
  wav: "audio",
  m4a: "audio",
  ogg: "audio",
};

/** Guesses the attachment preview kind from its path's extension -- the host never computes/stores a
 * mime type or dimensions, so extension
 * sniffing is the only signal available client-side. */
export function inferAttachmentKind(path: string): AttachmentKind {
  const match = /\.([a-zA-Z0-9]+)$/.exec(path);
  const ext = match?.[1]?.toLowerCase();
  return (ext != null && EXTENSION_KIND[ext]) || "file";
}

/** Extracts a filename from a Windows or POSIX attachment path. */
export function attachmentFileName(path: string): string {
  const index = Math.max(path.lastIndexOf("/"), path.lastIndexOf("\\"));
  return index === -1 ? path : path.slice(index + 1);
}

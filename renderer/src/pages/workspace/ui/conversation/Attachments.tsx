import { useEffect, useState } from "react";
import { Download, File, FileAudio, FileVideo } from "lucide-react";
import { useBridge } from "#shared/api/desktop/BridgeContext.tsx";
import { useT } from "#shared/i18n/TranslationProvider.tsx";
import { attachmentFileName, inferAttachmentKind } from "../../model/conversation/attachments.tsx";
import styles from "./Attachments.module.css";

export interface AttachmentsProps {
  readonly paths: readonly string[];
}

/** Attachment previews/chips below a message's text. There is no
 * attachment metadata on the entry at all -- paths come from `attachments.tsx#splitAttachments` parsing
 * them back out of the message text -- and `resolveMedia` returns `width:null,height:null` always (no
 * computed dimensions), so an image/video preview is sized by CSS, never by the source data. */
export function Attachments({ paths }: AttachmentsProps) {
  if (paths.length === 0) return null;
  return (
    <div className={styles["hb-attachments"]}>
      {paths.map((path) => (
        <AttachmentItem key={path} path={path} />
      ))}
    </div>
  );
}

interface ResolvedMedia {
  readonly kind: "image" | "video" | "audio";
  readonly src: string;
}

/** Normalizes the host's `{kind, dataUrl|src, width:null, height:null}` shape from `attachments.resolveMedia` -- `unknown` at the type level since the bridge interface
 * doesn't narrow it further, so this is the one place that validates it before trusting a `src`. */
function parseResolvedMedia(value: unknown): ResolvedMedia | null {
  if (typeof value !== "object" || value === null) return null;
  const record = value as Record<string, unknown>;
  const kind = record.kind;
  if (kind !== "image" && kind !== "video" && kind !== "audio") return null;
  const src =
    typeof record.dataUrl === "string"
      ? record.dataUrl
      : typeof record.src === "string"
        ? record.src
        : null;
  return src == null ? null : { kind, src };
}

function AttachmentItem({ path }: { readonly path: string }) {
  const bridge = useBridge();
  const { t } = useT();
  const kind = inferAttachmentKind(path);
  const fileName = attachmentFileName(path);
  const [media, setMedia] = useState<ResolvedMedia | null>(null);

  useEffect(() => {
    let cancelled = false;
    if (kind === "file") return;
    void bridge.attachments.resolveMedia(path).then((result) => {
      const parsed = parseResolvedMedia(result);
      if (!cancelled && parsed != null) setMedia(parsed);
    });
    return () => {
      cancelled = true;
    };
  }, [bridge, path, kind]);

  function download() {
    void bridge.attachments.download(path, fileName);
  }

  if (media?.kind === "image") {
    return (
      <button
        type="button"
        className={styles["hb-attachment-image"]}
        onClick={download}
        aria-label={t("conversation.attachment.download", { fileName })}
      >
        <img src={media.src} alt={fileName} />
      </button>
    );
  }
  if (media?.kind === "video") {
    return (
      <div className={styles["hb-attachment-image"]}>
        {/* oxlint-disable-next-line jsx-a11y/media-has-caption -- This preview displays a user-supplied attachment without a separate caption file. */}
        <video src={media.src} controls />
      </div>
    );
  }
  if (media?.kind === "audio") {
    return (
      <div className={styles["hb-attachment-chip"]}>
        <FileAudio size={16} aria-hidden />
        {/* oxlint-disable-next-line jsx-a11y/media-has-caption -- This preview displays a user-supplied attachment without a separate caption file. */}
        <audio src={media.src} controls className={styles["hb-attachment-audio"]} />
      </div>
    );
  }

  const Icon = kind === "video" ? FileVideo : kind === "audio" ? FileAudio : File;
  return (
    <button
      type="button"
      className={styles["hb-attachment-chip"]}
      onClick={download}
      aria-label={t("conversation.attachment.download", { fileName })}
    >
      <Icon size={16} aria-hidden />
      <span className={styles["hb-attachment-name"]}>{fileName}</span>
      <Download size={13} aria-hidden />
    </button>
  );
}

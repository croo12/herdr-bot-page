/** Mention chips follow the current contact name and color, including in restored drafts. */
import { createContext, useContext, type CSSProperties } from "react";
import { Mention } from "@tiptap/extension-mention";
import { NodeViewWrapper, ReactNodeViewRenderer } from "@tiptap/react";
import type { NodeViewProps } from "@tiptap/react";
import { Users } from "lucide-react";
import { personaColorVar, resolvePersonaColor } from "#shared/ui/avatar-colors.tsx";
import type { MentionMember } from "../model/mention-candidates.tsx";
import { Avatar } from "#shared/ui/Avatar.tsx";
import styles from "./MentionChip.module.css";

export const MentionContacts = createContext<readonly MentionMember[]>([]);

function MentionChipView({ node }: NodeViewProps) {
  const attrs = node.attrs as { readonly id?: unknown; readonly label?: unknown; readonly everyone?: unknown };
  const id = typeof attrs.id === "string" ? attrs.id : "";
  const storedLabel = typeof attrs.label === "string" ? attrs.label : "";
  const everyone = attrs.everyone === true;
  const contacts = useContext(MentionContacts);
  const chat = everyone ? undefined : contacts.find((contact) => contact.id === id);
  const displayName = everyone ? "everyone" : (chat?.name ?? storedLabel);
  const hue = chat == null ? undefined : ({ "--hb-mention-hue": personaColorVar(resolvePersonaColor(chat.avatarColor)) } as CSSProperties);

  return (
    <NodeViewWrapper as="span" className={styles["hb-mention-chip"]} style={hue} contentEditable={false} data-testid="composer-mention-chip">
      <span className={styles["hb-mention-chip-icon"]} aria-hidden>
        {everyone ? <Users size={14} /> : <Avatar kind="bot" size="2xs" color={resolvePersonaColor(chat?.avatarColor)} name={displayName} />}
      </span>
      <span>{everyone ? "everyone" : displayName}</span>
    </NodeViewWrapper>
  );
}

export const ComposerMention = Mention.extend({
  addAttributes() {
    return {
      ...this.parent?.(),
      everyone: {
        default: false,
        parseHTML: (element: HTMLElement) => element.getAttribute("data-everyone") === "true",
        renderHTML: (attributes: { readonly everyone?: boolean }) => (attributes.everyone ? { "data-everyone": "true" } : {}),
      },
    };
  },
  addNodeView() {
    return ReactNodeViewRenderer(MentionChipView);
  },
});

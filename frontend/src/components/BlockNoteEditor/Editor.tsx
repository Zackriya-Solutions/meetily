"use client";

import { useEffect } from "react";
import { PartialBlock, Block } from "@blocknote/core";
import "@blocknote/shadcn/style.css";
import "@blocknote/core/fonts/inter.css";
import { useConfig } from "@/contexts/ConfigContext";

interface EditorProps {
  initialContent?: Block[];
  onChange?: (blocks: Block[]) => void;
  editable?: boolean;
}

export default function Editor({ initialContent, onChange, editable = true }: EditorProps) {
  const { resolvedTheme } = useConfig();

  // Lazy import to avoid SSR issues
  const { useCreateBlockNote } = require("@blocknote/react");
  const { BlockNoteView } = require("@blocknote/shadcn");

  const editor = useCreateBlockNote({
    initialContent: initialContent as PartialBlock[] | undefined,
  });

  // Expose blocksToMarkdown method
  (editor as any).blocksToMarkdownLossy = async (blocks: Block[]) => {
    try {
      return await editor.blocksToMarkdownLossy(blocks);
    } catch {
      return '';
    }
  };

  // Handle content changes
  useEffect(() => {
    if (!onChange) return;

    const handleChange = () => {
      onChange(editor.document);
    };

    const unsubscribe = editor.onChange(handleChange);

    return () => {
      if (typeof unsubscribe === 'function') {
        unsubscribe();
      }
    };
  }, [editor, onChange]);

  return <BlockNoteView editor={editor} editable={editable} theme={resolvedTheme} />;
}

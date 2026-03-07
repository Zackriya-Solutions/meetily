"use client";

import { useEffect } from "react";
import { PartialBlock, Block } from "@blocknote/core";
import "@blocknote/shadcn/style.css";
import "@blocknote/core/fonts/inter.css";
import { useTheme } from "@/contexts/ThemeContext";

interface EditorProps {
  initialContent?: Block[];
  onChange?: (blocks: Block[]) => void;
  editable?: boolean;
}

export default function Editor({ initialContent, onChange, editable = true }: EditorProps) {
  console.log('📝 EDITOR: Initializing BlockNote editor with blocks:', {
    hasContent: !!initialContent,
    blocksCount: initialContent?.length || 0,
    editable
  });

  const { theme } = useTheme();

  // Lazy import to avoid SSR issues
  const { useCreateBlockNote } = require("@blocknote/react");
  const { BlockNoteView } = require("@blocknote/shadcn");

  const editor = useCreateBlockNote({
    initialContent: initialContent as PartialBlock[] | undefined,
  });

  console.log('📝 EDITOR: BlockNote editor created successfully');

  // Expose blocksToMarkdown method
  (editor as any).blocksToMarkdownLossy = async (blocks: Block[]) => {
    try {
      return await editor.blocksToMarkdownLossy(blocks);
    } catch (error) {
      console.error('❌ EDITOR: Failed to convert blocks to markdown:', error);
      return '';
    }
  };

  // Handle content changes
  useEffect(() => {
    if (!onChange) return;

    const handleChange = () => {
      console.log('📝 EDITOR: Content changed, notifying parent...', {
        blocksCount: editor.document.length
      });
      onChange(editor.document);
    };

    const unsubscribe = editor.onChange(handleChange);

    return () => {
      if (typeof unsubscribe === 'function') {
        console.log('📝 EDITOR: Cleaning up onChange listener');
        unsubscribe();
      }
    };
  }, [editor, onChange]);

  return <BlockNoteView editor={editor} editable={editable} theme={theme} />;
}

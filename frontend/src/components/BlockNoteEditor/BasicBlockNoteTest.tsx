import { editorDictionary } from '@/i18n/editor';
import { uiText, useUiTranslation } from '@/i18n/ui';
import "@blocknote/core/fonts/inter.css";
import { useCreateBlockNote } from "@blocknote/react";
import { BlockNoteView } from "@blocknote/shadcn";
import "@blocknote/shadcn/style.css";
import { ChangeEvent, useCallback, useEffect } from "react";

const initialMarkdown = "Hello, **world!**";

export default function BasicBlockNoteTest() {
  useUiTranslation();
  // Creates a new editor instance.
  const editor = useCreateBlockNote({
    dictionary: editorDictionary,});

  const markdownInputChanged = useCallback(
    async (e: ChangeEvent<HTMLTextAreaElement>) => {
      // Whenever the current Markdown content changes, converts it to an array of
      // Block objects and replaces the editor's content with them.
      const blocks = await editor.tryParseMarkdownToBlocks(e.target.value);
      editor.replaceBlocks(editor.document, blocks);
    },
    [editor],
  );

  // For initialization; on mount, convert the initial Markdown to blocks and replace the default editor's content
  useEffect(() => {
    async function loadInitialHTML() {
      const blocks = await editor.tryParseMarkdownToBlocks(initialMarkdown);
      editor.replaceBlocks(editor.document, blocks);
    }
    loadInitialHTML();
  }, [editor]);

  // Renders the Markdown input and editor instance.
  return (
    <div className="views">
      <div className="view-wrapper">
        <div className="view-label">{uiText("messages.markdownInput")}</div>
        <div className="view">
          <code>
            <textarea
              defaultValue={initialMarkdown}
              onChange={markdownInputChanged}
            />
          </code>
        </div>
      </div>
      <div className="view-wrapper">
        <div className="view-label">{uiText("messages.editorOutput")}</div>
        <div className="view">
          <BlockNoteView editor={editor} editable={true} />
        </div>
      </div>
    </div>
  );
}

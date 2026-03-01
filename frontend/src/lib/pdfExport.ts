import jsPDF from 'jspdf';
import { Lexer, type Token, type Tokens } from 'marked';

export interface BrandTemplate {
  name: string;
  primaryColor: string;
  secondaryColor: string;
  fontFamily: string;
}

const PAGE_MARGIN = 15;
const PAGE_WIDTH = 210; // A4 mm
const CONTENT_WIDTH = PAGE_WIDTH - PAGE_MARGIN * 2;
const PAGE_HEIGHT = 297; // A4 mm
const FOOTER_HEIGHT = 15;
const CONTENT_BOTTOM = PAGE_HEIGHT - FOOTER_HEIGHT;

export async function generatePdfFromMarkdown(
  markdownContent: string,
  title: string,
  date: string,
  brand?: BrandTemplate,
  logo?: Uint8Array
): Promise<Uint8Array> {
  const doc = new jsPDF({ orientation: 'portrait', unit: 'mm', format: 'a4' });

  const primaryColor = brand?.primaryColor ? hexToRgb(brand.primaryColor) : ([44, 62, 80] as [number, number, number]);
  const secondaryColor = brand?.secondaryColor ? hexToRgb(brand.secondaryColor) : ([100, 116, 139] as [number, number, number]);
  const fontFamily = brand?.fontFamily || 'helvetica';

  // ── helpers ──────────────────────────────────────────────────────────────
  let y = PAGE_MARGIN;

  function checkPage(needed = 6) {
    if (y + needed > CONTENT_BOTTOM) {
      doc.addPage();
      y = PAGE_MARGIN;
    }
  }

  function setColor(color: [number, number, number]) {
    doc.setTextColor(color[0], color[1], color[2]);
  }

  /** Render a single line that may contain **bold** and *italic* spans */
  function renderInline(rawText: string, xStart: number, fontSize: number, indent = 0): number {
    doc.setFontSize(fontSize);
    // Tokenise inline markdown: **bold**, *italic*, `code`, plain
    const segments: { text: string; bold: boolean; italic: boolean; code: boolean }[] = [];
    const regex = /(\*\*(.+?)\*\*|\*(.+?)\*|`(.+?)`|([^*`]+))/g;
    let m: RegExpExecArray | null;
    while ((m = regex.exec(rawText)) !== null) {
      if (m[2] !== undefined)      segments.push({ text: m[2], bold: true,  italic: false, code: false });
      else if (m[3] !== undefined) segments.push({ text: m[3], bold: false, italic: true,  code: false });
      else if (m[4] !== undefined) segments.push({ text: m[4], bold: false, italic: false, code: true  });
      else if (m[5] !== undefined) segments.push({ text: m[5], bold: false, italic: false, code: false });
    }

    // Word-wrap across the full content width
    const maxW = CONTENT_WIDTH - (xStart - PAGE_MARGIN) - indent;
    let lineBuffer: { text: string; bold: boolean; italic: boolean; code: boolean }[] = [];
    let lineWidth = 0;
    const spaceW = doc.getStringUnitWidth(' ') * fontSize / doc.internal.scaleFactor;

    function flushLine(isLast: boolean) {
      if (lineBuffer.length === 0) return;
      checkPage(fontSize * 0.352 + 1);
      let x = xStart + indent;
      for (const seg of lineBuffer) {
        const style = seg.bold && seg.italic ? 'bolditalic' : seg.bold ? 'bold' : seg.italic ? 'italic' : 'normal';
        doc.setFont(fontFamily, style);
        if (seg.code) {
          doc.setFont('courier', 'normal');
          doc.setFillColor(240, 240, 240);
        }
        setColor(seg.code ? [80, 80, 80] : [0, 0, 0]);
        doc.text(seg.text, x, y);
        x += doc.getStringUnitWidth(seg.text) * fontSize / doc.internal.scaleFactor;
      }
      doc.setFont(fontFamily, 'normal');
      setColor([0, 0, 0]);
      if (!isLast) { y += fontSize * 0.352 + 1.5; lineBuffer = []; lineWidth = 0; }
    }

    for (const seg of segments) {
      const words = seg.text.split(/( )/);
      for (const word of words) {
        if (word === '') continue;
        const style = seg.bold && seg.italic ? 'bolditalic' : seg.bold ? 'bold' : seg.italic ? 'italic' : 'normal';
        doc.setFont(seg.code ? 'courier' : fontFamily, seg.code ? 'normal' : style);
        const ww = doc.getStringUnitWidth(word) * fontSize / doc.internal.scaleFactor;
        if (lineWidth + ww > maxW && lineBuffer.length > 0) {
          flushLine(false);
        }
        // Merge consecutive same-style segments on the same line
        const last = lineBuffer[lineBuffer.length - 1];
        if (last && last.bold === seg.bold && last.italic === seg.italic && last.code === seg.code) {
          last.text += word;
        } else {
          lineBuffer.push({ ...seg, text: word });
        }
        lineWidth += ww + (word === ' ' ? spaceW : 0);
      }
    }
    flushLine(true);
    y += fontSize * 0.352 + 1.5;
    doc.setFont(fontFamily, 'normal');
    return y;
  }

  // ── header ───────────────────────────────────────────────────────────────
  if (logo) {
    try {
      const logoDataUrl = await uint8ArrayToDataUrl(logo);
      doc.addImage(logoDataUrl, 'PNG', PAGE_MARGIN, PAGE_MARGIN, 25, 25);
      y = PAGE_MARGIN + 30;
    } catch { /* ignore */ }
  }

  // Title
  doc.setFont(fontFamily, 'bold');
  doc.setFontSize(22);
  setColor(primaryColor);
  const titleLines: string[] = doc.splitTextToSize(title, CONTENT_WIDTH);
  doc.text(titleLines, PAGE_MARGIN, y);
  y += titleLines.length * 8 + 2;

  // Date line
  doc.setFont(fontFamily, 'normal');
  doc.setFontSize(9);
  setColor(secondaryColor);
  doc.text(`Generated: ${new Date().toLocaleString('en-US', { year: 'numeric', month: 'long', day: 'numeric', hour: '2-digit', minute: '2-digit' })}`, PAGE_MARGIN, y);
  y += 5;

  // Separator
  doc.setDrawColor(primaryColor[0], primaryColor[1], primaryColor[2]);
  doc.setLineWidth(0.4);
  doc.line(PAGE_MARGIN, y, PAGE_WIDTH - PAGE_MARGIN, y);
  y += 6;

  // ── render markdown tokens ───────────────────────────────────────────────
  function renderTokens(tokens: Token[], listDepth = 0) {
    for (const token of tokens) {
      switch (token.type) {
        case 'heading': {
          const t = token as Tokens.Heading;
          const sizes: Record<number, number> = { 1: 18, 2: 14, 3: 12, 4: 11 };
          const sz = sizes[t.depth] ?? 11;
          checkPage(sz * 0.352 + 6);
          y += t.depth <= 2 ? 4 : 2;
          doc.setFont(fontFamily, 'bold');
          doc.setFontSize(sz);
          setColor(t.depth <= 2 ? primaryColor : secondaryColor);
          const headingText = stripInlineMarkdown(t.text);
          const hLines: string[] = doc.splitTextToSize(headingText, CONTENT_WIDTH);
          doc.text(hLines, PAGE_MARGIN, y);
          y += hLines.length * (sz * 0.352 + 1) + (t.depth <= 2 ? 3 : 2);
          doc.setFont(fontFamily, 'normal');
          setColor([0, 0, 0]);
          // Underline for h1/h2
          if (t.depth <= 2) {
            doc.setDrawColor(secondaryColor[0], secondaryColor[1], secondaryColor[2]);
            doc.setLineWidth(0.2);
            doc.line(PAGE_MARGIN, y - 1, PAGE_WIDTH - PAGE_MARGIN, y - 1);
            y += 1;
          }
          break;
        }

        case 'paragraph': {
          const t = token as Tokens.Paragraph;
          checkPage(6);
          setColor([0, 0, 0]);
          renderInline(t.text, PAGE_MARGIN, 10.5);
          y += 1;
          break;
        }

        case 'list': {
          const t = token as Tokens.List;
          let itemIndex = 1;
          for (const item of t.items) {
            const bullet = t.ordered ? `${itemIndex++}.` : '•';
            const indent = listDepth * 5;
            const bulletX = PAGE_MARGIN + indent;
            const textX = bulletX + 5;

            checkPage(6);
            doc.setFont(fontFamily, 'normal');
            doc.setFontSize(10.5);
            setColor([0, 0, 0]);
            doc.text(bullet, bulletX, y);

            // Save y, render inline text, pass textX as indent offset
            const inlineText = item.tokens
              .filter(tk => tk.type === 'text' || tk.type === 'paragraph')
              .map(tk => (tk as any).text as string)
              .join(' ');

            const savedY = y;
            renderInline(inlineText, textX, 10.5);

            // Recurse for nested lists
            const nestedLists = item.tokens.filter(tk => tk.type === 'list');
            if (nestedLists.length) renderTokens(nestedLists, listDepth + 1);
          }
          y += 1;
          break;
        }

        case 'blockquote': {
          const t = token as Tokens.Blockquote;
          checkPage(8);
          doc.setDrawColor(secondaryColor[0], secondaryColor[1], secondaryColor[2]);
          doc.setLineWidth(0.8);
          const startY = y;
          setColor(secondaryColor);
          renderTokens(t.tokens, listDepth);
          doc.line(PAGE_MARGIN, startY, PAGE_MARGIN, y);
          setColor([0, 0, 0]);
          y += 2;
          break;
        }

        case 'code': {
          const t = token as Tokens.Code;
          const codeLines = t.text.split('\n');
          const blockH = codeLines.length * 5 + 4;
          checkPage(blockH);
          doc.setFillColor(245, 245, 245);
          doc.setDrawColor(200, 200, 200);
          doc.roundedRect(PAGE_MARGIN, y - 3, CONTENT_WIDTH, blockH, 1, 1, 'FD');
          doc.setFont('courier', 'normal');
          doc.setFontSize(9);
          setColor([50, 50, 50]);
          for (const cl of codeLines) {
            checkPage(5);
            const wrapped: string[] = doc.splitTextToSize(cl, CONTENT_WIDTH - 4);
            doc.text(wrapped, PAGE_MARGIN + 2, y);
            y += wrapped.length * 5;
          }
          doc.setFont(fontFamily, 'normal');
          setColor([0, 0, 0]);
          y += 4;
          break;
        }

        case 'hr': {
          checkPage(4);
          y += 2;
          doc.setDrawColor(200, 200, 200);
          doc.setLineWidth(0.3);
          doc.line(PAGE_MARGIN, y, PAGE_WIDTH - PAGE_MARGIN, y);
          y += 4;
          break;
        }

        case 'space': {
          y += 3;
          break;
        }

        default:
          break;
      }
    }
  }

  const tokens = Lexer.lex(markdownContent);
  renderTokens(tokens);

  // ── footer ───────────────────────────────────────────────────────────────
  const pageCount = doc.getNumberOfPages();
  for (let i = 1; i <= pageCount; i++) {
    doc.setPage(i);
    doc.setFont(fontFamily, 'normal');
    doc.setFontSize(8);
    setColor(secondaryColor);
    doc.text(`Page ${i} of ${pageCount}`, PAGE_WIDTH / 2, PAGE_HEIGHT - 8, { align: 'center' });
  }

  return new Uint8Array(doc.output('arraybuffer') as ArrayBuffer);
}

// ── utilities ──────────────────────────────────────────────────────────────

/** Strip markdown inline syntax to get plain text (for size calculations) */
function stripInlineMarkdown(text: string): string {
  return text
    .replace(/\*\*(.+?)\*\*/g, '$1')
    .replace(/\*(.+?)\*/g, '$1')
    .replace(/`(.+?)`/g, '$1')
    .replace(/\[(.+?)\]\(.+?\)/g, '$1');
}

function hexToRgb(hex: string): [number, number, number] {
  const result = /^#?([a-f\d]{2})([a-f\d]{2})([a-f\d]{2})$/i.exec(hex);
  if (result) {
    return [parseInt(result[1], 16), parseInt(result[2], 16), parseInt(result[3], 16)];
  }
  return [0, 0, 0];
}

function uint8ArrayToDataUrl(uint8Array: Uint8Array): Promise<string> {
  return new Promise((resolve, reject) => {
    const blob = new Blob([uint8Array.buffer as ArrayBuffer], { type: 'image/png' });
    const reader = new FileReader();
    reader.onload = () => resolve(reader.result as string);
    reader.onerror = reject;
    reader.readAsDataURL(blob);
  });
}

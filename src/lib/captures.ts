// Capturas por card do grid: copiar saída visível, copiar seleção e
// exportar o terminal como PNG (composição dos canvases do xterm).
import { getEntry, getTermTheme, TERM_FONT } from "./termRegistry";

/** Copia as linhas visíveis (viewport) do terminal para o clipboard. */
export async function copyVisible(uiId: string): Promise<boolean> {
  const entry = getEntry(uiId);
  if (!entry) return false;
  const buffer = entry.term.buffer.active;
  const lines: string[] = [];
  for (let i = 0; i < entry.term.rows; i++) {
    const line = buffer.getLine(buffer.viewportY + i);
    if (line) lines.push(line.translateToString(true));
  }
  await navigator.clipboard.writeText(lines.join("\n").trimEnd());
  return true;
}

/** Copia a seleção atual do terminal. */
export async function copySelection(uiId: string): Promise<boolean> {
  const entry = getEntry(uiId);
  const text = entry?.term.getSelection();
  if (!text) return false;
  await navigator.clipboard.writeText(text);
  return true;
}

/** Cores da captura por tema (tokens do app; o fundo do xterm é transparente). */
const CAPTURE_COLORS = {
  dark: { bg: "#101317", title: "#e6e8ea", sub: "#565c64", fg: "#e6e8ea" },
  light: { bg: "#faf9f6", title: "#1a1d21", sub: "#747b84", fg: "#1a1d21" },
} as const;

/**
 * Exporta o terminal do card como PNG para o clipboard: compõe os canvases
 * do xterm sobre o fundo do card, com cabeçalho "nome · addr". Sem canvases
 * (renderer DOM, WebGL indisponível) desenha o texto visível do buffer.
 */
export async function cardToPng(uiId: string, title: string, subtitle: string): Promise<boolean> {
  const entry = getEntry(uiId);
  if (!entry) return false;
  const colors = CAPTURE_COLORS[getTermTheme()];
  const canvases = [...entry.container.querySelectorAll<HTMLCanvasElement>("canvas")];

  const dpr = window.devicePixelRatio || 1;
  const pad = 14 * dpr;
  const headerH = 30 * dpr;

  // fallback texto: linhas visíveis do buffer, em monoespaçada
  const fontSize = (entry.term.options.fontSize ?? 13) * dpr;
  const lineH = Math.round(fontSize * (entry.term.options.lineHeight ?? 1.2));
  let lines: string[] = [];
  let width: number;
  let height: number;
  if (canvases.length > 0) {
    width = Math.max(...canvases.map((c) => c.width));
    height = Math.max(...canvases.map((c) => c.height));
  } else {
    const buffer = entry.term.buffer.active;
    for (let i = 0; i < entry.term.rows; i++) {
      lines.push(buffer.getLine(buffer.viewportY + i)?.translateToString(true) ?? "");
    }
    while (lines.length > 0 && lines[lines.length - 1].trim() === "") lines.pop();
    if (lines.length === 0) lines = [""];
    const measure = document.createElement("canvas").getContext("2d")!;
    measure.font = `${fontSize}px ${TERM_FONT}`;
    width = Math.ceil(Math.max(measure.measureText("M").width * entry.term.cols, 1));
    height = lineH * lines.length;
  }
  if (width === 0 || height === 0) return false;

  const out = document.createElement("canvas");
  out.width = width + pad * 2;
  out.height = height + pad * 2 + headerH;
  const ctx = out.getContext("2d")!;

  ctx.fillStyle = colors.bg;
  ctx.fillRect(0, 0, out.width, out.height);

  ctx.fillStyle = colors.title;
  ctx.font = `600 ${12 * dpr}px ${TERM_FONT}`;
  ctx.fillText(title, pad, pad + 12 * dpr);
  ctx.fillStyle = colors.sub;
  ctx.font = `${11 * dpr}px ${TERM_FONT}`;
  ctx.fillText(subtitle, pad + ctx.measureText(title).width + 18 * dpr, pad + 12 * dpr);

  if (canvases.length > 0) {
    for (const canvas of canvases) {
      ctx.drawImage(canvas, pad, pad + headerH);
    }
  } else {
    ctx.fillStyle = colors.fg;
    ctx.font = `${fontSize}px ${TERM_FONT}`;
    ctx.textBaseline = "top";
    lines.forEach((line, i) => ctx.fillText(line, pad, pad + headerH + i * lineH));
  }

  const blob = await new Promise<Blob | null>((resolve) => out.toBlob(resolve, "image/png"));
  if (!blob) return false;
  await navigator.clipboard.write([new ClipboardItem({ "image/png": blob })]);
  return true;
}

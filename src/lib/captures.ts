// Capturas por card do grid: copiar saída visível, copiar seleção e
// exportar o terminal como PNG (composição dos canvases do xterm).
import { getEntry, TERM_FONT } from "./termRegistry";

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

/**
 * Exporta o terminal do card como PNG para o clipboard: compõe os canvases
 * do xterm sobre o fundo do card, com cabeçalho "nome · addr".
 */
export async function cardToPng(uiId: string, title: string, subtitle: string): Promise<boolean> {
  const entry = getEntry(uiId);
  if (!entry) return false;
  const canvases = entry.container.querySelectorAll<HTMLCanvasElement>("canvas");
  if (canvases.length === 0) return false;

  const width = Math.max(...[...canvases].map((c) => c.width));
  const height = Math.max(...[...canvases].map((c) => c.height));
  if (width === 0 || height === 0) return false;

  const dpr = window.devicePixelRatio || 1;
  const pad = 14 * dpr;
  const headerH = 30 * dpr;

  const out = document.createElement("canvas");
  out.width = width + pad * 2;
  out.height = height + pad * 2 + headerH;
  const ctx = out.getContext("2d")!;

  ctx.fillStyle = "#101317";
  ctx.fillRect(0, 0, out.width, out.height);

  ctx.fillStyle = "#e6e8ea";
  ctx.font = `600 ${12 * dpr}px ${TERM_FONT}`;
  ctx.fillText(title, pad, pad + 12 * dpr);
  ctx.fillStyle = "#565c64";
  ctx.font = `${11 * dpr}px ${TERM_FONT}`;
  ctx.fillText(subtitle, pad + ctx.measureText(title).width + 18 * dpr, pad + 12 * dpr);

  for (const canvas of canvases) {
    ctx.drawImage(canvas, pad, pad + headerH);
  }

  const blob = await new Promise<Blob | null>((resolve) => out.toBlob(resolve, "image/png"));
  if (!blob) return false;
  await navigator.clipboard.write([new ClipboardItem({ "image/png": blob })]);
  return true;
}

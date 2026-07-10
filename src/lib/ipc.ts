// Wrappers tipados dos commands/eventos Tauri (contratos do PLANO.md §1).
import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";

export function openLocalSession(id: string, cols: number, rows: number): Promise<void> {
  return invoke("open_local_session", { id, cols, rows });
}

export function writeStdin(id: string, data: string): Promise<void> {
  return invoke("write_stdin", { id, data });
}

export function resizePty(id: string, cols: number, rows: number): Promise<void> {
  return invoke("resize_pty", { id, cols, rows });
}

export function closeSession(id: string): Promise<void> {
  return invoke("close_session", { id });
}

export interface SessionOutput {
  id: string;
  /** chunk de bytes do PTY em base64 */
  data: string;
}

export function onSessionOutput(handler: (payload: SessionOutput) => void): Promise<UnlistenFn> {
  return listen<SessionOutput>("session-output", (event) => handler(event.payload));
}

export function onSessionExit(handler: (id: string) => void): Promise<UnlistenFn> {
  return listen<{ id: string }>("session-exit", (event) => handler(event.payload.id));
}

export function base64ToBytes(b64: string): Uint8Array {
  const bin = atob(b64);
  const bytes = new Uint8Array(bin.length);
  for (let i = 0; i < bin.length; i++) bytes[i] = bin.charCodeAt(i);
  return bytes;
}

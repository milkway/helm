// Wrappers tipados dos commands/eventos Tauri (IPC front ↔ Rust).
import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import type { Host, SessionStatus } from "../types";

export interface SessionParams {
  mode: "shell" | "tmux" | "clmux";
  sessionName?: string;
  projectDir?: string;
}

export function openSshSession(
  id: string,
  hostId: string,
  cols: number,
  rows: number,
  params?: SessionParams,
): Promise<void> {
  return invoke("open_ssh_session", { id, hostId, cols, rows, params: params ?? null });
}

export interface TestResult {
  ok: boolean;
  latencyMs: number;
  tmux: string | null;
  message: string | null;
}

export function testConnection(draft: {
  user: string | null;
  host: string;
  port: number | null;
}): Promise<TestResult> {
  return invoke("test_connection", { draft });
}

export interface RemoteInfo {
  os: string | null;
  pkgManager: string | null;
  tmux: string | null;
}

export function detectRemote(hostId: string): Promise<RemoteInfo> {
  return invoke("detect_remote", { hostId });
}

export function installTmux(
  hostId: string,
  pkgManager: string,
  auth: { credentialId?: string; password?: string },
): Promise<string> {
  return invoke("install_tmux", {
    hostId,
    pkgManager,
    auth: { credentialId: auth.credentialId ?? null, password: auth.password ?? null },
  });
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

export function listHosts(): Promise<Host[]> {
  return invoke("list_hosts");
}

export function saveHost(host: Host): Promise<void> {
  return invoke("save_host", { host });
}

export function deleteHost(id: string): Promise<void> {
  return invoke("delete_host", { id });
}

export function hostHasSshCredential(hostId: string): Promise<boolean> {
  return invoke("host_has_ssh_credential", { hostId });
}

export function importSshConfig(): Promise<number> {
  return invoke("import_ssh_config");
}

export interface VaultCredentialMeta {
  id: string;
  kind: "ssh_key" | "password";
  label: string;
  algo: string | null;
  scope: string | null;
  lastUsed: string | null;
  /** false = só metadados (NOPASSWD, chave sem passphrase) */
  hasSecret: boolean;
}

export interface VaultStatus {
  locked: boolean;
  count: number;
}

export function vaultStatus(): Promise<VaultStatus> {
  return invoke("vault_status");
}

export function vaultUnlock(): Promise<VaultStatus> {
  return invoke("vault_unlock");
}

export function vaultLock(): Promise<VaultStatus> {
  return invoke("vault_lock");
}

export function vaultList(): Promise<VaultCredentialMeta[]> {
  return invoke("vault_list");
}

export function vaultSave(meta: VaultCredentialMeta, secret: string): Promise<void> {
  return invoke("vault_save", { meta, secret });
}

export function vaultDelete(id: string): Promise<void> {
  return invoke("vault_delete", { id });
}

export function vaultReveal(id: string): Promise<string> {
  return invoke("vault_reveal", { id });
}

export function onVaultStatus(handler: (status: VaultStatus) => void): Promise<UnlistenFn> {
  return listen<VaultStatus>("vault-status", (event) => handler(event.payload));
}

export interface AppInfo {
  version: string;
  build: string;
  arch: string;
  os: string;
}

export function appInfo(): Promise<AppInfo> {
  return invoke("app_info");
}

export interface UpdateInfo {
  current: string;
  latest: string;
  updateAvailable: boolean;
}

export function checkUpdates(): Promise<UpdateInfo> {
  return invoke("check_updates");
}

export function openExternal(url: string): Promise<void> {
  return invoke("open_external", { url });
}

export interface VpnProfile {
  name: string;
  state: "connected" | "disconnected" | "connecting";
  hostsUsing: number;
}

export function vpnList(): Promise<VpnProfile[]> {
  return invoke("vpn_list");
}

export function vpnConnect(profile: string): Promise<void> {
  return invoke("vpn_connect", { profile });
}

export function vpnDisconnect(profile: string): Promise<void> {
  return invoke("vpn_disconnect", { profile });
}

export function vpnSetAutoDisconnect(enabled: boolean): Promise<void> {
  return invoke("vpn_set_auto_disconnect", { enabled });
}

export function vpnGetAutoDisconnect(): Promise<boolean> {
  return invoke("vpn_get_auto_disconnect");
}

export function onVpnStatus(handler: (profiles: VpnProfile[]) => void): Promise<UnlistenFn> {
  return listen<VpnProfile[]>("vpn-status", (event) => handler(event.payload));
}

export interface SessionOutput {
  id: string;
  /** chunk de bytes do PTY em base64 */
  data: string;
}

export function onSessionOutput(handler: (payload: SessionOutput) => void): Promise<UnlistenFn> {
  return listen<SessionOutput>("session-output", (event) => handler(event.payload));
}

export interface SessionStatusPayload {
  id: string;
  status: SessionStatus;
  attempt?: number;
  delaySecs?: number;
  exitCode?: number;
}

export function onSessionStatus(
  handler: (payload: SessionStatusPayload) => void,
): Promise<UnlistenFn> {
  return listen<SessionStatusPayload>("session-status", (event) => handler(event.payload));
}

export interface AttentionPayload {
  id: string;
  active: boolean;
  reason?: string;
}

export function onAttention(handler: (payload: AttentionPayload) => void): Promise<UnlistenFn> {
  return listen<AttentionPayload>("attention", (event) => handler(event.payload));
}

export function detachSession(id: string): Promise<void> {
  return invoke("detach_session", { id });
}

export function retrySession(id: string): Promise<void> {
  return invoke("retry_session", { id });
}

export function base64ToBytes(b64: string): Uint8Array {
  const bin = atob(b64);
  const bytes = new Uint8Array(bin.length);
  for (let i = 0; i < bin.length; i++) bytes[i] = bin.charCodeAt(i);
  return bytes;
}

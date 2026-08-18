// Wrappers tipados dos commands/eventos Tauri (IPC front ↔ Rust).
import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import type { Host, SessionStatus } from "../types";

export interface SessionParams {
  mode: "shell" | "tmux" | "clmux";
  sessionName?: string;
  projectDir?: string;
  agent?: "claude" | "codex";
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
  claude: string | null;
  codex: string | null;
}

const REMOTE_INFO_TTL_MS = 10 * 60 * 1000;
const remoteInfoCache = new Map<
  string,
  { expiresAt: number; promise: Promise<RemoteInfo> }
>();

export function detectRemote(hostId: string): Promise<RemoteInfo> {
  const now = Date.now();
  const cached = remoteInfoCache.get(hostId);
  if (cached && cached.expiresAt > now) return cached.promise;

  const promise = invoke<RemoteInfo>("detect_remote", { hostId });
  remoteInfoCache.set(hostId, { expiresAt: now + REMOTE_INFO_TTL_MS, promise });
  void promise.catch(() => {
    if (remoteInfoCache.get(hostId)?.promise === promise) remoteInfoCache.delete(hostId);
  });
  return promise;
}

export function invalidateRemoteInfo(hostId: string): void {
  remoteInfoCache.delete(hostId);
}

export async function installTmux(
  hostId: string,
  pkgManager: string,
  auth: { credentialId?: string; password?: string },
): Promise<string> {
  const version = await invoke<string>("install_tmux", {
    hostId,
    pkgManager,
    auth: { credentialId: auth.credentialId ?? null, password: auth.password ?? null },
  });
  invalidateRemoteInfo(hostId);
  return version;
}

export function writeStdin(id: string, data: string): Promise<void> {
  return invoke("write_stdin", { id, data });
}

export function authorizeSudo(id: string): Promise<void> {
  return invoke("authorize_sudo", { id });
}

export function dismissSudoPrompt(id: string): Promise<void> {
  return invoke("dismiss_sudo_prompt", { id });
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

export async function saveHost(host: Host): Promise<void> {
  await invoke("save_host", { host });
  invalidateRemoteInfo(host.id);
}

export async function deleteHost(id: string): Promise<void> {
  await invoke("delete_host", { id });
  invalidateRemoteInfo(id);
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

export interface SudoPromptPayload {
  id: string;
  active: boolean;
  context: string;
}

export function onSudoPrompt(
  handler: (payload: SudoPromptPayload) => void,
): Promise<UnlistenFn> {
  return listen<SudoPromptPayload>("sudo-prompt", (event) => handler(event.payload));
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

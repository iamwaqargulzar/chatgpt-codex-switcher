import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import type {
  ActivityEntry,
  AuditEntry,
  OAuthDone,
  ProcessInfo,
  ProfileInfo,
  SessionInfo,
  Settings,
  StatusBundle,
  SwitchResult,
  UsageSnapshot,
  WarmupConfig,
} from "../types";

// ---- commands ----

export const status = () => invoke<StatusBundle>("desk_status");
export const oauthStart = () => invoke<void>("desk_oauth_start");
export const importAuth = (path?: string) => invoke<object>("desk_import_auth", { path });
export const importApiKey = (key: string, name?: string) =>
  invoke<object>("desk_import_apikey", { key, name });
export const removeAccount = (id: string) => invoke<void>("desk_remove_account", { id });
export const renameAccount = (id: string, name: string) =>
  invoke<void>("desk_rename_account", { id, name });
export const exportAccount = (id: string, path?: string) =>
  invoke<string>("desk_export_account", { id, path });
export const setProfile = (id: string, profile: string | null) =>
  invoke<void>("desk_set_profile", { id, profile });
export const setWarmup = (id: string, warmup: WarmupConfig) =>
  invoke<void>("desk_set_warmup", { id, warmup });
export const switchAccount = (id: string, force: boolean) =>
  invoke<SwitchResult>("desk_switch", { id, force });
export const restoreBackup = () => invoke<boolean>("desk_restore_backup");
export const refreshUsage = (id: string) => invoke<UsageSnapshot>("desk_refresh_usage", { id });
export const refreshAll = () => invoke<void>("desk_refresh_all");
export const warmup = (ids?: string[]) => invoke<void>("desk_warmup", { ids });
export const activityLog = (limit = 50) => invoke<ActivityEntry[]>("desk_activity_log", { limit });
export const auditLog = (limit = 50) => invoke<AuditEntry[]>("desk_audit_log", { limit });
export const sessions = () => invoke<SessionInfo[]>("desk_sessions");
export const sessionAction = (id: string, action: string) =>
  invoke<void>("desk_session_action", { id, action });
export const sessionLaunch = (id: string, mode: string) =>
  invoke<void>("desk_session_launch", { id, mode });
export const profiles = () => invoke<ProfileInfo[]>("desk_profiles");
export const profileRead = (name: string) => invoke<ProfileInfo>("desk_profile_read", { name });
export const profileWrite = (name: string, content: string) =>
  invoke<void>("desk_profile_write", { name, content });
export const profileCreate = (name: string) => invoke<ProfileInfo>("desk_profile_create", { name });
export const findProcesses = () => invoke<ProcessInfo[]>("desk_find_codex_processes");
export const killCodex = (pid: number) => invoke<void>("desk_kill_codex", { pid });
export const openCodex = (accountId?: string) =>
  invoke<void>("desk_open_codex", { accountId });
export const getSettings = () => invoke<Settings>("desk_settings");
export const saveSettings = (settings: Settings) =>
  invoke<Settings>("desk_save_settings", { newSettings: settings });
export const revealVault = () => invoke<void>("desk_reveal_vault");
export const checkCodex = () => invoke<string | null>("desk_check_codex");
export const showMain = () => invoke<void>("desk_show_main");
export const hidePopup = () => invoke<void>("desk_hide_popup");
export const quit = () => invoke<void>("desk_quit");

// ---- events ----

export function onEvent<T>(name: string, handler: (payload: T) => void) {
  return listen<T>(name, (event) => handler(event.payload));
}

export const EV = {
  vaultChanged: "vault://changed",
  usageUpdated: "usage://updated",
  activity: "activity://new",
  authProgress: "auth://progress",
  authDone: "auth://done",
  toast: "toast://show",
  settingsChanged: "settings://changed",
  switchBlocked: "switch://blocked",
  popupOpened: "popup://opened",
} as const;

export type AuthDonePayload = OAuthDone;

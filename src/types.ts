// Mirror of the Rust models in src-tauri/src/models.rs (camelCase on the wire).

export type AuthKind = "chatgpt" | "apikey";

export interface WarmupConfig {
  enabled: boolean;
  autoAfterReset: boolean;
  timedAt: string[];
}

export interface AccountPublic {
  id: string;
  name: string;
  email: string | null;
  kind: AuthKind;
  profile: string | null;
  addedAt: number;
  lastUsedAt: number | null;
  warmup: WarmupConfig;
  active: boolean;
  plan: string | null;
  subscriptionExpiresAt: number | null;
}

export interface RateBucket {
  name: string;
  window: string;
  resetAt: number;
  maximumTokens: number;
  remainingTokens: number;
  usedTokens: number;
}

export interface SoftHard {
  soft: number | null;
  hard: number | null;
}

export interface ResetCredit {
  id: string;
  expiresAt: number;
}

export interface ResetCredits {
  availableCount: number;
  resets: ResetCredit[];
}

export interface DayPoint {
  date: string;
  tokens: number;
}

export interface NameValue {
  name: string;
  tokens: number;
}

export interface UsageStats {
  lifetimeTokens: number;
  todayTokens: number;
  last7Tokens: number;
  last30Tokens: number;
  currentStreakDays: number;
  longestStreakDays: number;
  busiestDay: DayPoint | null;
  daily: DayPoint[];
  integrations: NameValue[];
}

export interface UsageSnapshot {
  fetchedAt: number;
  accountId: string | null;
  plan: string | null;
  subscriptionExpiresAt: number | null;
  systemHardLimitUsd: number | null;
  session: RateBucket | null;
  weekly: RateBucket | null;
  sessionSeconds: SoftHard | null;
  weeklySeconds: SoftHard | null;
  resetCredits: ResetCredits | null;
  stats: UsageStats | null;
}

export interface ProcessInfo {
  pid: number;
  name: string;
  cmdline: string;
}

export interface SwitchResult {
  switched: boolean;
  blocked: ProcessInfo[];
}

export interface ActivityEntry {
  ts: number;
  accountId: string;
  accountName: string;
  action: string;
  ok: boolean;
  detail: string;
}

export interface AuditEntry {
  ts: number;
  kind: string;
  accountId: string | null;
  detail: string;
}

export interface SessionInfo {
  id: string;
  cwd: string;
  createdAt: number;
  title: string;
  messageCount: number;
}

export interface ProfileInfo {
  name: string;
  isBase: boolean;
  content: string | null;
}

export interface Settings {
  theme: string;
  trayMode: string;
  hotkey: string | null;
  launchAtLogin: boolean;
  notifications: boolean;
  compact: boolean;
  refreshIntervalSecs: number;
  warmupTickSecs: number;
  terminalPreset: string;
  customTerminal: string | null;
  vaultEncrypted: boolean;
}

export interface StatusBundle {
  accounts: AccountPublic[];
  activeAccountId: string | null;
  snapshots: Record<string, UsageSnapshot>;
  settings: Settings;
  codexVersion: string | null;
  processes: ProcessInfo[];
  vaultEncrypted: boolean;
  terminalPresets: [string, string, boolean][];
}

export interface OAuthDone {
  ok: boolean;
  error: string | null;
  account: { id: string; name: string } | null;
}

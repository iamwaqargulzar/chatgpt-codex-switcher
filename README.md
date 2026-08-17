<div align="center">

# CodexDesk — ChatGPT Codex CLI Account Manager & Switcher

**A free, open-source desktop app for Linux that manages multiple ChatGPT Codex CLI accounts in one place.**

Switch accounts in one click · watch live rate limits & reset timers · track usage stats · warm up accounts automatically · manage per-account profiles and sessions.

[![License: MIT](https://img.shields.io/badge/license-MIT-green.svg)](LICENSE)
[![Platform: Linux](https://img.shields.io/badge/platform-Linux%20%7C%20Ubuntu%20%7C%20Pop!_OS-2f81f7.svg)](#)
[![Built with Tauri](https://img.shields.io/badge/built%20with-Tauri%202-ffc131.svg)](#)
[![Rust](https://img.shields.io/badge/backend-Rust-dea584.svg)](#)

> 🚧 **Active development** — the first public release is being built right now. Star the repo to follow along.

</div>

---

## What is CodexDesk?

CodexDesk is a **ChatGPT Codex CLI account switcher and account manager**. If you own more than one OpenAI account and use the [official Codex CLI](https://github.com/openai/codex), CodexDesk replaces manual `auth.json` editing with a desktop dashboard: every account is stored in a vault, switching takes one click, and your **rate limits, usage, and reset timers** are always on screen.

Built for **Linux first** — tested on **Ubuntu and Pop!_OS** — with macOS and Windows support in progress.

## Why CodexDesk?

- **One-click account switching** — no more copying `~/.codex/auth.json` files by hand. Your previous auth is backed up automatically and can be restored.
- **Live rate-limit monitoring** — see your current **5-hour session window** and **weekly window** as ring gauges, with **countdowns to the next reset**, remaining tokens, and credits.
- **Usage stats dashboard** — lifetime tokens, today / 7-day / 30-day usage, day streaks, busiest day, and per-integration breakdown for ChatGPT (OAuth) accounts.
- **Manual reset credits** — available reset credits shown as a badge with expiry highlighting (amber ≤ 10 days, red ≤ 3 days).
- **Smart warm-ups** — manually, automatically after each reset window, or on a timed schedule, with a visible log of every request the app makes.
- **Per-account profiles** — each account can carry its own Codex profile (`~/.codex/<name>.config.toml`): model, provider, sandbox, MCP servers, feature flags.
- **Session browser** — list, resume, fork, archive, and delete your Codex CLI sessions from the GUI.
- **Tray control** — switch accounts, check quota, and warm up from the system tray or the compact tray popup.
- **CLI companion** — `codexdesk switch <account>`, `codexdesk status`, and friends, for pure-terminal workflows and scripts.
- **Security first** — encrypted vault (OS keyring), zero telemetry, and a hard-coded network allowlist limited to OpenAI endpoints only.

## Features

| Area | What you get |
| --- | --- |
| 🔐 Account vault | Add accounts via ChatGPT OAuth login, import `auth.json`, or paste an API key. Rename, search, export, delete. Encrypted at rest with your OS keyring. |
| 🔀 Switching | Switch from the main window, tray menu, or tray popup. Detects running Codex sessions and offers a force-close flow. Auto-backup + restore of the previous auth. |
| 📊 Rate limits | Real-time 5-hour and weekly windows, remaining percentage, used/max tokens, reset countdowns, credit balance, subscription expiry. |
| 📈 Usage stats | Lifetime tokens, daily buckets, streaks, busiest day, 7/30-day charts, and most-used integrations. |
| 🎟️ Reset credits | Available manual reset credits with the closest expiry highlighted as it approaches. |
| 🔥 Warm-ups | Manual, auto-after-reset, or timed schedules. Conservative defaults, per-account toggles, and a complete activity log. |
| 🖥️ Profiles | Create and edit per-account Codex config profiles (model, MCP, sandbox, flags) and launch Codex with `--profile`. |
| 💬 Sessions | Browse `~/.codex/sessions`: resume, fork, archive, or delete without leaving the app. |
| 🗔 Tray & popup | System-tray menu plus a compact popup with quota and one-key switching. Optional global hotkey. |
| 🔔 Notifications | Desktop alerts when a limit drops below 10%, when a reset window opens, and when a warm-up runs. |
| ⌨️ Keyboard-first | Arrow keys to move between accounts, `/` to search, `Ctrl+K` command palette, `Enter` to switch. |
| 🎨 Themes | Dark, light, and auto themes; compact density mode. |

## Install

> 📦 First release binaries are in progress. Until then, build from source (2 minutes):

### Build from source

**Prerequisites:** [Rust](https://rustup.rs), [Node.js](https://nodejs.org) 18+, [pnpm](https://pnpm.io), and Tauri's Linux system libraries:

```bash
sudo apt install libwebkit2gtk-4.1-dev libappindicator3-dev librsvg2-dev patchelf build-essential
```

```bash
git clone https://github.com/iamwaqargulzar/chatgpt-codex-switcher.git
cd chatgpt-codex-switcher
pnpm install
pnpm tauri build
```

The `.deb` and AppImage land in `src-tauri/target/release/bundle/`.

### Run in development

```bash
pnpm install
pnpm tauri dev
```

### CLI companion

```bash
cargo install --path src-tauri --bin codexdesk
codexdesk list          # list accounts
codexdesk switch dev    # switch by name or id
codexdesk status        # active account + quota snapshot
codexdesk warmup all    # warm up all enabled accounts
```

## FAQ

**What is the ChatGPT Codex CLI?**  
The [official Codex CLI](https://github.com/openai/codex) is OpenAI's terminal agent for coding. It authenticates through ChatGPT accounts and enforces per-account usage windows.

**Why do I need an account switcher?**  
The Codex CLI stores exactly one account in `~/.codex/auth.json`. People with several personal accounts normally swap that file by hand. CodexDesk does it in one click and adds monitoring on top.

**How does switching work?**  
CodexDesk writes the selected account's credentials into `~/.codex/auth.json` (respecting `CODEX_HOME`), backs up whatever was there before, and records the switch in an audit log.

**Is it safe? Are there any backdoors?**  
CodexDesk is fully open source and has zero telemetry. Its outbound network traffic is hard-coded to `auth.openai.com`, `chatgpt.com`, and `api.openai.com` only — nothing else is ever contacted. Credentials are encrypted at rest with your OS keyring, and warm-ups log every request they make.

**Where is my data stored?**  
In `~/.local/share/codexdesk/` (vault, settings, audit and activity logs). Nothing leaves your machine.

**Does this break OpenAI's terms of service?**  
CodexDesk is for individuals managing accounts they personally own. It does not share or pool credentials and does not bypass quotas.

**Why does OAuth login use port 1455?**  
That is the localhost callback port registered for the official Codex CLI OAuth client. A browser tab opens, you approve, and the tab closes itself.

**Does it work on Ubuntu and Pop!_OS?**  
Yes — that is the primary target. X11 and Wayland are both supported for the app window; the tray icon uses AppIndicator and the global hotkey requires X11 (a Wayland limitation of the underlying hotkey library).

**Can I use API-key accounts too?**  
Yes. Import an `auth.json` containing an `OPENAI_API_KEY`, or paste a key directly.

## Roadmap

- [ ] Account vault (OAuth, `auth.json` import, API key)
- [ ] One-click switching with backup/restore and process detection
- [ ] Rate-limit gauges, reset countdowns, reset-credits badges
- [ ] Usage stats (lifetime, 7/30-day, streaks, integrations)
- [ ] Warm-ups: manual, auto-after-reset, timed
- [ ] Per-account profiles and session browser
- [ ] Tray menu, tray popup, notifications, hotkeys
- [ ] CLI companion (`codexdesk`)
- [ ] First signed release builds (.deb, AppImage, rpm)
- [ ] macOS and Windows builds
- [ ] Encrypted vault export/import for backups

## Privacy & security posture

- **No telemetry, no analytics, no crash reporters.**
- **Network allowlist:** `auth.openai.com`, `chatgpt.com`, `api.openai.com` — auditable with one grep.
- **No listening ports** in normal operation (the OAuth callback binds to `127.0.0.1` only while a login is in progress).
- **No auto-update daemon** — updates are checked only when you ask.
- **Vault encrypted** with AES-256-GCM using a key stored in your OS keyring (falls back to `0600`-permission plaintext with a visible warning if no keyring is available).

## Contributing

Pull requests, issues, and feature requests are welcome. Keep contributions self-contained and add a screenshot or log excerpt for UI changes.

## License

[MIT](LICENSE) © 2026 CodexDesk contributors

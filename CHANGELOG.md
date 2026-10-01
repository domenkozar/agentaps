# Changelog

Notable changes to Agentaps are recorded here.

## [Unreleased]

### Added

- Split desktop chats right or down from the session menu into resizable, independent panes. Restore layouts and visible sessions on restart, keep drafts and attachments with their sessions, and review changes in a shared diff panel that follows the active pane. Empty panes keep a compact menu, and the border indicates focus.

- Rename sessions inline by clicking the header title or choosing Rename session in the session menu. Hovering the title reveals an edit box. Custom names are searchable in the sidebar and survive restarts and agent title updates.

- Choose and remember a desktop theme from the sidebar, including light and dark presets ported from FactorSeal and a System option that follows GTK/Qt theme changes on Linux. Chat, activity, controls, and diffs share the selected palette while retaining zoom.

- Paste or drop images into the composer and see images in conversations.
- A **+** menu under the composer attaches files or images to a prompt. Dropped files attach too. Text files are sent with their contents when the agent accepts embedded context, and as links otherwise. Attached file contents echoed back by the agent show as a code block of the first few lines.

### Fixed

- The default Agentaps theme now follows the desktop light/dark scheme with matching light and dark palettes across the conversation, sidebar, controls, and diffs.

- Conversation history comes from harness session replay instead of a duplicate local transcript. Config saves contain only session references and app state, skip unchanged data, and retain prior session references in the archive after context resets or folder moves. An opt-in smoke test verifies history replay after restarting an authenticated harness.
- Web Connect pairs and connects in Safari, which previously could not reach the default Iroh relays.

### Changed

- The main website download button shows the latest Agentaps release version.

- The website calls browser access Remote Control and retains the GitHub stars count and latest release version.

- The website keeps the original dark blue palette in a refreshed layout with a centered hero, install and quickstart panels, a full desktop screenshot, and clearer agent and Remote Control sections.

- Desktop theme conversion uses the upstream native-theme-gpui connector pinned to its GPUI Kit 0.7 revision, while retaining live GTK/Qt detection, saved theme choices, and zoom.

- Linux builds and packages now require GTK 4.10 or later and Qt 6 Widgets for native desktop theming; Linux desktop CI now builds on Ubuntu 24.04.

- Context usage beneath the composer includes a progress bar that turns from green to amber at 60% and red at 85%, with token counts and Reset context still available.

- Thought messages use a compact disclosure row like completed activity, stay collapsed by default, and expand to show selectable Markdown when clicked.

- The README and website share one updated desktop screenshot showing the current conversation layout and agent controls.

- The website links directly to the Agentaps 0.4.0 desktop installers and shows the latest release version.

## [0.4.0] - 2026-09-30

### Added

- Pull requests and main branch changes run formatting, strict lint checks, and desktop tests on Linux, macOS, and Windows, plus browser protection tests and a complete WASM website build.
- CI checks that the devenv shell starts on Linux and macOS.
- A View menu offers Zoom In, Zoom Out, and Reset Zoom (Cmd+=, Cmd+-, Cmd+0 on macOS, Ctrl on other platforms) to enlarge or shrink the app font size, remembered across restarts.
- `agentaps --version` prints the installed version without opening the desktop app.
- A Mode button at the bottom left of the composer shows the agent's current mode and switches between the modes the agent offers, such as Claude's Manual, Accept edits, Plan, and Auto. It is hidden for agents that report no modes.
- A Plan toggle next to the Mode button switches planning on and off for agents that offer it separately from their modes, such as Codex.

### Fixed

- File-search regression checks accept native path separators on Windows.
- Session archive and restore buttons stay vertically centered in the sidebar.
- Informational notices, including Git sync confirmations, use a soft blue style, while errors stay red. All notices have a dismiss button.
- Windows saves sessions in its platform configuration directory without requiring `HOME`; existing configurations remain available through migration.
- Saving conversation history no longer blocks the UI on serialization or disk writes, coalesces waiting saves, and backs off after storage errors.
- Restored zoom now scales markdown code blocks, keeps multiline composers within the available space, and leaves other notices and save errors visible.
- Zoom In, Zoom Out, and Reset Zoom now reach the workspace regardless of which element has focus, and each zoom change shows a notice with the new percentage that hides itself after a second and a half.
- Zooming now visibly resizes the UI. The zoom level is written to the UI theme, whose font size the window applies on every frame; writing the window's rem size directly was overwritten before the next paint. Code text in the composer scales along.
- The Send and Stop buttons stay vertically centered in the message composer.
- Web Connect on phones no longer freezes on the first tap, so mobile browsers can type a passphrase and pair with the desktop again.
- Diff review and sidebar line counts exclude untracked files, so an otherwise unchanged checkout shows no diff.
- `devenv shell` evaluates on macOS again. The Linux-only graphics libraries (wayland, libxkbcommon, libxcb, vulkan-loader, fontconfig, freetype) are now installed only on Linux.
- Claude ACP sessions retry once when another Claude Code process temporarily blocks OAuth token refresh.
- Diff loading now uses a toolbar spinner without shifting the diff view.
- Split diffs keep both code panes aligned around a vertical divider.
- Sidebar upstream counts refresh when an agent finishes, so successful pushes do not appear to need pushing again.

### Changed

- Regression checks cover session draft and cursor preservation, zoom restoration, and Web Connect commands reaching the intended session while another is displayed.
- Desktop sessions use a UI-independent controller with common ACP updates; workspace components own their related state, and browser protection, transport, actions, and rendering live in separate modules.
- Session headers show the full project path beside Diff instead of the Git branch. Selecting the path lets you move the session to another folder and reconnect the agent there with fresh context while keeping earlier messages visible.
- Sidebar sessions show Git upstream counts in the agent status position when the agent has no active status.
- Diff review opens beside the conversation with one compact row for the comparison and change totals, keeping chat and its composer available.
- The message composer grows with pasted or typed multiline text up to most of the window height, then scrolls within the composer.
- The conversation and composer sit in a centered, width-limited column. The composer has an inline Send or Stop button, and model, effort, and context usage (with Reset) moved beneath it. The header is a single row with the project and branch, and a session menu for reset, new agent session, and archive. Agent reply actions (Copy, Fork from here) appear under the reply on hover.
- The README shows a recent desktop conversation and task activity screenshot.
- The website's direct download links and release label now show Agentaps 0.3.1.

## [0.3.1] - 2026-09-29

### Added

- The desktop header shows agent-supplied session titles after the context and Reset control and on session hover, retaining them across restarts.

### Changed

- Agentaps no longer scans folders under your home directory at startup. The new session picker offers a native folder chooser, recent projects, and manually entered local or SSH paths.
- Desktop header controls show reasoning effort and context usage without their text prefixes.
- Context usage and Reset now share one control in the desktop conversation header.
- The website shows Agentaps 0.3.0 and links directly to its public desktop installers.
- The README now leads with installation and everyday use, with detailed Web Connect and deployment instructions in separate guides.
- The README and website now show the same desktop screenshot.

### Fixed

- Local Claude ACP sessions use the installed Claude Code CLI and its configured authentication outside NixOS.
- Agents installed in user paths such as Homebrew, `~/.local/bin`, or Nix profiles are found when Agentaps is opened from the Dock, Finder, or a desktop launcher.

## [0.3.0] - 2026-09-28

### Documentation

- Included the bundled Lucide icon license in source and Web Connect builds.
- Explained Web Connect's encrypted peer-to-peer Iroh connection in the website features.
- Documented the GitHub app connection and build results for Cloudflare Pages deployments.
- Documented the production site deployment and custom domain setup.
- Added a guide for reviewing desktop build candidates and preparing a release.
- Updated the README and website tagline to explain local and SSH harness use through ACP and secure web access.
- Updated the README screenshot to show the current session interface.
- Added a tagline that captures Agentaps' direction across work environments.

### Added

- Agent tasks interrupted by shutdown automatically continue after their session restores, before queued prompts run.
- Sidebar upstream commit counts stay visible, while hovering hides the branch name and reveals a Push or Pull action when the branch can sync safely; diverged branches explain that they need manual resolution.
- Each desktop platform button links directly to its own preview download.
- The website shows the preview build date and status below the desktop download buttons.
- The website links to the current desktop build candidates with their sign-in and expiration limits clearly labeled.
- The website shows a larger linked GitHub stars badge beside the logo, including on phones.
- Local and SSH agent sidebar rows show compact green up and red down commit counts at the right edge, refreshed periodically from the upstream remote.
- Website updates on GitHub `main` deploy through Cloudflare Pages.
- Candidate desktop builds for Linux, Apple Silicon macOS, and Windows can be downloaded from GitHub Actions before a release, and the development environment includes their packaging tool.
- Version tags prepare a draft release with verified desktop packages for review before publishing.
- Windows candidate builds validate that installed batch commands can be launched.
- The website now has a landing page with desktop download availability and an entry point to Web Connect.
- Desktop mobile pairing lists linked browsers and lets you revoke each new pairing individually; older shared-token pairings can be revoked together.
- Agent replies offer a fork icon at the top right of each reply that starts a separate session from that point and carries the active conversation into its first prompt.
- Agent replies offer a copy button below the fork icon that matches its size and briefly shows a checkmark after copying the reply text.
- Starting a message with `!` asks the agent to run the exact shell command and shows `shell` inside the composer.
- Diff buttons show live added and removed line counts without loading the full diff while closed.
- The browser can start a new local or SSH agent session from its agents page.
- The browser lists saved desktop pairings so you can choose which one to unlock or pair another desktop.
- The browser can open its camera to scan the desktop pairing QR code directly on the site.
- Typing `@` in a prompt offers fuzzy file path completion from the current project, including SSH projects.
- A browser preview can view active sessions and send prompts, stop turns, and answer ACP permission requests through an encrypted Iroh connection.
- Projects on SSH servers can run ACP agents remotely while conversations stay in Agentaps.
- Search fields show a clear button while typing, and Escape clears the current search.
- Session headers let you choose from models offered by the connected agent.
- Session headers let you start a new session with another coding agent in the same project.
- Sessions let you choose reasoning effort when the connected agent offers that setting.

### Changed

- Each agent session keeps its own chat draft, cursor, and undo history when switching sessions.
- Desktop and Web Connect use GPUI Kit 0.7 instead of the GPUI CE forks. Web Connect embeds its UI icons so they remain available without extra site assets, and its WASM build shares one reqwest version with Iroh.
- Automatic reviews no longer appear in agent conversation activity.
- Sidebar branch names align to the right edge of each session row.
- Compact sidebar upstream counts appear before project names beside agent status dots, and zero count directions are hidden.
- Idle agents leave their sidebar status slot blank so project names stay aligned, and completed agents use a brighter green dot.
- The website hero starts directly with its main headline.
- Opening a file in diff review expands its changes beneath the file row while the file list stays visible.
- The desktop download button matching the visitor's platform is highlighted.
- Desktop download buttons are larger, and the preview note now shows only the September 28 build date.
- Desktop build candidates use installable packages for Linux and Windows, with the macOS app distributed in a DMG.
- The website pairs the Agentaps desktop screenshot with the mobile preview.
- Desktop downloads are limited to Apple Silicon Macs; Intel Mac builds are not offered.
- Mobile pairing shows linked clients below the QR code at every window width.
- The development environment can build the browser site, and a command publishes it to Cloudflare Pages.
- `@` completion includes folders and shows a folder icon beside them.
- Desktop platform downloads now appear in the opening section, with availability shown there instead of in a separate section.
- New sessions accept a first message while connecting and send it when the agent is ready. Cached npx adapters can start without a package freshness check.
- Running tool activity uses the same status dot as a working agent.
- The website presents Agentaps as a Rust and Apache-2.0 universal UI for ACP-compatible coding harnesses, highlights Linux, macOS, and Windows, and links to source build instructions while installers are unavailable.
- The website logo uses horizontal strokes, and the landing page colors match the desktop app.
- Desktop mobile access offers a provider choice when the SecretSpec default is unavailable and shows how to configure one permanently.
- Archive and Mobile controls sit together as icon buttons in the desktop sidebar.
- The Stop agent control uses a stop-recording icon and matches the conversation action buttons.
- The browser agents page uses New in place of Lock and hides the Connected label when the session is healthy.
- The browser opens with a compact saved desktop chooser, and saved desktops can be renamed.
- Desktop mobile pairing shows a large QR code across the main window for easier phone scanning.
- Mobile pairing uses a one time link and saves an encrypted connection in the phone browser, protected by a passphrase or a compatible phone passkey. Unlocking opens the session without another Connect step.
- Desktop pairing links point to `agentaps.dev` by default.
- Mobile access stores its Iroh identity and pairing token in the user-global SecretSpec provider and gives setup guidance when none is configured.
- Conversation headers keep agent settings and Reset context on the left, project and Diff on the right, and stay compact in narrow windows.
- Dragging an agent in the sidebar shows a horizontal line at its drop position.
- Tool activity highlights the current step, groups completed steps, and describes checks and tests in plain language.

### Fixed

- Agent rows keep upstream commit counts visible while agents are working or done.
- Sidebar Git counts begin refreshing when the app opens.
- Reset context is hidden when a session's context usage is unknown or zero.
- Mobile access keeps the session sidebar visible and shows progress while SecretSpec loads credentials and the connection starts.
- The macOS desktop package includes an icon format its app bundler accepts.
- Landing page style updates avoid stale browser CSS that could make the product screenshot fill the screen.
- The landing page keeps its text, desktop availability, and device preview visible on narrow phones.
- Cloudflare Pages can build the website without system package installation privileges.
- Windows desktop builds find and launch installed agent commands with executable extensions.
- Native macOS and Windows builds use the current GPUI asset registry for menu icons.
- The visible saved session connects before other sessions on startup, and connection replies are handled sooner.
- File path completion adds a space and closes its suggestion menu.
- Finger swipes scroll browser conversations, and phone keyboards can type into the browser chat composer while the page adjusts to the keyboard.
- The browser conversation is easier to read on phones, with distinct message roles, formatted replies, a visible composer, and automatic scrolling to new replies when already at the bottom.
- Mobile pairing keeps its Iroh connection open until the browser receives the reply and reports desktop connection errors in logs.
- The browser preview falls back to WebGL2 when WebGPU cannot initialize.
- Diff review opens promptly and shows loading progress while checkout changes are read.

## [0.2.1] - 2026-09-25

### Changed

- Diff review opens with a file summary and lets you inspect one file at a time.

### Fixed

- Branch labels work without the Git executable installed.

## [0.2.0] - 2026-09-25

### Changed

- Diff views refresh automatically as checkout changes arrive.
- Prepared diff rows in the background to keep large changesets responsive.
- Switched to GPUI CE and its component library for improved Markdown rendering and streaming updates.
- Made folder search responsive while creating sessions by matching paths in the background and reusing results between renders.

### Added

- View each agent's checkout changes against HEAD in unified or split diff layouts, including untracked files.
- Recall earlier prompts with Up and Down in the chat composer, including queued prompts and drafts.
- Rank session search results by relevance and switch to the selected session as the search changes or Up and Down are pressed.
- Stop a working agent with Escape, the same as clicking Stop.

### Fixed

- Diff views fill the available panel height so large changesets scroll correctly.
- Sidebar rendering passes strict lint checks.
- Escape reliably stops a working agent while the chat input is focused.

### Documentation

- Added this changelog and instructions for keeping it up to date.

## [0.1.0] - 2026-09-25

### Added

- Released the first Agentaps desktop client on crates.io under the Apache-2.0 license.
- Added project sessions with saved chat history, agent reconnection, session archiving, and sidebar controls.
- Added discovery for Codex, Claude, Gemini CLI, and OpenCode, plus custom ACP commands and support for ACP v1 and v2 agents.
- Added chat controls for queued messages, stopping turns, resetting agent context, and completing agent slash commands.
- Added ACP form questions with answer, decline, and cancel actions.

[Unreleased]: https://github.com/domenkozar/agentaps/compare/v0.4.0...HEAD
[0.4.0]: https://github.com/domenkozar/agentaps/compare/v0.3.1...v0.4.0
[0.3.1]: https://github.com/domenkozar/agentaps/compare/v0.3.0...v0.3.1
[0.3.0]: https://github.com/domenkozar/agentaps/compare/v0.2.1...v0.3.0
[0.2.1]: https://github.com/domenkozar/agentaps/compare/v0.2.0...v0.2.1
[0.2.0]: https://github.com/domenkozar/agentaps/compare/v0.1.0...v0.2.0
[0.1.0]: https://github.com/domenkozar/agentaps/tree/v0.1.0

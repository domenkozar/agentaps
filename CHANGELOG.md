# Changelog

Notable changes to Agentaps are recorded here.

## [Unreleased]

### Documentation

- Updated the README screenshot to show the current session interface.
- Added a tagline that captures Agentaps' direction across work environments.

### Added

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

- New sessions accept a first message while connecting and send it when the agent is ready. Cached npx adapters can start without a package freshness check.
- Running tool activity uses the same status dot as a working agent.
- The website now introduces ACP harness compatibility and Linux, macOS, and Windows in its opening section.
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

- The visible saved session connects before other sessions on startup, and connection replies are handled sooner.
- Desktop and browser use current GPUI CE, including WebGL2 fallback and mobile input support.
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

[Unreleased]: https://github.com/domenkozar/agentaps/compare/v0.2.1...HEAD
[0.2.1]: https://github.com/domenkozar/agentaps/compare/v0.2.0...v0.2.1
[0.2.0]: https://github.com/domenkozar/agentaps/compare/v0.1.0...v0.2.0
[0.1.0]: https://github.com/domenkozar/agentaps/tree/v0.1.0

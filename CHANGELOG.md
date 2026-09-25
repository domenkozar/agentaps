# Changelog

Notable changes to Agentaps are recorded here.

## [Unreleased]

### Documentation

- Added a tagline that captures Agentaps' direction across work environments.

### Added

- Search fields show a clear button while typing, and Escape clears the current search.
- Session headers let you choose from models offered by the connected agent.
- Session headers let you start a new session with another coding agent in the same project.
- Sessions let you choose reasoning effort when the connected agent offers that setting.

### Changed

- Conversation headers keep agent settings and Reset context on the left, project and Diff on the right, and stay compact in narrow windows.

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

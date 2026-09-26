# Agentaps

Your coding agents, wherever you work.

Agentaps is a Rust and GPUI desktop client for agents that speak the Agent Client Protocol (ACP).

![Agentaps showing a Codex conversation and project sessions](docs/images/agentaps.png)

## Features

- **Project based sessions:** Open a folder with **New** or **Ctrl+P** (**Cmd+P** on macOS). Search by name or path, or enter an absolute path, then choose an agent.
- **Agent discovery:** Detect Codex and Claude ACP adapters, Gemini CLI's `--acp` mode, and OpenCode's `acp` mode. Enter a custom ACP command for other agents. Agentaps requests ACP v2 and also accepts v1 agents.
- **Session sidebar:** See each project's git branch and status, reorder rows by dragging, resize the sidebar, and archive sessions.
- **Diff review:** Open **Diff** from an agent conversation to see a file summary of the folder's staged, unstaged, and untracked changes against HEAD. Open a file to inspect its unified or split diff. The view refreshes automatically as the checkout changes, and agents in the same folder share it.
- **Persistent chats:** Save projects, agents, chat history, and queued messages. On launch, reconnect agents and resume sessions when supported. If an agent cannot restore a session, keep the saved chat visible and start a new session.
- **Context reset:** Start a fresh agent session in the same project while keeping earlier messages visible above a divider. Resetting stops the active turn and clears queued messages.
- **Conversation forks:** Choose the fork icon beside an agent reply to start a separate session with the conversation through that reply. The first new prompt includes user and agent messages since the last context reset. Agent internal state and past file versions are not restored.
- **Chat controls:** Send with **Enter**, insert a newline with **Ctrl+Enter**, stop an active turn, or queue messages while the agent works. Use **Up/Down** in the composer to recall earlier prompts. Type `/` to find agent commands, use **Up/Down** to choose one, and complete it with **Tab** or **Enter**. Start a message with `!` to ask the agent to run the following text as a shell command. A `shell` label appears inside the composer.
- **Structured questions:** Answer, decline, or cancel ACP form questions in conversation cards. A status dot and count show pending questions in the sidebar while the agent continues working.

## Run

Install from crates.io where GPUI's native build dependencies are available:

```sh
cargo install agentaps
```

Source builds on Linux need fontconfig and FreeType development files available to `pkg-config`.

From the repository root, with `devenv` installed:

```sh
devenv shell cargo run --release
```

The development environment provides Rust, the native libraries GPUI needs, and Node for optional ACP adapters.

## Notes

- If a Codex or Claude CLI is installed without an ACP adapter, Agentaps can offer one through `npx` when available. The first launch may download it.
- On NixOS, Agentaps points `claude-agent-acp` at an installed `claude` executable. Set `CLAUDE_CODE_EXECUTABLE` to override this.
- Session data is stored in `$XDG_CONFIG_HOME/agentaps/config.json`, or `~/.config/agentaps/config.json` if `XDG_CONFIG_HOME` is unset. Closing the app may interrupt an active turn.
- URL-based elicitation, ACP client file system and terminal methods, and a built-in authentication flow are not yet supported. Agents that require those client features may not work.

## License

Apache-2.0. See [LICENSE](LICENSE).

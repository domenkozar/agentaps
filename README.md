# Agentaps

Agentaps is a Rust and GPUI desktop client for agents that speak the Agent Client Protocol (ACP). Open a project folder, choose an agent, and chat with it while keeping its git branch and status in view.

## Run

From the repository root, with `devenv` installed:

```sh
devenv shell cargo run --release
```

The development environment provides Rust, the native libraries GPUI needs, and Node for optional ACP adapters.

## Use

1. Click **New** or press **Ctrl+P** (**Cmd+P** on macOS) to open the folder picker.
2. Search for a folder by name or path, or enter an absolute path. Press **Enter** to select it.
3. Choose a detected agent, or enter a custom ACP command. Agentaps starts it in the selected folder.

Each sidebar row shows a project, its git branch, and a status dot. Hover over the dot for the state. Drag rows to reorder them or drag the divider to resize the sidebar. Hover over a row to reveal its archive icon; **Archive** at the bottom shows archived sessions.

In chat, **Enter** sends and **Ctrl+Enter** inserts a newline. Type `/` for commands advertised by the current agent, use **Up/Down** to choose one, and press **Tab** or **Enter** to complete it. While the agent works, the square Stop icon cancels the current turn. You can send more messages during a turn; Agentaps queues them and sends them in order.

## Agents

Agentaps requests ACP v2 and also accepts v1 agents. It detects installed Codex and Claude ACP adapters, Gemini CLI's `--acp` mode, and OpenCode's `acp` mode. If a Codex or Claude CLI is installed without an adapter, Agentaps can offer one through `npx` when available. The first launch may download that adapter. You can also enter any ACP command directly.

On NixOS, Agentaps points `claude-agent-acp` at an installed `claude` executable. Set `CLAUDE_CODE_EXECUTABLE` to override this.

## Sessions

Agentaps saves projects, agents, chat history, and queued messages in `$XDG_CONFIG_HOME/agentaps/config.json`, or `~/.config/agentaps/config.json` if `XDG_CONFIG_HOME` is unset. On launch it reconnects saved agents and resumes their sessions when supported. If an agent cannot restore a session, the saved chat remains visible and a new session starts. An active turn may be interrupted when the app closes.

Agentaps does not yet provide ACP client file system or terminal methods, or a built-in authentication flow. Agents that require those client features may not work.

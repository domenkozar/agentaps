# Agentaps

A GPUI desktop client for Agent Client Protocol (ACP) agents. Each left sidebar row shows one agent's project folder and current git branch, with a colored status dot whose tooltip shows connecting, idle, working, done, or error. Drag rows to rearrange them and drag the divider to resize the sidebar; both are saved. Search sessions by project name, path, branch, or agent in the field above the list. The bottom row has Archive and New controls; New opens a folder, and Archive switches to saved sessions, which can be restored by clicking them. Hover over a session to reveal its archive icon. The right pane shows the selected agent's conversation, tool activity, and permission requests. Consecutive tool calls appear as compact activity groups with searches and reads summarized inline. Approval reviews are condensed into a status line. Click an activity heading to collapse it or an action to inspect its full command and output. A small indicator appears while the agent responds.

## Run

```sh
devenv shell cargo run --release
```

The included `devenv.nix` provides Rust, the native libraries needed by GPUI, and Node for optional ACP adapters. Inside an active `devenv shell`, run `cargo run --release` directly. The optimized build keeps the GPUI chat interface responsive.

The first screen is a folder picker. Press **Ctrl+P** on Linux and Windows or **Cmd+P** on macOS to open it from anywhere. Search folders by name or path with fuzzy matching, use the arrow keys to choose a result, and press Enter. The picker scans your home directory and current working directory in the background, to a depth of five folders. Hidden and generated directories are skipped. You can also type an existing absolute path directly.

After choosing a folder, pick a detected ACP agent. The app checks `PATH` for installed ACP adapters and for Gemini CLI and OpenCode's native ACP commands. If Codex or Claude is installed alongside `npx`, the picker offers the respective ACP adapter through `npx`; this downloads the adapter on first launch. Enter a custom ACP command if your agent is not listed. Commands are parsed as arguments, without a shell, and launched with the project as their working directory.

On NixOS, the Claude adapter is pointed at an installed `claude` executable. This avoids the adapter's bundled generic Linux executable, which NixOS cannot launch by default. Set `CLAUDE_CODE_EXECUTABLE` yourself to override that choice.

Type a message and press **Enter** to send. Press **Ctrl+Enter** to add a newline. Type `/` to see commands advertised by the selected agent, then use the arrow keys and **Tab** or **Enter** to complete one. The conversation has a scrollbar when it overflows. The square Stop icon beside the working indicator sends ACP `session/cancel` for the active turn. You can keep sending messages while the agent works; they appear as queued messages and are sent in order after the current turn ends. Queued messages are saved across app restarts. Permission choices are shown in the chat and sent back to the agent. The active agent header shows its model and context usage when the agent reports them through ACP session configuration and usage updates.

Opening an agent focuses the chat composer. Conversation text is selectable; drag over text and press **Ctrl+C** (or **Cmd+C** on macOS) to copy it.

Projects, agent commands, session IDs, model and context details, and visible chat and tool activity are saved in `$XDG_CONFIG_HOME/agentaps/config.json`, or `~/.config/agentaps/config.json` when `XDG_CONFIG_HOME` is unset. On first launch after the rename, Agentaps loads the old `devenv-terminal/config.json` if the new config does not exist, then saves it to the new location. The old file is retained. The file is written atomically with private permissions on Unix. When the app opens, it reconnects each saved agent and resumes its session when the agent supports it. ACP v2 uses `session/resume`; ACP v1 uses `session/resume` when advertised, or `session/load` otherwise. If an agent cannot restore a session, the app starts a new one and keeps the saved activity visible with a notice. An in-progress turn may be interrupted when the app closes.

The client requests draft ACP v2 and uses the official Rust v2 schema for initialization, prompt acknowledgements, and session updates. If an agent negotiates v1, the client uses its existing v1 conversation path. In v2, a prompt acknowledgement confirms message insertion; the sidebar changes to **done** only after the agent sends an idle `state_update`.

The client does not advertise optional client file system or terminal capabilities, so agents that require those client services will need a future integration. Agent authentication is also outside this initial version.

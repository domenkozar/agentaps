# Agentaps

Agentaps is a desktop workspace for coding agents that speak the [Agent Client Protocol (ACP)](https://agentclientprotocol.com/get-started/introduction). Run an agent in a local project or over SSH, keep its conversations together, and continue from a phone browser when you step away.

![Agentaps showing a Codex conversation, project sessions, and task activity](web/site/agentaps-desktop.png)

## Get started

1. [Download Agentaps](https://github.com/domenkozar/agentaps/releases) for Linux, Apple Silicon macOS, or Windows. You can also [build from source](#build-from-source).
2. Install an ACP compatible agent or adapter. Agentaps discovers Codex and Claude adapters, Gemini CLI (`gemini --acp`), and OpenCode (`opencode acp`). You can enter another ACP command yourself.
3. Open Agentaps and select **New**. Use **Choose Folder** to browse for a local project, select a recent folder, or enter a local absolute path. Then choose an agent and send a message.

If Codex or Claude is installed without its ACP adapter, Agentaps can offer to start the adapter through `npx`. The first run may download it. Agentaps supports ACP v1 and v2 agents.

## Work in Agentaps

- **Keep projects and conversations together.** Agentaps saves harness session references and queued prompts. Conversation history stays in the harness and is replayed when you reopen a session. Agent-supplied session titles appear in the desktop header and when you hover over a session in the sidebar. Select the project path beside Diff to move a session to another folder; the agent reconnects there with fresh context, and the previous session remains in the archive. On launch, Agentaps reconnects agents and resumes sessions when they support it. You can archive sessions or reset an agent's context, which archives the previous session. Forking a reply starts a new session with the visible conversation as context.
- **View chats together.** Use the session **••• menu** to **Split Right** or **Split Down**, then select a sidebar session or create one in the new pane. Empty panes keep a small **••• menu** in the top-right corner. Split any pane again, drag dividers to resize, and use **Close Pane** to remove a view while its agent keeps running. Sidebar selections open in the active pane; sessions already visible receive focus. Layouts restore on restart, and the shared Diff panel follows the active pane.
- **Choose your theme.** Open **Theme** using the settings button at the bottom of the sidebar. Choose Agentaps, System, or a bundled light or dark palette. Agentaps follows the desktop light/dark scheme using its own palettes; bundled presets keep their selected scheme. The choice and zoom are remembered across restarts. On Linux, System follows the current GTK or Qt theme, including desktop settings changes.
- **Review local changes.** Select **Diff** in a conversation to inspect staged and unstaged changes against HEAD. Open files in unified or split view. The diff refreshes as the checkout changes.
- **Use your agent's controls.** Choose a model or reasoning effort when the agent offers them, run its slash commands, and answer ACP form questions in the chat.
- **Work over SSH.** Enter a path such as `ssh://user@server.example/home/user/project` when creating a project. Agentaps runs the agent on that server using your existing SSH configuration and keys. The agent and its ACP adapter must be installed there, and the server's host key must already be known. Try `ssh user@server.example` first. Diff review is currently available for local projects only.
- **Continue in a browser.** Pair a phone browser with the desktop app to read conversations, send prompts, stop turns, and answer permission requests. Agent processes and project files stay on the desktop computer.

In the composer, **Enter** sends a prompt and **Ctrl+Enter** inserts a newline. Type `/` for agent commands, `@` to find a project file, or start with `!` to ask the agent to run a shell command. Prompts sent while an agent is busy are queued. **Up/Down** recalls earlier prompts when the composer is empty. Paste an image or drop an image file on the composer to send it with your prompt when the agent accepts images. To attach other files, use **+** under the composer or drop them on it. Text files go to the agent with their contents when it supports embedded context; other files are sent as links.

## Web Connect

1. In the desktop sidebar, select the phone icon. Agentaps shows a pairing QR code.
2. On your phone, open [Web Connect](https://agentaps.dev/connect/) and scan the code, or open the pairing link from the desktop.
3. Protect the saved connection with a compatible phone passkey or a passphrase of at least 15 characters. On later visits, unlock the saved desktop to reconnect.

Web Connect connects to the running desktop app through Iroh. Both devices need network access to a compatible relay. The desktop stores its Iroh identity and linked browser credentials in a [SecretSpec](https://secretspec.dev/) provider. If no default provider is configured, Agentaps prompts you to choose one; `secretspec config global init` configures a persistent default. Keep unused pairing links private, and revoke a linked browser from the desktop pairing view when needed. Closing Agentaps ends browser access until you run it again.

See the [Web Connect guide](docs/web-connect.md) for setup, storage, revocation, and current browser limitations.

## Build from source

On a system with Rust and GPUI's native build dependencies, install from crates.io:

```sh
cargo install agentaps
```

Theme conversion currently uses a pinned upstream `native-theme-gpui` Git revision for GPUI Kit 0.7 support, ahead of its crates.io release. GTK/Qt probing stays in Agentaps.

On Linux, fontconfig, FreeType, GTK 4.10 or later, and Qt 6 Widgets development files must be available to `pkg-config`. Ubuntu 24.04 provides these through `libfontconfig-dev`, `libfreetype6-dev`, `libgtk-4-dev`, and `qt6-base-dev`. To run this checkout with [devenv](https://devenv.sh/) installed:

```sh
devenv shell cargo run --release
```

The development environment provides Rust, GPUI's native libraries, and Node for optional ACP adapters. To build the static website and Web Connect UI in `web/`:

```sh
devenv shell -- bash web/build.sh
```

The output is in `web/dist/`. See the [web deployment guide](docs/web-deployment.md) for hosting and local testing, and the [desktop build guide](docs/releasing.md) for package builds and releases.

For local Claude ACP sessions, Agentaps uses the Claude Code CLI found on `PATH` or at `CLAUDE_CODE_EXECUTABLE`. When a CLI path is available, Agentaps omits the inherited `ANTHROPIC_API_KEY` so Claude Code can use its configured authentication.

## Current limitations

- Web Connect shows the latest 100 messages per session and shortens long messages. It does not yet offer diff review or ACP form questions.
- URL based elicitation, ACP client file system and terminal methods, and built in authentication are not yet supported. Agents that require those client features may not work.
- Session data is stored at `$XDG_CONFIG_HOME/agentaps/config.json`, or `~/.config/agentaps/config.json` when `XDG_CONFIG_HOME` is unset, on Linux and macOS. Windows uses its roaming application data directory (`%APPDATA%/agentaps/config.json`). Existing configurations migrate automatically. Closing the app can interrupt an active turn.

## License

Apache-2.0. See [LICENSE](LICENSE).

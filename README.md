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
- **Chat controls:** Send with **Enter**, insert a newline with **Ctrl+Enter**, stop an active turn, or queue messages while the agent connects or works. Use **Up/Down** in the composer to recall earlier prompts. Type `/` to find agent commands, use **Up/Down** to choose one, and complete it with **Tab** or **Enter**. Start a message with `!` to ask the agent to run the following text as a shell command. A `shell` label appears inside the composer.
- **Structured questions:** Answer, decline, or cancel ACP form questions in conversation cards. A status dot and count show pending questions in the sidebar while the agent continues working.
- **Mobile browser preview:** Open a paired browser page to read active conversations, send prompts, stop turns, and answer ACP permission requests through Iroh.
- **SSH projects:** Enter `ssh://user@host/absolute/path` as a project path to run an ACP agent on a server over SSH.

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

## Mobile browser preview

The browser UI is a separate WebAssembly build in `web/`. It needs a static web host to load its files. The browser then connects directly to the running desktop app through Iroh. Agent processes and project files stay on the desktop computer.

The browser preview uses WebGPU when available and falls back to WebGL2 if WebGPU cannot initialize.

Install the `wasm32-unknown-unknown` Rust target, Trunk, and Clang, then build the static site:

```sh
cd web
CC_wasm32_unknown_unknown=clang trunk build --release
```

Publish `web/dist/` at `https://agentaps.dev/`. The desktop pairing link points there by default. Set `AGENTAPS_WEB_URL` to another site URL if you host it elsewhere, or to `http://localhost:8080/` when testing with `trunk serve` on the same computer. Select the phone icon next to Archive in the desktop sidebar to start Iroh access. Agentaps shows a large QR code across the main window; press Escape or click **Close** when done. On your phone, visit the site and allow camera access to scan the desktop QR code. You can also open the copied pairing link directly. The site in the QR code must match the site open on your phone.

The pairing link contains a one time enrollment secret that grants access until it is used. Keep unused links private. The desktop app stores its Iroh identity and access token through your user-global SecretSpec provider. Configure a provider that supports reading and writing with `secretspec config global init`. If that default is missing or fails, Agentaps shows the configuration path it checked and lets you choose a provider for this run. You can choose the system keyring, 1Password, or enter another SecretSpec provider name or URI. This choice is not saved as a new default; select the same provider on the next launch to keep existing phone pairings. The stored value is hex text so text-based providers can hold it.

Agentaps creates the desktop credentials in the configured provider on first use and reads them from there on later launches. After scanning the QR code or opening the pairing link on the phone, choose **Pair with phone unlock** or **Pair with passphrase**. Phone unlock creates a platform passkey and uses its WebAuthn PRF output to encrypt the saved connection. The system may verify you with a fingerprint, face scan, or device PIN. If the browser or passkey does not support PRF, use a unique passphrase of at least 15 characters instead. When a link is opened, the page removes the secret from the address bar. When scanned in the page, the secret never enters the address bar. The page exchanges the one time enrollment secret for an access token, and saves that token and the desktop's public Iroh ID as an AES-GCM encrypted record in browser storage. The used pairing link then expires.

Later, visit `https://agentaps.dev/`. The first page shows desktops saved in this browser and **Pair another desktop**. You can rename each saved desktop. Choose one to unlock it: phone unlock requests system verification, while passphrase unlock shows a field and **Unlock** button. Agentaps connects as soon as the selected connection is unlocked. The page locks when hidden or on reload. If browser storage is cleared, the passkey becomes unavailable, or you forget the passphrase, pair again from the desktop. To change protection methods, open a new desktop pairing link and choose the other method. Pair on the final HTTPS site because browser storage and passkeys belong to that site's origin.

The chosen unlock method protects the saved token if someone gets your phone. It does not protect a session already open in the browser, or a phone whose passphrase or device PIN is known to the person holding it. Keep each new pairing link private until it has been used.

The saved desktop list belongs to this browser and site origin. The desktop currently uses one shared mobile access token, so it cannot list or revoke individual phones.

Closing Agentaps ends mobile access until the app runs again. To rotate credentials and revoke pairing links, close Agentaps and remove the `MOBILE_CREDENTIALS` entry for project `agentaps` and profile `default` from the provider you used before starting it again.

The phone browser uses an Iroh relay, so both devices need network access to a compatible relay. On the agents page, **New** starts a session by choosing a project and an agent. **Other project** accepts a local absolute path or an `ssh://host/absolute/path` URL, and **Custom ACP command** accepts an executable with arguments. The mobile preview shows the latest 100 messages per session, shortens long messages, and does not yet offer diff review or ACP form questions.

## SSH projects

Choose **New**, enter a project path such as `ssh://user@server.example/home/user/project`, then choose an ACP adapter or enter its command. Agentaps starts that command on the server through `ssh -T` and uses the server path as the ACP working directory. The agent and adapter must be installed on the server. Agentaps uses your SSH configuration and keys, requires a known host key, and does not store SSH credentials. Check that `ssh user@server.example` works before starting a remote session. Diff review is currently available for local projects only.

## Notes

- If a Codex or Claude CLI is installed without an ACP adapter, Agentaps can offer one through `npx` when available. The first launch may download it.
- On NixOS, Agentaps points `claude-agent-acp` at an installed `claude` executable. Set `CLAUDE_CODE_EXECUTABLE` to override this.
- Session data is stored in `$XDG_CONFIG_HOME/agentaps/config.json`, or `~/.config/agentaps/config.json` if `XDG_CONFIG_HOME` is unset. Closing the app may interrupt an active turn.
- URL-based elicitation, ACP client file system and terminal methods, and a built-in authentication flow are not yet supported. Agents that require those client features may not work.

## License

Apache-2.0. See [LICENSE](LICENSE).

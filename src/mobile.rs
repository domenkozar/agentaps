use agentaps_control_protocol::{ALPN, Command, MAX_REQUEST_BYTES, Request, Response};
use etcetera::app_strategy::{AppStrategy, AppStrategyArgs, choose_app_strategy};
use iroh::{
    Endpoint, SecretKey,
    endpoint::{Connection, SendStream, presets},
};
use secretspec::{NamedResolution, Secret, SecretBytes, Secrets, Spec};
use std::{
    fs,
    path::{Path, PathBuf},
    sync::{
        Arc, Mutex,
        mpsc::{self, Receiver},
    },
    thread,
    time::Duration,
};
use subtle::ConstantTimeEq;

pub struct Server {
    pub commands: Receiver<PendingCommand>,
    pub snapshot: Arc<Mutex<Response>>,
    pub status: Receiver<Result<String, String>>,
    pub pairing_token: Arc<Mutex<String>>,
}

pub struct PendingCommand {
    pub command: Command,
    pub reply: tokio::sync::oneshot::Sender<Response>,
}

const CREDENTIAL_NAME: &str = "MOBILE_CREDENTIALS";
const CONFIGURE_PROVIDER: &str = "Configure a user-global SecretSpec provider with `secretspec config global init`, then retry Mobile";

fn random_token() -> String {
    hex::encode(SecretKey::generate().to_bytes())
}

fn consume_pairing_token(current: &Mutex<String>, attempt: &str) -> bool {
    let mut current = current.lock().unwrap();
    if !bool::from(attempt.as_bytes().ct_eq(current.as_bytes())) {
        return false;
    }
    *current = random_token();
    true
}

fn global_config_path() -> Result<PathBuf, String> {
    let strategy = choose_app_strategy(AppStrategyArgs {
        top_level_domain: String::new(),
        author: String::new(),
        app_name: "secretspec".into(),
    })
    .map_err(|error| error.to_string())?;
    Ok(strategy.config_dir().join("config.toml"))
}

fn configured_provider_at(path: &Path) -> Result<String, String> {
    let content = match fs::read_to_string(path) {
        Ok(content) => content,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return Err(CONFIGURE_PROVIDER.into());
        }
        Err(error) => return Err(format!("Could not read SecretSpec configuration: {error}")),
    };
    let config: toml::Value = toml::from_str(&content)
        .map_err(|error| format!("Invalid SecretSpec configuration: {error}"))?;
    config
        .get("defaults")
        .and_then(|defaults| defaults.get("provider"))
        .and_then(toml::Value::as_str)
        .map(str::trim)
        .filter(|provider| !provider.is_empty())
        .map(str::to_owned)
        .ok_or_else(|| CONFIGURE_PROVIDER.into())
}

fn base_credential_store() -> Result<Secrets, String> {
    let spec = Spec::builder("agentaps")
        .secret(
            CREDENTIAL_NAME,
            Secret::optional("Iroh identity and mobile access token"),
        )
        .build()
        .map_err(|error| error.to_string())?;
    let mut store = Secrets::from_spec(spec)
        .map_err(|error| error.to_string())?
        .with_reason("Start Agentaps mobile access");
    store.set_profile("default");
    store.set_ignore_ambient_scope(true);
    Ok(store)
}

#[cfg(test)]
fn credential_store_with_provider(provider: &str) -> Result<Secrets, String> {
    let mut store = base_credential_store()?;
    store.set_provider(provider);
    Ok(store)
}

fn credential_store_at(path: &Path) -> Result<Secrets, String> {
    let mut store = base_credential_store()?;
    let provider = configured_provider_at(path)?;
    store.set_provider(provider);
    Ok(store)
}

fn credential_store() -> Result<Secrets, String> {
    credential_store_at(&global_config_path()?)
}

fn read_credentials(store: &Secrets) -> Result<Option<SecretBytes>, String> {
    match store
        .resolve_named_bytes(CREDENTIAL_NAME)
        .map_err(|error| error.to_string())?
    {
        NamedResolution::Resolved(secret) => secret
            .value
            .map(Some)
            .ok_or_else(|| "Mobile credentials were returned as a file path".into()),
        NamedResolution::Missing { .. } => Ok(None),
        NamedResolution::Undeclared => Err("Mobile credentials are not declared".into()),
    }
}

fn decode_credentials(stored: SecretBytes) -> Result<SecretBytes, String> {
    if stored.expose_secret().len() != 128 {
        return Err("Invalid mobile credentials in SecretSpec provider".into());
    }
    let decoded = hex::decode(stored.expose_secret())
        .map_err(|_| "Invalid mobile credentials in SecretSpec provider".to_string())?;
    Ok(SecretBytes::from_vec(decoded))
}

fn save_credentials(store: &Secrets, bytes: &SecretBytes) -> Result<(), String> {
    store
        .set(
            CREDENTIAL_NAME,
            SecretBytes::from_utf8(hex::encode(bytes.expose_secret())),
        )
        .map_err(|error| error.to_string())?;
    let saved = read_credentials(store)?.ok_or("Mobile credentials were not saved")?;
    let saved = decode_credentials(saved)?;
    if saved.expose_secret() != bytes.expose_secret() {
        return Err("Mobile credentials did not match after saving".into());
    }
    Ok(())
}

fn credentials_with_store(store: &Secrets) -> Result<(SecretKey, String), String> {
    let bytes = if let Some(stored) = read_credentials(store)? {
        decode_credentials(stored)?
    } else {
        let mut bytes = Vec::with_capacity(64);
        bytes.extend_from_slice(&SecretKey::generate().to_bytes());
        bytes.extend_from_slice(&SecretKey::generate().to_bytes());
        let bytes = SecretBytes::from_vec(bytes);
        save_credentials(store, &bytes)?;
        bytes
    };
    let secret = SecretKey::from_bytes(bytes.expose_secret()[..32].try_into().unwrap());
    let token = bytes.expose_secret()[32..]
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect();
    Ok((secret, token))
}

fn credentials() -> Result<(SecretKey, String), String> {
    let store = credential_store()?;
    credentials_with_store(&store)
}

async fn send_response(connection: Connection, mut send: SendStream, response: Response) {
    let Ok(bytes) = serde_json::to_vec(&response) else {
        eprintln!("Mobile response serialization failed");
        return;
    };
    if let Err(error) = send.write_all(&bytes).await {
        eprintln!("Mobile response write failed: {error}");
        return;
    }
    if let Err(error) = send.finish() {
        eprintln!("Mobile response finish failed: {error}");
        return;
    }
    // The browser closes the connection after reading the complete response.
    let _ = tokio::time::timeout(Duration::from_secs(30), connection.closed()).await;
}

pub fn start() -> Result<Server, String> {
    let (secret, token) = credentials()?;
    let (commands_tx, commands) = mpsc::channel();
    let (status_tx, status) = mpsc::channel();
    let snapshot = Arc::new(Mutex::new(Response::Snapshot {
        projects: Vec::new(),
        agent_options: Vec::new(),
    }));
    let shared_snapshot = snapshot.clone();
    let shared_token = token.clone();
    let pairing_token = Arc::new(Mutex::new(random_token()));
    let shared_pairing_token = pairing_token.clone();
    thread::spawn(move || {
        let ready_tx = status_tx.clone();
        let result = tokio::runtime::Builder::new_multi_thread()
            .enable_all()
            .build()
            .map_err(|error| error.to_string())
            .and_then(|runtime| {
                runtime.block_on(async move {
                    let endpoint = Endpoint::builder(presets::N0)
                        .secret_key(secret)
                        .alpns(vec![ALPN.to_vec()])
                        .bind()
                        .await
                        .map_err(|error| error.to_string())?;
                    let endpoint_id = endpoint.id().to_string();
                    let _ = ready_tx.send(Ok(endpoint_id.clone()));
                    while let Some(incoming) = endpoint.accept().await {
                        let tx = commands_tx.clone();
                        let snapshot = shared_snapshot.clone();
                        let token = shared_token.clone();
                        let pairing_token = shared_pairing_token.clone();
                        let paired_tx = ready_tx.clone();
                        let endpoint_id = endpoint_id.clone();
                        tokio::spawn(async move {
                            let connection = match incoming.await {
                                Ok(connection) => connection,
                                Err(error) => {
                                    eprintln!("Mobile connection handshake failed: {error}");
                                    return;
                                }
                            };
                            let (send, mut recv) = match connection.accept_bi().await {
                                Ok(stream) => stream,
                                Err(error) => {
                                    eprintln!("Mobile request stream failed: {error}");
                                    return;
                                }
                            };
                            let response = match recv.read_to_end(MAX_REQUEST_BYTES).await {
                                Ok(bytes) => match serde_json::from_slice::<Request>(&bytes) {
                                    Ok(request) if matches!(request.command, Command::Pair) => {
                                        if consume_pairing_token(&pairing_token, &request.token) {
                                            eprintln!("Mobile pairing accepted");
                                            let _ = paired_tx.send(Ok(endpoint_id));
                                            Response::Paired { token }
                                        } else {
                                            eprintln!("Mobile pairing rejected: expired link");
                                            Response::Error { message: "Pairing link has expired. Copy a new one from desktop Agentaps.".into() }
                                        }
                                    }
                                    Ok(request) if bool::from(request.token.as_bytes().ct_eq(token.as_bytes())) => {
                                        match request.command {
                                            Command::Pair => unreachable!(),
                                            Command::Snapshot => snapshot.lock().unwrap().clone(),
                                            command => {
                                                let (reply, answer) =
                                                    tokio::sync::oneshot::channel();
                                                match tx.send(PendingCommand { command, reply }) {
                                                    Ok(()) => {
                                                        answer.await.unwrap_or(Response::Error {
                                                            message: "Desktop session closed"
                                                                .into(),
                                                        })
                                                    }
                                                    Err(_) => Response::Error {
                                                        message: "Desktop session closed".into(),
                                                    },
                                                }
                                            }
                                        }
                                    }
                                    Ok(_) => Response::Error {
                                        message: "Access denied".into(),
                                    },
                                    Err(error) => Response::Error {
                                        message: format!("Invalid request: {error}"),
                                    },
                                },
                                Err(error) => Response::Error {
                                    message: format!("Could not read request: {error}"),
                                },
                            };
                            send_response(connection, send, response).await;
                        });
                    }
                    Ok(())
                })
            });
        if let Err(error) = result {
            let _ = status_tx.send(Err(error));
        }
    });
    Ok(Server {
        commands,
        snapshot,
        status,
        pairing_token,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn file_store(root: &Path) -> Secrets {
        credential_store_with_provider(&format!("file:{}", root.display())).unwrap()
    }

    #[test]
    fn requires_a_user_global_provider() {
        let temp = tempfile::tempdir().unwrap();
        let config = temp.path().join("config.toml");
        assert!(
            configured_provider_at(&config)
                .unwrap_err()
                .contains("secretspec config global init")
        );
        fs::write(&config, "[defaults]\nprofile = 'default'\n").unwrap();
        assert!(
            configured_provider_at(&config)
                .unwrap_err()
                .contains("secretspec config global init")
        );
        fs::write(&config, "[defaults]\nprovider = 'onepassword'\n").unwrap();
        assert_eq!(configured_provider_at(&config).unwrap(), "onepassword");
    }

    #[test]
    fn creates_and_reuses_credentials_in_configured_provider() {
        let temp = tempfile::tempdir().unwrap();
        let config_path = temp.path().join("config.toml");
        let provider_root = temp.path().join("provider");
        fs::write(
            &config_path,
            format!(
                "[defaults]\nprovider = 'file:{}'\n",
                provider_root.display()
            ),
        )
        .unwrap();
        let store = credential_store_at(&config_path).unwrap();

        let (key, token) = credentials_with_store(&store).unwrap();
        let (saved_key, saved_token) = credentials_with_store(&store).unwrap();
        assert_eq!(saved_key.to_bytes(), key.to_bytes());
        assert_eq!(saved_token, token);
        let stored = read_credentials(&store).unwrap().unwrap();
        assert_eq!(stored.expose_secret().len(), 128);
        assert!(stored.expose_secret().iter().all(u8::is_ascii_hexdigit));
    }

    #[test]
    fn deleting_configured_provider_entry_rotates_pairing_credentials() {
        let temp = tempfile::tempdir().unwrap();
        let store = file_store(temp.path());
        let (old_key, old_token) = credentials_with_store(&store).unwrap();
        store.delete(CREDENTIAL_NAME).unwrap();

        let (new_key, new_token) = credentials_with_store(&store).unwrap();
        assert_ne!(old_key.to_bytes(), new_key.to_bytes());
        assert_ne!(old_token, new_token);
    }

    #[test]
    fn pairing_token_can_be_used_only_once() {
        let initial = random_token();
        let current = Mutex::new(initial.clone());
        assert!(!consume_pairing_token(&current, "wrong"));
        assert!(consume_pairing_token(&current, &initial));
        assert!(!consume_pairing_token(&current, &initial));
        assert_ne!(*current.lock().unwrap(), initial);
    }

    #[test]
    fn response_reaches_client_before_connection_closes() {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        runtime.block_on(async {
            tokio::time::timeout(Duration::from_secs(10), async {
                let server = Endpoint::builder(presets::Minimal)
                    .alpns(vec![ALPN.to_vec()])
                    .bind()
                    .await
                    .unwrap();
                let client = Endpoint::bind(presets::Minimal).await.unwrap();
                let server_addr = server.addr();
                let server_task = tokio::spawn({
                    let server = server.clone();
                    async move {
                        let connection = server.accept().await.unwrap().await.unwrap();
                        let (send, mut recv) = connection.accept_bi().await.unwrap();
                        assert_eq!(recv.read_to_end(64).await.unwrap(), b"request");
                        send_response(connection, send, Response::Accepted).await;
                    }
                });
                let connection = client.connect(server_addr, ALPN).await.unwrap();
                let (mut send, mut recv) = connection.open_bi().await.unwrap();
                send.write_all(b"request").await.unwrap();
                send.finish().unwrap();
                let bytes = recv.read_to_end(1024).await.unwrap();
                assert!(matches!(
                    serde_json::from_slice::<Response>(&bytes).unwrap(),
                    Response::Accepted
                ));
                connection.close(0u32.into(), b"done");
                server_task.await.unwrap();
                client.close().await;
                server.close().await;
            })
            .await
            .expect("local mobile response timed out");
        });
    }
}

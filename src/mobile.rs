use agentaps_control_protocol::{ALPN, Command, MAX_REQUEST_BYTES, Request, Response};
use etcetera::app_strategy::{AppStrategy, AppStrategyArgs, choose_app_strategy};
use iroh::{
    Endpoint, SecretKey,
    endpoint::{Connection, SendStream, presets},
};
use secretspec::{NamedResolution, Secret, SecretBytes, Secrets, Spec};
use serde::{Deserialize, Serialize};
use std::{
    fs,
    path::{Path, PathBuf},
    sync::{
        Arc, Mutex,
        mpsc::{self, Receiver},
    },
    thread,
    time::{Duration, SystemTime, UNIX_EPOCH},
};
use subtle::ConstantTimeEq;

pub struct Server {
    pub commands: Receiver<PendingCommand>,
    pub snapshot: Arc<Mutex<Response>>,
    pub status: Receiver<Result<String, String>>,
    pub pairing_token: Arc<Mutex<String>>,
    clients: Arc<Mutex<Credentials>>,
    provider: String,
    revoke_tx: mpsc::Sender<Result<String, String>>,
    pub revoke_results: Receiver<Result<String, String>>,
}

#[derive(Clone, Debug)]
pub struct ClientSummary {
    pub id: String,
    pub name: String,
    pub paired_at: Option<u64>,
}

pub struct PendingCommand {
    pub command: Command,
    pub reply: tokio::sync::oneshot::Sender<Response>,
}

const CREDENTIAL_NAME: &str = "MOBILE_CREDENTIALS";
pub const LEGACY_CLIENT_ID: &str = "legacy";
const CONFIGURE_PROVIDER: &str =
    "Run `secretspec config global init` to save a default provider, or choose one for this run";

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
struct LinkedClient {
    id: String,
    name: String,
    token: String,
    paired_at: u64,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
struct Credentials {
    version: u8,
    secret_key: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    legacy_token: Option<String>,
    #[serde(default)]
    clients: Vec<LinkedClient>,
}

impl Credentials {
    fn new() -> Self {
        Self {
            version: 1,
            secret_key: random_token(),
            legacy_token: None,
            clients: Vec::new(),
        }
    }

    fn secret_key(&self) -> SecretKey {
        let bytes = hex::decode(&self.secret_key).unwrap();
        SecretKey::from_bytes(bytes.as_slice().try_into().unwrap())
    }

    fn authorizes(&self, token: &str) -> bool {
        self.legacy_token
            .iter()
            .chain(self.clients.iter().map(|client| &client.token))
            .fold(false, |authorized, candidate| {
                authorized | bool::from(token.as_bytes().ct_eq(candidate.as_bytes()))
            })
    }

    fn linked_clients(&self) -> Vec<ClientSummary> {
        let mut clients =
            Vec::with_capacity(self.clients.len() + usize::from(self.legacy_token.is_some()));
        if self.legacy_token.is_some() {
            clients.push(ClientSummary {
                id: LEGACY_CLIENT_ID.into(),
                name: "Previously paired browsers".into(),
                paired_at: None,
            });
        }
        clients.extend(self.clients.iter().map(|client| ClientSummary {
            id: client.id.clone(),
            name: client.name.clone(),
            paired_at: Some(client.paired_at),
        }));
        clients
    }
}

fn missing_provider(path: &Path) -> String {
    format!(
        "No default SecretSpec provider found in {}. {CONFIGURE_PROVIDER}",
        path.display()
    )
}

fn random_token() -> String {
    hex::encode(SecretKey::generate().to_bytes())
}

fn client_name(name: Option<&str>) -> String {
    name.map(|name| {
        name.chars()
            .filter(|character| !character.is_control())
            .take(48)
            .collect::<String>()
            .trim()
            .to_owned()
    })
    .filter(|name| !name.is_empty())
    .unwrap_or_else(|| "Browser".into())
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
            return Err(missing_provider(path));
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
        .ok_or_else(|| missing_provider(path))
}

fn base_credential_store() -> Result<Secrets, String> {
    let spec = Spec::builder("agentaps")
        .secret(
            CREDENTIAL_NAME,
            Secret::optional("Iroh identity and linked mobile clients"),
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

fn credential_store_with_provider(provider: &str) -> Result<Secrets, String> {
    let mut store = base_credential_store()?;
    store.set_provider(provider);
    Ok(store)
}

#[cfg(test)]
fn credential_store_at(path: &Path) -> Result<Secrets, String> {
    let mut store = base_credential_store()?;
    let provider = configured_provider_at(path)?;
    store.set_provider(provider);
    Ok(store)
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

fn valid_token(token: &str) -> bool {
    token.len() == 64 && token.bytes().all(|byte| byte.is_ascii_hexdigit())
}

fn decode_credentials(stored: SecretBytes) -> Result<Credentials, String> {
    let bytes = stored.expose_secret();
    let credentials = if bytes.len() == 128 && bytes.iter().all(u8::is_ascii_hexdigit) {
        let value = std::str::from_utf8(bytes)
            .map_err(|_| "Invalid mobile credentials in SecretSpec provider")?;
        Credentials {
            version: 1,
            secret_key: value[..64].into(),
            legacy_token: Some(value[64..].into()),
            clients: Vec::new(),
        }
    } else {
        serde_json::from_slice::<Credentials>(bytes)
            .map_err(|_| "Invalid mobile credentials in SecretSpec provider")?
    };
    if credentials.version != 1
        || !valid_token(&credentials.secret_key)
        || credentials
            .legacy_token
            .as_deref()
            .is_some_and(|token| !valid_token(token))
        || credentials.clients.iter().any(|client| {
            client.id.is_empty()
                || client.id == LEGACY_CLIENT_ID
                || client.name.is_empty()
                || !valid_token(&client.token)
        })
    {
        return Err("Invalid mobile credentials in SecretSpec provider".into());
    }
    Ok(credentials)
}

fn save_credentials(store: &Secrets, credentials: &Credentials) -> Result<(), String> {
    let bytes = serde_json::to_vec(credentials).map_err(|error| error.to_string())?;
    store
        .set(CREDENTIAL_NAME, SecretBytes::from_vec(bytes))
        .map_err(|error| error.to_string())?;
    let saved = read_credentials(store)?.ok_or("Mobile credentials were not saved")?;
    let saved = decode_credentials(saved)?;
    if &saved != credentials {
        return Err("Mobile credentials did not match after saving".into());
    }
    Ok(())
}

fn credentials_with_store(store: &Secrets) -> Result<(SecretKey, Credentials), String> {
    let credentials = if let Some(stored) = read_credentials(store)? {
        decode_credentials(stored)?
    } else {
        let credentials = Credentials::new();
        save_credentials(store, &credentials)?;
        credentials
    };
    Ok((credentials.secret_key(), credentials))
}

fn credentials(provider: Option<&str>) -> Result<(SecretKey, Credentials, String), String> {
    let provider = match provider {
        Some(provider) => provider.to_owned(),
        None => configured_provider_at(&global_config_path()?)?,
    };
    let store = credential_store_with_provider(&provider)?;
    let (secret, credentials) = credentials_with_store(&store)?;
    Ok((secret, credentials, provider))
}

fn pair_client(
    credentials: &Mutex<Credentials>,
    pairing_token: &Mutex<String>,
    provider: &str,
    attempt: &str,
    name: Option<&str>,
) -> Result<String, String> {
    let mut pairing_token = pairing_token.lock().unwrap();
    if !bool::from(attempt.as_bytes().ct_eq(pairing_token.as_bytes())) {
        return Err("Pairing link has expired. Copy a new one from desktop Agentaps.".into());
    }
    let mut current = credentials.lock().unwrap();
    let mut updated = current.clone();
    let id = loop {
        let candidate = random_token()[..16].to_owned();
        if !updated.clients.iter().any(|client| client.id == candidate) {
            break candidate;
        }
    };
    let token = random_token();
    updated.clients.push(LinkedClient {
        id,
        name: client_name(name),
        token: token.clone(),
        paired_at: SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs(),
    });
    let store = credential_store_with_provider(provider)?;
    save_credentials(&store, &updated)?;
    *current = updated;
    *pairing_token = random_token();
    Ok(token)
}

fn revoke_client(credentials: &Mutex<Credentials>, provider: &str, id: &str) -> Result<(), String> {
    let mut current = credentials.lock().unwrap();
    let mut updated = current.clone();
    let found = if id == LEGACY_CLIENT_ID {
        updated.legacy_token.take().is_some()
    } else {
        let previous = updated.clients.len();
        updated.clients.retain(|client| client.id != id);
        previous != updated.clients.len()
    };
    if !found {
        return Err("Linked client was already removed".into());
    }
    let store = credential_store_with_provider(provider)?;
    save_credentials(&store, &updated)?;
    *current = updated;
    Ok(())
}

impl Server {
    pub fn linked_clients(&self) -> Vec<ClientSummary> {
        self.clients.lock().unwrap().linked_clients()
    }

    pub fn revoke_client(&self, id: String) {
        let credentials = self.clients.clone();
        let provider = self.provider.clone();
        let sender = self.revoke_tx.clone();
        thread::spawn(move || {
            let result = revoke_client(&credentials, &provider, &id).map(|()| id);
            let _ = sender.send(result);
        });
    }
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
    start_with_provider(None)
}

pub fn start_with_provider(provider: Option<&str>) -> Result<Server, String> {
    let (secret, credentials, provider) = credentials(provider)?;
    let (commands_tx, commands) = mpsc::channel();
    let (status_tx, status) = mpsc::channel();
    let (revoke_tx, revoke_results) = mpsc::channel();
    let snapshot = Arc::new(Mutex::new(Response::Snapshot {
        projects: Vec::new(),
        agent_options: Vec::new(),
    }));
    let shared_snapshot = snapshot.clone();
    let clients = Arc::new(Mutex::new(credentials));
    let shared_clients = clients.clone();
    let shared_provider = provider.clone();
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
                        let clients = shared_clients.clone();
                        let provider = shared_provider.clone();
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
                                        let result = tokio::task::spawn_blocking(move || {
                                            pair_client(
                                                &clients,
                                                &pairing_token,
                                                &provider,
                                                &request.token,
                                                request.client_name.as_deref(),
                                            )
                                        })
                                        .await;
                                        match result {
                                            Ok(Ok(token)) => {
                                                eprintln!("Mobile pairing accepted");
                                                let _ = paired_tx.send(Ok(endpoint_id));
                                                Response::Paired { token }
                                            }
                                            Ok(Err(message)) => Response::Error { message },
                                            Err(error) => Response::Error {
                                                message: format!(
                                                    "Could not save linked client: {error}"
                                                ),
                                            },
                                        }
                                    }
                                    Ok(request)
                                        if clients.lock().unwrap().authorizes(&request.token) =>
                                    {
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
        clients,
        provider,
        revoke_tx,
        revoke_results,
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
        assert!(
            configured_provider_at(&config)
                .unwrap_err()
                .contains(&config.display().to_string())
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

        let (key, credentials) = credentials_with_store(&store).unwrap();
        let (saved_key, saved_credentials) = credentials_with_store(&store).unwrap();
        assert_eq!(saved_key.to_bytes(), key.to_bytes());
        assert_eq!(saved_credentials, credentials);
        let stored = read_credentials(&store).unwrap().unwrap();
        assert_eq!(decode_credentials(stored).unwrap(), credentials);
        assert!(credentials.clients.is_empty());
        assert!(credentials.legacy_token.is_none());
    }

    #[test]
    fn ad_hoc_provider_creates_and_reuses_credentials() {
        let temp = tempfile::tempdir().unwrap();
        let provider = format!("file:{}", temp.path().display());
        let (key, initial_credentials, _) = credentials(Some(&provider)).unwrap();
        let (saved_key, saved_credentials, _) = credentials(Some(&provider)).unwrap();
        assert_eq!(saved_key.to_bytes(), key.to_bytes());
        assert_eq!(saved_credentials, initial_credentials);
    }

    #[test]
    fn deleting_configured_provider_entry_rotates_pairing_credentials() {
        let temp = tempfile::tempdir().unwrap();
        let store = file_store(temp.path());
        let (old_key, _) = credentials_with_store(&store).unwrap();
        store.delete(CREDENTIAL_NAME).unwrap();

        let (new_key, _) = credentials_with_store(&store).unwrap();
        assert_ne!(old_key.to_bytes(), new_key.to_bytes());
    }

    #[test]
    fn linked_clients_have_individual_revocable_tokens() {
        let temp = tempfile::tempdir().unwrap();
        let provider = format!("file:{}", temp.path().display());
        let store = file_store(temp.path());
        let (_, credentials) = credentials_with_store(&store).unwrap();
        let clients = Mutex::new(credentials);
        let initial = random_token();
        let pairing = Mutex::new(initial.clone());
        assert!(pair_client(&clients, &pairing, &provider, "wrong", None).is_err());
        assert_eq!(*pairing.lock().unwrap(), initial);

        let first = pair_client(
            &clients,
            &pairing,
            &provider,
            &initial,
            Some("Android browser"),
        )
        .unwrap();
        assert!(pair_client(&clients, &pairing, &provider, &initial, None).is_err());
        let second_pairing = pairing.lock().unwrap().clone();
        let second = pair_client(&clients, &pairing, &provider, &second_pairing, None).unwrap();
        let first_id = clients.lock().unwrap().clients[0].id.clone();
        assert!(clients.lock().unwrap().authorizes(&first));
        assert!(clients.lock().unwrap().authorizes(&second));
        assert_eq!(
            clients.lock().unwrap().linked_clients()[0].name,
            "Android browser"
        );

        revoke_client(&clients, &provider, &first_id).unwrap();
        assert!(!clients.lock().unwrap().authorizes(&first));
        assert!(clients.lock().unwrap().authorizes(&second));
        let (_, saved) = credentials_with_store(&store).unwrap();
        assert_eq!(saved, *clients.lock().unwrap());
    }

    #[test]
    fn older_shared_token_is_shown_and_can_be_revoked_as_a_group() {
        let temp = tempfile::tempdir().unwrap();
        let provider = format!("file:{}", temp.path().display());
        let store = file_store(temp.path());
        let legacy_token = random_token();
        store
            .set(
                CREDENTIAL_NAME,
                SecretBytes::from_utf8(format!("{}{}", random_token(), legacy_token)),
            )
            .unwrap();
        let (_, credentials) = credentials_with_store(&store).unwrap();
        assert!(credentials.authorizes(&legacy_token));
        assert_eq!(credentials.linked_clients()[0].id, LEGACY_CLIENT_ID);
        let clients = Mutex::new(credentials);
        let pairing_token = random_token();
        let new_token = pair_client(
            &clients,
            &Mutex::new(pairing_token.clone()),
            &provider,
            &pairing_token,
            None,
        )
        .unwrap();
        revoke_client(&clients, &provider, LEGACY_CLIENT_ID).unwrap();
        assert!(!clients.lock().unwrap().authorizes(&legacy_token));
        assert!(clients.lock().unwrap().authorizes(&new_token));
        let (_, saved) = credentials_with_store(&store).unwrap();
        assert!(saved.legacy_token.is_none());
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

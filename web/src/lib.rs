use agentaps_control_protocol::{
    ALPN, Agent, AgentOption, Command, MAX_RESPONSE_BYTES, Project, Request, Response,
};
use async_channel::{Receiver, Sender};
use gpui::{
    App, ApplicationHandle, Context, Entity, IntoElement, Render, ScrollHandle,
    StatefulInteractiveElement, Window, WindowOptions, div, prelude::*, px, relative, rems, rgb,
};
use gpui_component::{
    ActiveTheme, Root,
    input::{Input, InputEvent, InputState, Textarea, TextareaState},
    text::{TextView, TextViewStyle},
};
use gpui_component_assets::Assets;
use iroh::{Endpoint, EndpointId, endpoint::presets};
use js_sys::{Promise, Reflect, Uint8Array};
use std::{borrow::Cow, cell::RefCell, str::FromStr, time::Duration};
use wasm_bindgen::prelude::*;
use wasm_bindgen_futures::JsFuture;

#[wasm_bindgen(inline_js = r#"
const storageKey = 'agentaps.connection.v1';
const iterations = 600000;
const encoder = new TextEncoder();
const decoder = new TextDecoder();
let pendingPasskey = null;
let qrCamera = null;

function stopCamera(session) {
  session.cancelled = true;
  session.stream?.getTracks().forEach(track => track.stop());
  document.removeEventListener('visibilitychange', session.onVisibilityChange);
  session.overlay.remove();
  if (qrCamera === session) qrCamera = null;
}
export function stopQrCamera() {
  if (qrCamera) stopCamera(qrCamera);
}
export async function startQrCamera() {
  if (!globalThis.isSecureContext || !navigator.mediaDevices?.getUserMedia) {
    throw new Error('Camera access needs HTTPS or localhost in a browser that supports it.');
  }
  stopQrCamera();
  const overlay = document.createElement('div');
  overlay.style.cssText = 'position:fixed;inset:0;z-index:2147483647;background:#10151c;display:flex;flex-direction:column;align-items:center;justify-content:center;gap:16px;padding:24px;color:#edf2f7;font:16px sans-serif;text-align:center';
  const title = document.createElement('div');
  title.textContent = 'Scan the QR code shown in desktop Agentaps';
  const video = document.createElement('video');
  video.autoplay = true;
  video.muted = true;
  video.playsInline = true;
  video.style.cssText = 'width:min(100%,480px);max-height:70vh;object-fit:contain;border-radius:12px;background:#202a36';
  const cancel = document.createElement('button');
  cancel.textContent = 'Cancel';
  cancel.style.cssText = 'border:0;border-radius:8px;background:#304b60;color:#edf2f7;padding:12px 24px;font:inherit;cursor:pointer';
  overlay.append(title, video, cancel);
  document.body.append(overlay);
  const canvas = document.createElement('canvas');
  const context = canvas.getContext('2d', { willReadFrequently: true });
  if (!context) {
    overlay.remove();
    throw new Error('Could not read camera frames');
  }
  const session = { overlay, video, canvas, context, stream: null, cancelled: false };
  session.onVisibilityChange = () => { if (document.hidden) stopCamera(session); };
  document.addEventListener('visibilitychange', session.onVisibilityChange);
  qrCamera = session;
  cancel.addEventListener('click', () => stopCamera(session));
  try {
    const stream = await navigator.mediaDevices.getUserMedia({ audio: false, video: { facingMode: { ideal: 'environment' } } });
    if (session.cancelled) {
      stream.getTracks().forEach(track => track.stop());
      throw new Error('Camera scan cancelled');
    }
    session.stream = stream;
    video.srcObject = stream;
    await video.play();
    if (session.cancelled) throw new Error('Camera scan cancelled');
  } catch (error) {
    stopCamera(session);
    throw error;
  }
}
export async function nextQrFrame() {
  await new Promise(resolve => setTimeout(resolve, 180));
  const session = qrCamera;
  if (!session || session.cancelled) throw new Error('Camera scan cancelled');
  const { video } = session;
  if (session.stream?.getVideoTracks()[0]?.readyState === 'ended') {
    throw new Error('Camera disconnected. Try scanning again.');
  }
  if (!video.videoWidth || !video.videoHeight) return null;
  const width = Math.min(640, video.videoWidth);
  const height = Math.round(video.videoHeight * width / video.videoWidth);
  session.canvas.width = width;
  session.canvas.height = height;
  session.context.drawImage(video, 0, 0, width, height);
  const rgba = session.context.getImageData(0, 0, width, height).data;
  const pixels = new Uint8Array(width * height);
  for (let i = 0, j = 0; i < pixels.length; i++, j += 4) {
    pixels[i] = (rgba[j] * 77 + rgba[j + 1] * 150 + rgba[j + 2] * 29) >> 8;
  }
  return { width, height, pixels };
}

function bytesToHex(bytes) {
  return Array.from(bytes, byte => byte.toString(16).padStart(2, '0')).join('');
}
function hexToBytes(hex) {
  if (!/^(?:[0-9a-f]{2})+$/.test(hex)) throw new Error('Saved connection is damaged');
  return Uint8Array.from(hex.match(/../g), pair => parseInt(pair, 16));
}
async function keyFromPassphrase(passphrase, salt) {
  const material = await crypto.subtle.importKey('raw', encoder.encode(passphrase), 'PBKDF2', false, ['deriveKey']);
  return crypto.subtle.deriveKey(
    { name: 'PBKDF2', hash: 'SHA-256', salt, iterations },
    material, { name: 'AES-GCM', length: 256 }, false, ['encrypt', 'decrypt']
  );
}
async function keyFromPrf(output, salt) {
  const material = await crypto.subtle.importKey('raw', output, 'HKDF', false, ['deriveKey']);
  return crypto.subtle.deriveKey(
    { name: 'HKDF', hash: 'SHA-256', salt, info: encoder.encode('Agentaps phone unlock v2') },
    material, { name: 'AES-GCM', length: 256 }, false, ['encrypt', 'decrypt']
  );
}
function connectionKey(id) {
  if (id === 'default') return storageKey;
  if (!/^[0-9a-f]{64}$/.test(id)) throw new Error('Invalid saved connection');
  return `${storageKey}.${id}`;
}
function readConnection(id) {
  const raw = localStorage.getItem(connectionKey(id));
  if (!raw) throw new Error('No saved connection. Pair with the desktop again.');
  try { return JSON.parse(raw); } catch (_) { throw new Error('Saved connection is damaged'); }
}
async function connectionId(pairing) {
  const endpoint = pairing.split(':', 1)[0];
  const hash = await crypto.subtle.digest('SHA-256', encoder.encode(endpoint));
  return bytesToHex(new Uint8Array(hash));
}
export function savedConnections() {
  const saved = [];
  for (let index = 0; index < localStorage.length; index++) {
    const key = localStorage.key(index);
    if (key !== storageKey && !key?.startsWith(`${storageKey}.`)) continue;
    const id = key === storageKey ? 'default' : key.slice(storageKey.length + 1);
    if (id !== 'default' && !/^[0-9a-f]{64}$/.test(id)) continue;
    try {
      const record = JSON.parse(localStorage.getItem(key));
      if (record.version !== 1 && record.version !== 2) continue;
      saved.push({
        id,
        label: typeof record.label === 'string' ? record.label : 'Saved desktop',
        protection: record.version,
      });
    } catch (_) { /* A damaged record cannot be unlocked. */ }
  }
  saved.sort((a, b) => a.label.localeCompare(b.label));
  return JSON.stringify(saved);
}
export function renameSavedConnection(id) {
  const record = readConnection(id);
  const name = globalThis.prompt('Desktop name', record.label || 'Saved desktop');
  if (name === null) return false;
  const label = name.trim();
  if (!label || Array.from(label).length > 64) return false;
  record.label = label;
  localStorage.setItem(connectionKey(id), JSON.stringify(record));
  return true;
}
export function promptProjectPath() {
  return globalThis.prompt('Project path or ssh://host/absolute/path')?.trim() || null;
}
export function promptAcpCommand() {
  return globalThis.prompt('ACP command and arguments')?.trim() || null;
}
function connectionLabel(keyName, endpoint) {
  try {
    const label = JSON.parse(localStorage.getItem(keyName))?.label;
    if (typeof label === 'string' && label.trim()) return label;
  } catch (_) { /* A damaged record will be replaced when pairing again. */ }
  return `Desktop ${endpoint.slice(0, 8)}`;
}
export function canUsePhoneUnlock() {
  return !!(globalThis.isSecureContext && globalThis.PublicKeyCredential && navigator.credentials?.create && navigator.credentials?.get);
}
export async function saveConnection(passphrase, pairing) {
  if (Array.from(passphrase).length < 15) throw new Error('Use at least 15 characters for the unlock passphrase');
  const id = await connectionId(pairing);
  const keyName = connectionKey(id);
  const endpoint = pairing.split(':', 1)[0];
  const salt = crypto.getRandomValues(new Uint8Array(16));
  const iv = crypto.getRandomValues(new Uint8Array(12));
  const key = await keyFromPassphrase(passphrase, salt);
  const ciphertext = await crypto.subtle.encrypt(
    { name: 'AES-GCM', iv, additionalData: encoder.encode(keyName) },
    key, encoder.encode(pairing)
  );
  localStorage.setItem(keyName, JSON.stringify({
    version: 1, label: connectionLabel(keyName, endpoint),
    salt: bytesToHex(salt), iv: bytesToHex(iv), ciphertext: bytesToHex(new Uint8Array(ciphertext))
  }));
  return id;
}
export async function unlockConnection(passphrase, id) {
  const record = readConnection(id);
  if (record.version !== 1) throw new Error('This connection uses phone unlock');
  const salt = hexToBytes(record.salt);
  const iv = hexToBytes(record.iv);
  const ciphertext = hexToBytes(record.ciphertext);
  if (salt.length !== 16 || iv.length !== 12) throw new Error('Saved connection is damaged');
  try {
    const key = await keyFromPassphrase(passphrase, salt);
    return decoder.decode(await crypto.subtle.decrypt(
      { name: 'AES-GCM', iv, additionalData: encoder.encode(connectionKey(id)) }, key, ciphertext
    ));
  } catch (_) {
    throw new Error('Could not unlock. Check your passphrase.');
  }
}
function base64url(bytes) {
  return btoa(String.fromCharCode(...bytes)).replaceAll('+', '-').replaceAll('/', '_').replace(/=+$/, '');
}
function prfOutput(credential) {
  const output = credential?.getClientExtensionResults()?.prf?.results?.first;
  if (!output || output.byteLength !== 32) {
    throw new Error('This phone or passkey does not support secure phone unlock. Use a passphrase instead.');
  }
  return output;
}
function assertionOptions(id, prfSalt) {
  const credentialId = hexToBytes(id);
  if (!credentialId.length || credentialId.length > 1024) throw new Error('Saved connection is damaged');
  return {
    challenge: crypto.getRandomValues(new Uint8Array(32)),
    allowCredentials: [{ type: 'public-key', id: credentialId }],
    userVerification: 'required',
    extensions: { prf: { evalByCredential: { [base64url(credentialId)]: { first: prfSalt } } } },
  };
}
export async function beginPasskeyEnrollment() {
  if (!canUsePhoneUnlock()) throw new Error('Phone unlock needs a compatible browser on HTTPS. Use a passphrase instead.');
  const prfSalt = crypto.getRandomValues(new Uint8Array(32));
  const creation = navigator.credentials.create({ publicKey: {
    challenge: crypto.getRandomValues(new Uint8Array(32)),
    rp: { name: 'Agentaps' },
    user: {
      id: crypto.getRandomValues(new Uint8Array(16)),
      name: 'Agentaps on this phone', displayName: 'Agentaps on this phone',
    },
    pubKeyCredParams: [{ type: 'public-key', alg: -7 }],
    timeout: 60000,
    authenticatorSelection: {
      authenticatorAttachment: 'platform', residentKey: 'required', userVerification: 'required',
    },
    extensions: { prf: { eval: { first: prfSalt } } },
  }});
  const credential = await creation;
  if (!credential) throw new Error('Phone unlock was cancelled');
  const id = bytesToHex(new Uint8Array(credential.rawId));
  let output = credential.getClientExtensionResults()?.prf?.results?.first;
  if (!output) {
    const assertion = await navigator.credentials.get({ publicKey: assertionOptions(id, prfSalt) });
    output = prfOutput(assertion);
  }
  if (output.byteLength !== 32) throw new Error('This phone or passkey does not support secure phone unlock. Use a passphrase instead.');
  const kdfSalt = crypto.getRandomValues(new Uint8Array(16));
  pendingPasskey = { id, prfSalt: bytesToHex(prfSalt), kdfSalt: bytesToHex(kdfSalt), key: await keyFromPrf(output, kdfSalt) };
}
export async function finishPasskeyEnrollment(pairing) {
  if (!pendingPasskey) throw new Error('Phone unlock setup was interrupted');
  const id = await connectionId(pairing);
  const keyName = connectionKey(id);
  const endpoint = pairing.split(':', 1)[0];
  const prepared = pendingPasskey;
  const iv = crypto.getRandomValues(new Uint8Array(12));
  const ciphertext = await crypto.subtle.encrypt(
    { name: 'AES-GCM', iv, additionalData: encoder.encode(keyName) },
    prepared.key, encoder.encode(pairing)
  );
  localStorage.setItem(keyName, JSON.stringify({
    version: 2, label: connectionLabel(keyName, endpoint),
    credentialId: prepared.id, prfSalt: prepared.prfSalt,
    kdfSalt: prepared.kdfSalt, iv: bytesToHex(iv),
    ciphertext: bytesToHex(new Uint8Array(ciphertext)),
  }));
  pendingPasskey = null;
  return id;
}
export function discardPasskeyEnrollment() {
  pendingPasskey = null;
}
export async function beginPasskeyUnlock(id) {
  if (!canUsePhoneUnlock()) throw new Error('Phone unlock is unavailable in this browser');
  const record = readConnection(id);
  if (record.version !== 2) throw new Error('This connection uses a passphrase');
  const prfSalt = hexToBytes(record.prfSalt);
  const kdfSalt = hexToBytes(record.kdfSalt);
  const iv = hexToBytes(record.iv);
  const ciphertext = hexToBytes(record.ciphertext);
  if (prfSalt.length !== 32 || kdfSalt.length !== 16 || iv.length !== 12) throw new Error('Saved connection is damaged');
  const assertion = await navigator.credentials.get({ publicKey: assertionOptions(record.credentialId, prfSalt) });
  if (!assertion || bytesToHex(new Uint8Array(assertion.rawId)) !== record.credentialId) {
    throw new Error('Wrong phone unlock credential');
  }
  const key = await keyFromPrf(prfOutput(assertion), kdfSalt);
  try {
    return decoder.decode(await crypto.subtle.decrypt(
      { name: 'AES-GCM', iv, additionalData: encoder.encode(connectionKey(id)) }, key, ciphertext
    ));
  } catch (_) { throw new Error('Could not unlock saved connection'); }
}
export function clearPairingHash() {
  history.replaceState(null, '', location.pathname + location.search);
}
export function pageHidden() {
  return document.hidden;
}
"#)]
extern "C" {
    fn savedConnections() -> String;
    fn renameSavedConnection(id: String) -> bool;
    fn promptProjectPath() -> JsValue;
    fn promptAcpCommand() -> JsValue;
    fn canUsePhoneUnlock() -> bool;
    fn clearPairingHash();
    fn pageHidden() -> bool;
    fn stopQrCamera();
    #[wasm_bindgen(catch)]
    async fn startQrCamera() -> Result<(), JsValue>;
    #[wasm_bindgen(catch)]
    async fn nextQrFrame() -> Result<JsValue, JsValue>;
    fn beginPasskeyEnrollment() -> Promise;
    fn beginPasskeyUnlock(id: String) -> Promise;
    fn discardPasskeyEnrollment();
    #[wasm_bindgen(catch)]
    async fn saveConnection(passphrase: String, pairing: String) -> Result<JsValue, JsValue>;
    #[wasm_bindgen(catch)]
    async fn unlockConnection(passphrase: String, id: String) -> Result<JsValue, JsValue>;
    #[wasm_bindgen(catch)]
    async fn finishPasskeyEnrollment(pairing: String) -> Result<JsValue, JsValue>;
}

const BG: u32 = 0x10151c;
const SURFACE: u32 = 0x202a36;
const TEXT: u32 = 0xedf2f7;
const MUTED: u32 = 0x9aaaba;
const ACCENT: u32 = 0x8fc5ec;

thread_local! {
    static APPLICATION: RefCell<Option<ApplicationHandle>> = const { RefCell::new(None) };
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Protection {
    None,
    Passphrase,
    Phone,
}

impl Protection {
    fn from_code(code: u8) -> Self {
        match code {
            1 => Self::Passphrase,
            2 => Self::Phone,
            _ => Self::None,
        }
    }
}

#[derive(Clone, serde::Deserialize)]
struct SavedConnection {
    id: String,
    label: String,
    protection: u8,
}

fn saved_connections() -> Vec<SavedConnection> {
    serde_json::from_str(&savedConnections()).unwrap_or_default()
}

fn js_error(error: JsValue) -> String {
    error
        .as_string()
        .or_else(|| {
            Reflect::get(&error, &JsValue::from_str("message"))
                .ok()?
                .as_string()
        })
        .unwrap_or_else(|| "Could not unlock connection".into())
}

struct MobileView {
    projects: Vec<Project>,
    agent_options: Vec<AgentOption>,
    selected: Option<u64>,
    pending_new_agent: Option<u64>,
    new_session: bool,
    new_project: Option<String>,
    creating_session: bool,
    saved_connections: Vec<SavedConnection>,
    selected_connection: Option<String>,
    show_unlock: bool,
    conversation_scroll: ScrollHandle,
    composer: Entity<TextareaState>,
    passphrase: Entity<InputState>,
    outbound: Sender<Command>,
    incoming: Receiver<Command>,
    pending_pairing: Option<(EndpointId, String)>,
    protection: Protection,
    phone_unlock_available: bool,
    connected: bool,
    unlocking: bool,
    scanning: bool,
    generation: u64,
    connection_status: String,
    action_error: Option<String>,
    prompt_queued: bool,
    _subscriptions: Vec<gpui::Subscription>,
}

impl MobileView {
    fn new(
        window: &mut Window,
        cx: &mut Context<Self>,
        remote: Option<(EndpointId, String)>,
    ) -> Self {
        let composer = cx.new(|cx| TextareaState::new(window, cx).placeholder("Ask your agent…"));
        let passphrase = cx.new(|cx| {
            InputState::new(window, cx)
                .placeholder(if remote.is_some() {
                    "Choose a passphrase (15+ characters)"
                } else {
                    "Unlock passphrase"
                })
                .masked(true)
        });
        let subscription = cx.subscribe_in(&composer, window, |this, _, event, window, cx| {
            if matches!(
                event,
                InputEvent::PressEnter {
                    secondary: false,
                    shift: false
                }
            ) {
                this.send_prompt(window, cx);
            }
        });
        let passphrase_subscription =
            cx.subscribe_in(&passphrase, window, |this, _, event, window, cx| {
                if matches!(
                    event,
                    InputEvent::PressEnter {
                        secondary: false,
                        shift: false
                    }
                ) {
                    this.connect(window, cx);
                }
            });
        let (outbound, incoming) = async_channel::unbounded();
        let saved_connections = saved_connections();
        let selected_connection = saved_connections
            .first()
            .map(|connection| connection.id.clone());
        let protection = saved_connections
            .first()
            .map(|connection| Protection::from_code(connection.protection))
            .unwrap_or(Protection::None);
        let phone_unlock_available = canUsePhoneUnlock();
        Self {
            projects: Vec::new(),
            agent_options: Vec::new(),
            selected: None,
            pending_new_agent: None,
            new_session: false,
            new_project: None,
            creating_session: false,
            saved_connections,
            selected_connection,
            show_unlock: false,
            conversation_scroll: ScrollHandle::new(),
            composer,
            passphrase,
            outbound,
            incoming,
            pending_pairing: remote,
            protection,
            phone_unlock_available,
            connected: false,
            unlocking: false,
            scanning: false,
            generation: 0,
            connection_status: String::new(),
            action_error: None,
            prompt_queued: false,
            _subscriptions: vec![subscription, passphrase_subscription],
        }
    }

    fn scan_qr(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.scanning || self.unlocking || self.connected {
            return;
        }
        self.scanning = true;
        self.action_error = None;
        cx.notify();
        cx.spawn_in(window, async move |this, cx| {
            let result = async {
                startQrCamera().await?;
                loop {
                    let frame = nextQrFrame().await?;
                    if let Some(pairing) =
                        decode_qr_frame(&frame).map_err(|error| JsValue::from_str(&error))?
                    {
                        break Ok(pairing);
                    }
                }
            }
            .await;
            stopQrCamera();
            let _ = this.update_in(cx, |view, window, cx| {
                view.scanning = false;
                match result {
                    Ok(pairing) => {
                        view.pending_pairing = Some(pairing);
                        view.passphrase.update(cx, |input, cx| {
                            input.set_placeholder(
                                "Choose a passphrase (15+ characters)",
                                window,
                                cx,
                            )
                        });
                        view.show_unlock = false;
                        view.connection_status.clear();
                        view.action_error = None;
                    }
                    Err(error) => {
                        let message = js_error(error);
                        if message != "Camera scan cancelled" {
                            view.action_error = Some(message);
                        }
                    }
                }
                cx.notify();
            });
        })
        .detach();
    }

    fn select_saved_connection(&mut self, id: String, window: &mut Window, cx: &mut Context<Self>) {
        let Some(connection) = self
            .saved_connections
            .iter()
            .find(|connection| connection.id == id)
        else {
            return;
        };
        self.protection = Protection::from_code(connection.protection);
        self.selected_connection = Some(id);
        self.pending_pairing = None;
        self.show_unlock = true;
        self.action_error = None;
        self.connection_status.clear();
        self.passphrase.update(cx, |input, cx| {
            input.set_placeholder("Unlock passphrase", window, cx)
        });
        cx.notify();
        if self.protection == Protection::Phone && self.phone_unlock_available {
            self.connect_with_phone(window, cx);
        }
    }

    fn show_saved_desktops(&mut self, cx: &mut Context<Self>) {
        self.pending_pairing = None;
        self.show_unlock = false;
        self.connection_status.clear();
        self.action_error = None;
        cx.notify();
    }

    fn rename_saved_connection(&mut self, id: String, cx: &mut Context<Self>) {
        if renameSavedConnection(id) {
            self.saved_connections = saved_connections();
            cx.notify();
        }
    }

    fn connect(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.unlocking || self.connected {
            return;
        }
        let passphrase = self.passphrase.read(cx).value().to_string();
        if passphrase.is_empty() {
            self.action_error = Some("Enter your unlock passphrase".into());
            cx.notify();
            return;
        }
        if self.pending_pairing.is_some() && passphrase.chars().count() < 15 {
            self.action_error = Some("Use at least 15 characters for the unlock passphrase".into());
            cx.notify();
            return;
        }
        let selected_id = self.selected_connection.clone();
        if self.pending_pairing.is_none() && selected_id.is_none() {
            self.action_error = Some("Choose a saved desktop first".into());
            cx.notify();
            return;
        }
        let pending = self.pending_pairing.take();
        self.unlocking = true;
        self.connection_status = "Unlocking…".into();
        self.action_error = None;
        cx.notify();
        cx.spawn_in(window, async move |this, cx| {
            let result = if let Some((remote, pairing_token)) = pending.as_ref() {
                let result = async {
                    let token = exchange_pairing(*remote, pairing_token).await?;
                    let id = saveConnection(passphrase, format!("{remote}:{token}"))
                        .await?
                        .as_string()
                        .ok_or_else(|| JsValue::from_str("Could not save connection"))?;
                    Ok((*remote, token, id))
                };
                result.await
            } else {
                let id = selected_id.unwrap();
                unlockConnection(passphrase, id.clone())
                    .await
                    .and_then(|value| {
                        value
                            .as_string()
                            .and_then(|value| parse_pairing(&value))
                            .map(|(endpoint, token)| (endpoint, token, id))
                            .ok_or_else(|| JsValue::from_str("Saved connection is damaged"))
                    })
            };
            let _ = this.update_in(cx, |view, window, cx| match result {
                Ok((endpoint, token, id)) => {
                    view.protection = Protection::Passphrase;
                    view.selected_connection = Some(id);
                    view.saved_connections = saved_connections();
                    view.finish_unlock(endpoint, token, window, cx);
                }
                Err(error) => {
                    view.unlocking = false;
                    view.pending_pairing = pending;
                    view.connection_status = "Locked".into();
                    view.action_error = Some(js_error(error));
                    cx.notify();
                }
            });
        })
        .detach();
    }

    fn connect_with_phone(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.unlocking || self.connected {
            return;
        }
        let selected_id = self.selected_connection.clone();
        if self.pending_pairing.is_none() && selected_id.is_none() {
            self.action_error = Some("Choose a saved desktop first".into());
            cx.notify();
            return;
        }
        let pending = self.pending_pairing.take();
        let ceremony = if pending.is_some() {
            beginPasskeyEnrollment()
        } else {
            beginPasskeyUnlock(selected_id.clone().unwrap())
        };
        self.unlocking = true;
        self.connection_status = "Waiting for phone unlock…".into();
        self.action_error = None;
        cx.notify();
        cx.spawn_in(window, async move |this, cx| {
            let result = async {
                let value = JsFuture::from(ceremony).await?;
                if let Some((remote, pairing_token)) = pending.as_ref() {
                    let token = exchange_pairing(*remote, pairing_token).await?;
                    let id = finishPasskeyEnrollment(format!("{remote}:{token}"))
                        .await?
                        .as_string()
                        .ok_or_else(|| JsValue::from_str("Could not save connection"))?;
                    Ok((*remote, token, id))
                } else {
                    let id = selected_id.unwrap();
                    value
                        .as_string()
                        .and_then(|value| parse_pairing(&value))
                        .map(|(endpoint, token)| (endpoint, token, id))
                        .ok_or_else(|| JsValue::from_str("Saved connection is damaged"))
                }
            }
            .await;
            discardPasskeyEnrollment();
            let _ = this.update_in(cx, |view, window, cx| match result {
                Ok((endpoint, token, id)) => {
                    view.protection = Protection::Phone;
                    view.selected_connection = Some(id);
                    view.saved_connections = saved_connections();
                    view.finish_unlock(endpoint, token, window, cx);
                }
                Err(error) => {
                    view.unlocking = false;
                    view.pending_pairing = pending;
                    view.connection_status = "Locked".into();
                    view.action_error = Some(js_error(error));
                    cx.notify();
                }
            });
        })
        .detach();
    }

    fn finish_unlock(
        &mut self,
        endpoint: EndpointId,
        token: String,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.unlocking = false;
        self.passphrase
            .update(cx, |input, cx| input.set_value("", window, cx));
        self.pending_pairing = None;
        self.connected = true;
        self.generation += 1;
        self.connection_status = "Connecting…".into();
        self.start_client(endpoint, token, self.generation, window, cx);
        cx.notify();
    }

    fn lock(&mut self, cx: &mut Context<Self>) {
        self.generation += 1;
        self.connected = false;
        self.unlocking = false;
        self.show_unlock = false;
        self.projects.clear();
        self.agent_options.clear();
        self.selected = None;
        self.pending_new_agent = None;
        self.new_session = false;
        self.new_project = None;
        self.creating_session = false;
        self.prompt_queued = false;
        while self.incoming.try_recv().is_ok() {}
        self.connection_status.clear();
        cx.notify();
    }

    fn start_client(
        &self,
        remote: EndpointId,
        token: String,
        generation: u64,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let incoming = self.incoming.clone();
        cx.spawn_in(window, async move |this, cx| {
            let endpoint = match Endpoint::bind(presets::N0).await {
                Ok(endpoint) => endpoint,
                Err(error) => {
                    let _ = this.update_in(cx, |view, _, cx| {
                        if view.generation != generation {
                            return;
                        }
                        view.connection_status = format!("Could not start Iroh: {error}");
                        cx.notify();
                    });
                    return;
                }
            };
            loop {
                if this
                    .read_with(cx, |view, _| view.generation != generation)
                    .unwrap_or(true)
                {
                    break;
                }
                if pageHidden() {
                    let _ = this.update_in(cx, |view, _, cx| view.lock(cx));
                    break;
                }
                while let Ok(command) = incoming.try_recv() {
                    if this
                        .read_with(cx, |view, _| view.generation != generation)
                        .unwrap_or(true)
                    {
                        break;
                    }
                    let prompt = match &command {
                        Command::Prompt { text, .. } => Some(text.clone()),
                        _ => None,
                    };
                    let creating = matches!(&command, Command::NewSession { .. });
                    let response = request(&endpoint, remote, &token, command).await;
                    let created = match &response {
                        Ok(Response::SessionCreated { agent_id }) => Some(*agent_id),
                        _ => None,
                    };
                    let error = match response {
                        Ok(Response::Error { message }) | Err(message) => Some(message),
                        _ => None,
                    };
                    let _ = this.update_in(cx, |view, window, cx| {
                        if view.generation != generation {
                            return;
                        }
                        if let Some(prompt) = prompt {
                            view.prompt_queued = false;
                            if error.is_none() && view.composer.read(cx).value().as_ref() == prompt
                            {
                                view.composer
                                    .update(cx, |input, cx| input.set_value("", window, cx));
                            }
                        }
                        if creating {
                            view.creating_session = false;
                            if let Some(agent_id) = created {
                                view.selected = Some(agent_id);
                                view.pending_new_agent = Some(agent_id);
                                view.new_session = false;
                                view.new_project = None;
                                view.conversation_scroll.scroll_to_bottom();
                            }
                        }
                        view.action_error = error;
                        cx.notify();
                    });
                }
                match request(&endpoint, remote, &token, Command::Snapshot).await {
                    Ok(Response::Snapshot {
                        projects,
                        agent_options,
                    }) => {
                        if this
                            .update_in(cx, |view, _, cx| {
                                if view.generation != generation {
                                    return;
                                }
                                let previous_selected = view.selected;
                                let previous_message = view.selected_agent().and_then(|agent| {
                                    agent
                                        .messages
                                        .last()
                                        .map(|message| (agent.messages.len(), message.text.len()))
                                });
                                let near_bottom = view.conversation_scroll.max_offset().y
                                    + view.conversation_scroll.offset().y
                                    < px(80.);
                                view.projects = projects;
                                view.agent_options = agent_options;
                                if view.pending_new_agent.is_some_and(|id| {
                                    view.projects.iter().any(|project| {
                                        project.agents.iter().any(|agent| agent.id == id)
                                    })
                                }) {
                                    view.pending_new_agent = None;
                                }
                                if view.pending_new_agent.is_none() && view.selected.is_none_or(|id| {
                                    !view.projects.iter().any(|project| {
                                        project.agents.iter().any(|agent| agent.id == id)
                                    })
                                }) {
                                    view.selected = view
                                        .projects
                                        .iter()
                                        .flat_map(|project| &project.agents)
                                        .next()
                                        .map(|agent| agent.id);
                                }
                                let current_message = view.selected_agent().and_then(|agent| {
                                    agent
                                        .messages
                                        .last()
                                        .map(|message| (agent.messages.len(), message.text.len()))
                                });
                                if previous_selected != view.selected
                                    || (near_bottom && previous_message != current_message)
                                {
                                    view.conversation_scroll.scroll_to_bottom();
                                }
                                view.connection_status = "Connected".into();
                                cx.notify();
                            })
                            .is_err()
                        {
                            break;
                        }
                    }
                    Ok(Response::Error { message }) | Err(message) => {
                        if this
                            .update_in(cx, |view, _, cx| {
                                if view.generation != generation {
                                    return;
                                }
                                view.connection_status = message;
                                cx.notify();
                            })
                            .is_err()
                        {
                            break;
                        }
                    }
                    Ok(Response::Accepted | Response::Paired { .. } | Response::SessionCreated { .. }) => {}
                }
                cx.background_executor()
                    .timer(Duration::from_millis(800))
                    .await;
            }
            endpoint.close().await;
        })
        .detach();
    }

    fn open_new_session(&mut self, cx: &mut Context<Self>) {
        self.new_session = true;
        self.new_project = None;
        self.action_error = None;
        cx.notify();
    }

    fn back_from_new_session(&mut self, cx: &mut Context<Self>) {
        if self.creating_session {
            return;
        }
        if self.new_project.is_some() {
            self.new_project = None;
        } else {
            self.new_session = false;
        }
        self.action_error = None;
        cx.notify();
    }

    fn choose_new_project(&mut self, project: String, cx: &mut Context<Self>) {
        self.new_project = Some(project);
        self.action_error = None;
        cx.notify();
    }

    fn prompt_new_project(&mut self, cx: &mut Context<Self>) {
        if let Some(project) = promptProjectPath().as_string() {
            self.choose_new_project(project, cx);
        }
    }

    fn start_new_session(&mut self, command: Vec<String>, name: Option<String>, cx: &mut Context<Self>) {
        if self.creating_session {
            return;
        }
        let Some(project) = self.new_project.clone() else {
            return;
        };
        if self.outbound.try_send(Command::NewSession { project, command, name }).is_ok() {
            self.creating_session = true;
            self.action_error = None;
        } else {
            self.action_error = Some("Could not start a new session".into());
        }
        cx.notify();
    }

    fn start_custom_session(&mut self, cx: &mut Context<Self>) {
        let Some(value) = promptAcpCommand().as_string() else {
            return;
        };
        match shell_words::split(&value) {
            Ok(command) if !command.is_empty() => self.start_new_session(command, None, cx),
            _ => {
                self.action_error = Some("Enter an ACP executable and its arguments".into());
                cx.notify();
            }
        }
    }

    fn send_prompt(&mut self, _window: &mut Window, cx: &mut Context<Self>) {
        if self.prompt_queued {
            return;
        }
        let Some(agent_id) = self.selected else {
            return;
        };
        let text = self.composer.read(cx).value().to_string();
        let text = text.strip_suffix('\n').unwrap_or(&text).to_owned();
        if text.trim().is_empty() {
            return;
        }
        if self
            .outbound
            .try_send(Command::Prompt { agent_id, text })
            .is_ok()
        {
            self.prompt_queued = true;
            self.action_error = None;
            cx.notify();
        } else {
            self.action_error = Some("Could not queue prompt".into());
            cx.notify();
        }
    }

    fn selected_agent(&self) -> Option<&Agent> {
        let id = self.selected?;
        self.projects
            .iter()
            .flat_map(|project| &project.agents)
            .find(|agent| agent.id == id)
    }
}

impl Render for MobileView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        if !self.connected {
            let mut saved_list = div()
                .id("saved-desktops")
                .flex()
                .flex_col()
                .flex_shrink_0()
                .gap_2()
                .child(
                    div()
                        .text_sm()
                        .text_color(rgb(MUTED))
                        .child("Saved desktops"),
                );
            for connection in &self.saved_connections {
                let id = connection.id.clone();
                let rename_id = id.clone();
                saved_list = saved_list.child(
                    div()
                        .id(format!("saved-connection-{id}"))
                        .flex()
                        .items_center()
                        .gap_2()
                        .rounded_lg()
                        .bg(rgb(SURFACE))
                        .child(
                            div()
                                .id(format!("open-saved-connection-{id}"))
                                .flex_1()
                                .min_w_0()
                                .px_3()
                                .py_3()
                                .cursor_pointer()
                                .child(div().truncate().child(connection.label.clone()))
                                .on_click(cx.listener(move |this, _, window, cx| {
                                    this.select_saved_connection(id.clone(), window, cx)
                                })),
                        )
                        .child(
                            div()
                                .id(format!("rename-saved-connection-{rename_id}"))
                                .px_3()
                                .py_3()
                                .text_sm()
                                .text_color(rgb(ACCENT))
                                .cursor_pointer()
                                .child("Rename")
                                .on_click(cx.listener(move |this, _, _, cx| {
                                    this.rename_saved_connection(rename_id.clone(), cx)
                                })),
                        ),
                );
            }
            let page = div()
                .id("locked-page")
                .size_full()
                .flex()
                .flex_col()
                .overflow_y_scroll()
                .gap_4()
                .p_4()
                .bg(rgb(BG))
                .text_color(rgb(TEXT));
            let pairing = self.pending_pairing.is_some() || (self.unlocking && !self.show_unlock);
            if pairing || self.show_unlock {
                let title = if pairing {
                    "Pair desktop".to_string()
                } else {
                    self.saved_connections
                        .iter()
                        .find(|connection| {
                            self.selected_connection.as_deref() == Some(connection.id.as_str())
                        })
                        .map(|connection| connection.label.clone())
                        .unwrap_or_else(|| "Saved desktop".into())
                };
                return page
                    .child(
                        div()
                            .id("back-to-saved-desktops")
                            .text_color(rgb(ACCENT))
                            .cursor_pointer()
                            .child("Back")
                            .when(!self.unlocking, |element| {
                                element.on_click(
                                    cx.listener(|this, _, _, cx| this.show_saved_desktops(cx)),
                                )
                            }),
                    )
                    .child(div().text_lg().child(title))
                    .when(self.unlocking, |element| {
                        element.child(
                            div()
                                .text_color(rgb(MUTED))
                                .child(self.connection_status.clone()),
                        )
                    })
                    .when_some(self.action_error.as_ref(), |element, error| {
                        element.child(div().text_color(rgb(0xe99191)).child(error.clone()))
                    })
                    .when(
                        !pairing
                            && self.protection == Protection::Phone
                            && !self.phone_unlock_available,
                        |element| {
                            element.child(
                                div()
                                    .text_color(rgb(0xe99191))
                                    .child("Phone unlock is unavailable in this browser."),
                            )
                        },
                    )
                    .when(
                        !self.unlocking
                            && self.phone_unlock_available
                            && (pairing || self.protection == Protection::Phone),
                        |element| {
                            element.child(
                                div()
                                    .id("connect-with-phone")
                                    .rounded_md()
                                    .px_3()
                                    .py_2()
                                    .bg(rgb(0x304b60))
                                    .cursor_pointer()
                                    .child(if pairing {
                                        "Pair with phone unlock"
                                    } else {
                                        "Unlock with phone"
                                    })
                                    .on_click(cx.listener(|this, _, window, cx| {
                                        this.connect_with_phone(window, cx)
                                    })),
                            )
                        },
                    )
                    .when(
                        !self.unlocking && (pairing || self.protection == Protection::Passphrase),
                        |element| {
                            element.child(Input::new(&self.passphrase)).child(
                                div()
                                    .id("connect-with-passphrase")
                                    .rounded_md()
                                    .px_3()
                                    .py_2()
                                    .bg(rgb(0x304b60))
                                    .cursor_pointer()
                                    .child(if pairing {
                                        "Pair with passphrase"
                                    } else {
                                        "Unlock"
                                    })
                                    .on_click(
                                        cx.listener(|this, _, window, cx| this.connect(window, cx)),
                                    ),
                            )
                        },
                    )
                    .into_any_element();
            }
            return page
                .when_some(self.action_error.as_ref(), |element, error| {
                    element.child(div().text_color(rgb(0xe99191)).child(error.clone()))
                })
                .when(self.scanning, |element| {
                    element.child(div().text_color(rgb(MUTED)).child("Scanning QR code…"))
                })
                .when(!self.saved_connections.is_empty(), |element| {
                    element.child(saved_list)
                })
                .child(
                    div()
                        .id("scan-desktop-qr")
                        .rounded_md()
                        .px_3()
                        .py_2()
                        .bg(rgb(0x304b60))
                        .cursor_pointer()
                        .child(if self.saved_connections.is_empty() {
                            "Pair a desktop"
                        } else {
                            "Pair another desktop"
                        })
                        .on_click(cx.listener(|this, _, window, cx| this.scan_qr(window, cx))),
                )
                .into_any_element();
        }
        if self.new_session {
            let mut choices = div()
                .id("new-session-choices")
                .flex_1()
                .overflow_y_scroll()
                .flex()
                .flex_col()
                .gap_2();
            if let Some(project) = &self.new_project {
                for (index, option) in self.agent_options.iter().enumerate() {
                    let command = option.command.clone();
                    let name = option.name.clone();
                    choices = choices.child(
                        div()
                            .id(("new-agent-option", index))
                            .rounded_lg()
                            .p_3()
                            .bg(rgb(SURFACE))
                            .cursor_pointer()
                            .child(name.clone())
                            .on_click(cx.listener(move |this, _, _, cx| {
                                this.start_new_session(command.clone(), Some(name.clone()), cx)
                            })),
                    );
                }
                choices = choices.child(
                    div()
                        .id("new-custom-agent")
                        .rounded_lg()
                        .p_3()
                        .bg(rgb(SURFACE))
                        .cursor_pointer()
                        .child("Custom ACP command")
                        .on_click(cx.listener(|this, _, _, cx| this.start_custom_session(cx))),
                );
                return div()
                    .id("new-session-page")
                    .size_full()
                    .flex()
                    .flex_col()
                    .gap_4()
                    .p_4()
                    .bg(rgb(BG))
                    .text_color(rgb(TEXT))
                    .child(
                        div()
                            .id("new-session-back")
                            .text_color(rgb(ACCENT))
                            .cursor_pointer()
                            .child("Back")
                            .on_click(cx.listener(|this, _, _, cx| this.back_from_new_session(cx))),
                    )
                    .child(div().text_lg().child("Choose agent"))
                    .child(div().text_sm().text_color(rgb(MUTED)).child(project.clone()))
                    .when(self.creating_session, |element| {
                        element.child(div().text_color(rgb(MUTED)).child("Starting session…"))
                    })
                    .when_some(self.action_error.as_ref(), |element, error| {
                        element.child(div().text_color(rgb(0xe99191)).child(error.clone()))
                    })
                    .child(choices)
                    .into_any_element();
            }
            for (index, project) in self.projects.iter().enumerate() {
                let path = project.path.clone();
                choices = choices.child(
                    div()
                        .id(("new-project-option", index))
                        .rounded_lg()
                        .p_3()
                        .bg(rgb(SURFACE))
                        .cursor_pointer()
                        .child(path.clone())
                        .on_click(cx.listener(move |this, _, _, cx| {
                            this.choose_new_project(path.clone(), cx)
                        })),
                );
            }
            choices = choices.child(
                div()
                    .id("new-other-project")
                    .rounded_lg()
                    .p_3()
                    .bg(rgb(SURFACE))
                    .cursor_pointer()
                    .child("Other project…")
                    .on_click(cx.listener(|this, _, _, cx| this.prompt_new_project(cx))),
            );
            return div()
                .id("new-session-page")
                .size_full()
                .flex()
                .flex_col()
                .gap_4()
                .p_4()
                .bg(rgb(BG))
                .text_color(rgb(TEXT))
                .child(
                    div()
                        .id("new-session-back")
                        .text_color(rgb(ACCENT))
                        .cursor_pointer()
                        .child("Back")
                        .on_click(cx.listener(|this, _, _, cx| this.back_from_new_session(cx))),
                )
                .child(div().text_lg().child("Choose project"))
                .when_some(self.action_error.as_ref(), |element, error| {
                    element.child(div().text_color(rgb(0xe99191)).child(error.clone()))
                })
                .child(choices)
                .into_any_element();
        }
        let narrow = f32::from(window.viewport_size().width) < 600.;
        let mut sessions = div()
            .id("sessions")
            .flex()
            .flex_shrink_0()
            .gap_2()
            .overflow_x_scroll()
            .px_3()
            .py_2();
        for project in &self.projects {
            for agent in &project.agents {
                let id = agent.id;
                let selected = self.selected == Some(id);
                let folder = project.path.rsplit('/').next().unwrap_or(&project.path);
                sessions = sessions.child(
                    div()
                        .id(("session", id))
                        .flex_shrink_0()
                        .max_w(px(if narrow { 200. } else { 260. }))
                        .rounded_md()
                        .px_3()
                        .py_2()
                        .bg(rgb(if selected { 0x30485f } else { SURFACE }))
                        .text_color(rgb(TEXT))
                        .cursor_pointer()
                        .child(div().truncate().child(format!("{folder} · {}", agent.name)))
                        .on_click(cx.listener(move |this, _, _, cx| {
                            this.selected = Some(id);
                            this.pending_new_agent = None;
                            this.conversation_scroll.scroll_to_bottom();
                            cx.notify();
                        })),
                );
            }
        }
        let mut conversation = div()
            .id("conversation")
            .flex_1()
            .min_h(px(0.))
            .min_w(px(0.))
            .overflow_y_scroll()
            .track_scroll(&self.conversation_scroll)
            .flex()
            .flex_col()
            .gap_3()
            .px_3()
            .py_4();
        if let Some(agent) = self.selected_agent() {
            if agent.has_older_messages {
                conversation = conversation.child(
                    div()
                        .text_color(rgb(MUTED))
                        .child("Showing the latest 100 messages"),
                );
            }
            for (index, message) in agent.messages.iter().enumerate() {
                let is_user = message.role == "user";
                let is_agent = message.role == "agent";
                let is_context_reset = message.role == "contextreset";
                let label = match message.role.as_str() {
                    "user" => "You",
                    "agent" => agent.name.as_str(),
                    "thought" => "Thought",
                    "tool" => "Tool",
                    "system" => "System",
                    "contextreset" => "Context reset",
                    _ => "Update",
                };
                if is_context_reset {
                    conversation = conversation.child(
                        div()
                            .id(("message", index))
                            .w_full()
                            .flex_shrink_0()
                            .py_2()
                            .text_center()
                            .text_xs()
                            .text_color(rgb(MUTED))
                            .child(message.text.clone()),
                    );
                    continue;
                }
                let text_id: gpui::ElementId = ("mobile-message", agent.id).into();
                let content =
                    TextView::markdown((text_id, index.to_string()), message.text.clone())
                        .style(TextViewStyle {
                            paragraph_gap: rems(0.25),
                            highlight_theme: cx.theme().highlight_theme.clone(),
                            is_dark: true,
                            ..Default::default()
                        })
                        .selectable(true)
                        .text_sm()
                        .text_color(rgb(TEXT));
                conversation = conversation.child(
                    div()
                        .id(("message", index))
                        .w_full()
                        .flex_shrink_0()
                        .min_w(px(0.))
                        .flex()
                        .when(is_user, |element| element.justify_end())
                        .child(
                            div()
                                .min_w(px(0.))
                                .max_w(relative(if is_user { 0.92 } else { 1.0 }))
                                .rounded_lg()
                                .px_3()
                                .py_2()
                                .bg(rgb(if is_user {
                                    0x29465c
                                } else if is_agent {
                                    SURFACE
                                } else {
                                    0x1d2732
                                }))
                                .child(
                                    div()
                                        .mb_2()
                                        .text_xs()
                                        .text_color(rgb(if is_user || is_agent {
                                            ACCENT
                                        } else {
                                            MUTED
                                        }))
                                        .child(label.to_owned()),
                                )
                                .child(div().min_w(px(0.)).whitespace_normal().child(content)),
                        ),
                );
            }
            for (index, permission) in agent.permissions.iter().enumerate() {
                let mut card = div()
                    .rounded_lg()
                    .flex_shrink_0()
                    .min_w(px(0.))
                    .p_3()
                    .bg(rgb(0x4b382a))
                    .text_color(rgb(TEXT))
                    .child(permission.title.clone());
                if let Some(description) = &permission.description {
                    card = card.child(div().mt_2().child(description.clone()));
                }
                let mut options = div().flex().flex_wrap().gap_2().mt_3();
                for (option_index, option) in permission.options.iter().enumerate() {
                    let command = Command::Permission {
                        agent_id: agent.id,
                        request_id: permission.request_id.clone(),
                        option_id: option.id.clone(),
                    };
                    let outbound = self.outbound.clone();
                    options = options.child(
                        div()
                            .id(("option", index * 100 + option_index))
                            .rounded_md()
                            .px_3()
                            .py_2()
                            .bg(rgb(0x304b60))
                            .cursor_pointer()
                            .child(option.label.clone())
                            .on_click(move |_, _, _| {
                                let _ = outbound.try_send(command.clone());
                            }),
                    );
                }
                conversation = conversation.child(card.child(options));
            }
        } else {
            conversation =
                conversation.child(div().text_color(rgb(MUTED)).child("No sessions yet"));
        }
        let active = self.selected_agent().is_some_and(|agent| agent.active);
        let selected = self.selected;
        let outbound = self.outbound.clone();
        div()
            .size_full()
            .min_w(px(0.))
            .flex()
            .flex_col()
            .bg(rgb(BG))
            .text_color(rgb(TEXT))
            .child(
                div()
                    .flex()
                    .flex_shrink_0()
                    .items_center()
                    .justify_between()
                    .px_3()
                    .py_2()
                    .child(div().text_lg().child("Agentaps"))
                    .child(
                        div()
                            .id("new-session")
                            .rounded_md()
                            .px_3()
                            .py_2()
                            .text_sm()
                            .text_color(rgb(ACCENT))
                            .cursor_pointer()
                            .child("New")
                            .on_click(cx.listener(|this, _, _, cx| this.open_new_session(cx))),
                    ),
            )
            .when(
                self.connection_status != "Connected" && !self.connection_status.is_empty(),
                |element| {
                    element.child(
                        div()
                            .px_4()
                            .text_sm()
                            .text_color(rgb(MUTED))
                            .child(self.connection_status.clone()),
                    )
                },
            )
            .when_some(self.action_error.as_ref(), |element, error| {
                element.child(
                    div()
                        .px_4()
                        .py_1()
                        .text_sm()
                        .text_color(rgb(0xe99191))
                        .child(error.clone()),
                )
            })
            .child(sessions)
            .child(conversation)
            .child(
                div()
                    .flex_shrink_0()
                    .p_3()
                    .bg(rgb(SURFACE))
                    .flex()
                    .items_end()
                    .gap_2()
                    .child(
                        div()
                            .flex_1()
                            .min_w(px(0.))
                            .child(Textarea::new(&self.composer)),
                    )
                    .child(
                        div()
                            .id("send")
                            .rounded_md()
                            .px_3()
                            .py_2()
                            .bg(rgb(0x304b60))
                            .cursor_pointer()
                            .child(if self.prompt_queued {
                                "Sending"
                            } else {
                                "Send"
                            })
                            .on_click(
                                cx.listener(|this, _, window, cx| this.send_prompt(window, cx)),
                            ),
                    )
                    .when(active, |element| {
                        element.child(
                            div()
                                .id("stop")
                                .rounded_md()
                                .px_3()
                                .py_2()
                                .bg(rgb(0x51343a))
                                .cursor_pointer()
                                .child("Stop")
                                .on_click(move |_, _, _| {
                                    if let Some(agent_id) = selected {
                                        let _ = outbound.try_send(Command::Cancel { agent_id });
                                    }
                                }),
                        )
                    }),
            )
            .into_any_element()
    }
}

async fn request(
    endpoint: &Endpoint,
    remote: EndpointId,
    token: &str,
    command: Command,
) -> Result<Response, String> {
    let connection = endpoint
        .connect(remote, ALPN)
        .await
        .map_err(|error| error.to_string())?;
    let (mut send, mut recv) = connection
        .open_bi()
        .await
        .map_err(|error| error.to_string())?;
    let bytes = serde_json::to_vec(&Request {
        token: token.into(),
        command,
    })
    .map_err(|error| error.to_string())?;
    send.write_all(&bytes)
        .await
        .map_err(|error| error.to_string())?;
    send.finish().map_err(|error| error.to_string())?;
    let bytes = recv
        .read_to_end(MAX_RESPONSE_BYTES)
        .await
        .map_err(|error| error.to_string())?;
    connection.close(0u32.into(), b"done");
    serde_json::from_slice(&bytes).map_err(|error| error.to_string())
}

async fn exchange_pairing(remote: EndpointId, pairing_token: &str) -> Result<String, JsValue> {
    let endpoint = Endpoint::bind(presets::N0)
        .await
        .map_err(|error| JsValue::from_str(&error.to_string()))?;
    let response = request(&endpoint, remote, pairing_token, Command::Pair).await;
    endpoint.close().await;
    match response {
        Ok(Response::Paired { token }) => Ok(token),
        Ok(Response::Error { message }) | Err(message) => Err(JsValue::from_str(&message)),
        _ => Err(JsValue::from_str("Unexpected pairing response")),
    }
}

fn pairing() -> Option<(EndpointId, String)> {
    let hash = web_sys::window()?.location().hash().ok()?;
    if hash.is_empty() {
        return None;
    }
    clearPairingHash();
    parse_pairing(hash.trim_start_matches('#'))
}

fn parse_pairing(value: &str) -> Option<(EndpointId, String)> {
    let (endpoint, token) = value.split_once(':')?;
    if token.len() != 64 || !token.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return None;
    }
    Some((EndpointId::from_str(endpoint).ok()?, token.into()))
}

fn decode_qr_frame(frame: &JsValue) -> Result<Option<(EndpointId, String)>, String> {
    if frame.is_null() {
        return Ok(None);
    }
    let property = |key| Reflect::get(frame, &JsValue::from_str(key)).ok();
    let Some(width) = property("width").and_then(|value| value.as_f64()) else {
        return Ok(None);
    };
    let Some(height) = property("height").and_then(|value| value.as_f64()) else {
        return Ok(None);
    };
    let (width, height) = (width as usize, height as usize);
    if !(80..=1280).contains(&width) || !(80..=1280).contains(&height) {
        return Ok(None);
    }
    let Some(raw_pixels) = property("pixels") else {
        return Ok(None);
    };
    let pixels = Uint8Array::new(&raw_pixels).to_vec();
    if pixels.len() != width * height {
        return Ok(None);
    }
    let mut image =
        rqrr::PreparedImage::prepare_from_greyscale(width, height, |x, y| pixels[y * width + x]);
    for grid in image.detect_grids() {
        let Ok((_, content)) = grid.decode() else {
            continue;
        };
        let Ok(url) = web_sys::Url::new(&content) else {
            continue;
        };
        let Some(pairing) = parse_pairing(url.hash().trim_start_matches('#')) else {
            continue;
        };
        let origin = web_sys::window()
            .and_then(|window| window.location().origin().ok())
            .ok_or("Could not verify this site's address")?;
        if url.origin() != origin {
            return Err("This QR code points to a different site. Open the site shown in the desktop pairing link and scan again.".into());
        }
        return Ok(Some(pairing));
    }
    Ok(None)
}

#[wasm_bindgen(start)]
pub fn start() {
    gpui_ce_platform::web_init();
    let app = gpui_ce_platform::single_threaded_web().with_assets(Assets::default());
    APPLICATION.with(|application| {
        *application.borrow_mut() = Some(app.run_embedded(|cx: &mut App| {
            gpui_component::init(cx);
            cx.text_system()
                .add_fonts(vec![Cow::Borrowed(include_bytes!(
                    "../fonts/IBMPlexSans-Regular.ttf"
                ))])
                .expect("Could not load font");
            cx.open_window(WindowOptions::default(), |window, cx| {
                let remote = pairing();
                let auto_scan = remote.is_none() && saved_connections().is_empty();
                let view = cx.new(|cx| MobileView::new(window, cx, remote));
                let root = cx.new(|cx| Root::new(view.clone(), window, cx));
                if auto_scan {
                    view.update(cx, |view, cx| view.scan_qr(window, cx));
                }
                root
            })
            .expect("Could not open browser window");
            cx.activate(true);
        }));
    });
}

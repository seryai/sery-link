use std::collections::HashMap;
use std::sync::Arc;

use russh::client::{self, Config, Handle};
use russh::keys::{key::PrivateKeyWithHashAlg, load_secret_key};
use russh::ChannelMsg;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;
use tokio::sync::Mutex;
use tokio::task::JoinHandle;
use tokio::time::Duration;

/// Initial delay between SSH reconnect attempts.
const INITIAL_RECONNECT_DELAY: Duration = Duration::from_secs(5);
/// Maximum delay for exponential backoff.
const MAX_RECONNECT_DELAY: Duration = Duration::from_secs(60);
/// Maximum number of consecutive reconnect attempts before giving up.
const MAX_RECONNECT_ATTEMPTS: u32 = 10;

#[derive(Debug, Clone)]
pub struct SshTunnelConfig {
    pub host: String,
    pub port: u16,
    pub username: String,
    pub auth: SshAuth,
}

#[derive(Debug, Clone)]
pub enum SshAuth {
    Password(String),
    PrivateKey { path: String, passphrase: Option<String> },
}

struct SshClient;

impl client::Handler for SshClient {
    type Error = russh::Error;

    async fn check_server_key(
        &mut self,
        _server_public_key: &russh::keys::ssh_key::PublicKey,
    ) -> Result<bool, Self::Error> {
        Ok(true)
    }
}

async fn connect_and_authenticate(
    ssh_host: &str,
    ssh_port: u16,
    ssh_user: &str,
    ssh_password: &str,
    ssh_key_path: &str,
    ssh_key_passphrase: &str,
    connect_timeout_secs: u64,
) -> Result<Handle<SshClient>, String> {
    let config =
        Arc::new(Config { nodelay: true, keepalive_interval: Some(Duration::from_secs(30)), ..Default::default() });
    let connect_timeout = Duration::from_secs(connect_timeout_secs);

    let mut session =
        tokio::time::timeout(connect_timeout, client::connect(config, (ssh_host, ssh_port), SshClient {}))
            .await
            .map_err(|_| format!("SSH connection timed out ({connect_timeout_secs}s)"))?
            .map_err(|e| format!("SSH connection failed: {e}"))?;

    if !ssh_key_path.is_empty() {
        let passphrase = if ssh_key_passphrase.is_empty() { None } else { Some(ssh_key_passphrase) };
        let key_pair = load_secret_key(ssh_key_path, passphrase).map_err(|e| format!("Failed to load SSH key: {e}"))?;
        let auth_res = tokio::time::timeout(
            connect_timeout,
            session.authenticate_publickey(
                ssh_user,
                PrivateKeyWithHashAlg::new(
                    Arc::new(key_pair),
                    session.best_supported_rsa_hash().await.ok().flatten().flatten(),
                ),
            ),
        )
        .await
        .map_err(|_| format!("SSH key auth timed out ({connect_timeout_secs}s)"))?
        .map_err(|e| format!("SSH key auth failed: {e}"))?;
        if !auth_res.success() {
            return Err("SSH public key authentication failed".to_string());
        }
    } else if !ssh_password.is_empty() {
        let auth_res = tokio::time::timeout(connect_timeout, session.authenticate_password(ssh_user, ssh_password))
            .await
            .map_err(|_| format!("SSH password auth timed out ({connect_timeout_secs}s)"))?
            .map_err(|e| format!("SSH password auth failed: {e}"))?;
        if !auth_res.success() {
            return Err("SSH password authentication failed".to_string());
        }
    } else {
        return Err("No SSH password or key provided".to_string());
    }

    Ok(session)
}

/// Accept connections on the local listener and forward them through the SSH session.
async fn forward_loop(session: &Handle<SshClient>, listener: &TcpListener, remote_host: &str, remote_port: u16) {
    loop {
        let (mut stream, peer_addr) = match listener.accept().await {
            Ok(v) => v,
            Err(e) => {
                log::error!("SSH tunnel listener error: {e}");
                break;
            }
        };

        if session.is_closed() {
            log::warn!("SSH session closed, exiting forward loop");
            break;
        }

        let mut channel = match session
            .channel_open_direct_tcpip(
                remote_host,
                remote_port.into(),
                peer_addr.ip().to_string(),
                peer_addr.port().into(),
            )
            .await
        {
            Ok(c) => c,
            Err(e) => {
                log::error!("SSH direct-tcpip failed: {e}");
                if session.is_closed() {
                    log::warn!("SSH session closed after channel open failure");
                    break;
                }
                continue;
            }
        };

        tokio::spawn(async move {
            let mut buf = vec![0u8; 65536];
            let mut stream_closed = false;

            loop {
                tokio::select! {
                    r = stream.read(&mut buf), if !stream_closed => {
                        match r {
                            Ok(0) => {
                                stream_closed = true;
                                let _ = channel.eof().await;
                            }
                            Ok(n) => {
                                if channel.data(&buf[..n]).await.is_err() {
                                    break;
                                }
                            }
                            Err(_) => break,
                        }
                    }
                    msg = channel.wait() => {
                        match msg {
                            Some(ChannelMsg::Data { ref data }) => {
                                if stream.write_all(data).await.is_err() {
                                    break;
                                }
                            }
                            Some(ChannelMsg::Eof) | None => break,
                            _ => {}
                        }
                    }
                }
            }
        });
    }
}

#[allow(clippy::too_many_arguments)]
async fn tunnel_reconnect_loop(
    mut session: Handle<SshClient>,
    ssh_host: String,
    ssh_port: u16,
    ssh_user: String,
    ssh_password: String,
    ssh_key_path: String,
    ssh_key_passphrase: String,
    connect_timeout_secs: u64,
    listener: TcpListener,
    remote_host: String,
    remote_port: u16,
) {
    loop {
        log::info!("SSH tunnel active: {}:{} -> {}:{}", ssh_host, ssh_port, remote_host, remote_port);

        forward_loop(&session, &listener, &remote_host, remote_port).await;

        log::warn!("SSH tunnel connection lost ({}:{}), reconnecting...", ssh_host, ssh_port);

        let mut delay = INITIAL_RECONNECT_DELAY;
        let mut attempts: u32 = 0;

        loop {
            if attempts >= MAX_RECONNECT_ATTEMPTS {
                log::error!(
                    "SSH tunnel ({ssh_host}:{ssh_port}): max reconnect attempts ({MAX_RECONNECT_ATTEMPTS}) exhausted, giving up"
                );
                return;
            }

            tokio::time::sleep(delay).await;

            match connect_and_authenticate(
                &ssh_host,
                ssh_port,
                &ssh_user,
                &ssh_password,
                &ssh_key_path,
                &ssh_key_passphrase,
                connect_timeout_secs,
            )
            .await
            {
                Ok(new_session) => {
                    session = new_session;
                    log::info!("SSH tunnel reconnected to {}:{} (attempt {})", ssh_host, ssh_port, attempts + 1);
                    break;
                }
                Err(e) => {
                    attempts += 1;
                    log::error!(
                        "SSH reconnect failed ({}:{}, attempt {attempts}/{MAX_RECONNECT_ATTEMPTS}): {e}",
                        ssh_host,
                        ssh_port,
                    );
                    delay = std::cmp::min(delay * 2, MAX_RECONNECT_DELAY);
                }
            }
        }
    }
}

pub struct TunnelManager {
    tunnels: Mutex<HashMap<String, (JoinHandle<()>, u16)>>,
}

impl Default for TunnelManager {
    fn default() -> Self {
        Self::new()
    }
}

impl TunnelManager {
    pub fn new() -> Self {
        Self { tunnels: Mutex::new(HashMap::new()) }
    }

    #[allow(clippy::too_many_arguments)]
    pub async fn start_tunnel(
        &self,
        connection_id: &str,
        ssh_host: &str,
        ssh_port: u16,
        ssh_user: &str,
        ssh_password: &str,
        ssh_key_path: &str,
        ssh_key_passphrase: &str,
        connect_timeout_secs: u64,
        remote_host: &str,
        remote_port: u16,
        expose_to_lan: bool,
    ) -> Result<u16, String> {
        let local_port = portpicker::pick_unused_port().ok_or("No available port")?;

        let bind_addr = if expose_to_lan { "0.0.0.0" } else { "127.0.0.1" };
        let listener =
            TcpListener::bind((bind_addr, local_port)).await.map_err(|e| format!("Failed to bind local port: {e}"))?;

        let session = connect_and_authenticate(
            ssh_host,
            ssh_port,
            ssh_user,
            ssh_password,
            ssh_key_path,
            ssh_key_passphrase,
            connect_timeout_secs,
        )
        .await?;

        let handle = tokio::spawn(tunnel_reconnect_loop(
            session,
            ssh_host.to_string(),
            ssh_port,
            ssh_user.to_string(),
            ssh_password.to_string(),
            ssh_key_path.to_string(),
            ssh_key_passphrase.to_string(),
            connect_timeout_secs,
            listener,
            remote_host.to_string(),
            remote_port,
        ));

        self.tunnels.lock().await.insert(connection_id.to_string(), (handle, local_port));

        Ok(local_port)
    }

    /// Open a tunnel from a SshTunnelConfig, returning the local port.
    pub async fn open_from_config(
        &self,
        connection_id: &str,
        config: &SshTunnelConfig,
        remote_host: &str,
        remote_port: u16,
    ) -> Result<u16, String> {
        let (ssh_password, ssh_key_path, ssh_key_passphrase) = match &config.auth {
            SshAuth::Password(pw) => (pw.clone(), String::new(), String::new()),
            SshAuth::PrivateKey { path, passphrase } => {
                (String::new(), path.clone(), passphrase.clone().unwrap_or_default())
            }
        };
        self.start_tunnel(
            connection_id,
            &config.host,
            config.port,
            &config.username,
            &ssh_password,
            &ssh_key_path,
            &ssh_key_passphrase,
            10,
            remote_host,
            remote_port,
            false,
        )
        .await
    }

    pub async fn local_port(&self, connection_id: &str) -> Option<u16> {
        self.tunnels.lock().await.get(connection_id).map(|(_, port)| *port)
    }

    pub async fn stop_tunnel(&self, connection_id: &str) {
        if let Some((handle, _)) = self.tunnels.lock().await.remove(connection_id) {
            handle.abort();
        }
    }
}

/// A single SSH tunnel handle (simpler API for one-off use).
pub struct SshTunnel {
    pub local_port: u16,
    _handle: JoinHandle<()>,
}

impl SshTunnel {
    pub async fn open(config: &SshTunnelConfig, remote_host: &str, remote_port: u16) -> Result<Self, String> {
        let local_port = portpicker::pick_unused_port().ok_or("No available port")?;
        let listener = TcpListener::bind(("127.0.0.1", local_port))
            .await
            .map_err(|e| format!("Failed to bind local port: {e}"))?;

        let (ssh_password, ssh_key_path, ssh_key_passphrase) = match &config.auth {
            SshAuth::Password(pw) => (pw.clone(), String::new(), String::new()),
            SshAuth::PrivateKey { path, passphrase } => {
                (String::new(), path.clone(), passphrase.clone().unwrap_or_default())
            }
        };

        let session = connect_and_authenticate(
            &config.host,
            config.port,
            &config.username,
            &ssh_password,
            &ssh_key_path,
            &ssh_key_passphrase,
            10,
        )
        .await?;

        let handle = tokio::spawn(tunnel_reconnect_loop(
            session,
            config.host.clone(),
            config.port,
            config.username.clone(),
            ssh_password,
            ssh_key_path,
            ssh_key_passphrase,
            10,
            listener,
            remote_host.to_string(),
            remote_port,
        ));

        Ok(Self { local_port, _handle: handle })
    }

    pub fn local_port(&self) -> u16 {
        self.local_port
    }
}

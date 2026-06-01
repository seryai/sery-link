use serde::de::DeserializeOwned;

use crate::agent_manager::{AgentManager, DEFAULT_JRE_KEY};
use crate::db::agent_driver::{AgentDriverClient, AgentMethod};

pub async fn stop_daemons(manager: &AgentManager) {
    manager.daemons.lock().await.clear();
}

pub async fn stop_daemon_by_key(manager: &AgentManager, agent_key: &str) {
    manager.daemons.lock().await.remove(agent_key);
}

pub async fn call_daemon<T: DeserializeOwned + Send + 'static>(
    manager: &AgentManager,
    driver_key: &str,
    method: &str,
    params: serde_json::Value,
) -> Result<T, String> {
    let key = driver_key.to_string();

    let mut daemons = manager.daemons.lock().await;

    if !daemons.contains_key(&key) {
        let client = spawn_client_for_key(manager, &key).await?;
        daemons.insert(key.clone(), client);
    }

    let client = daemons.get_mut(&key).unwrap();
    match client.call::<T>(method, params.clone()).await {
        Ok(result) => Ok(result),
        Err(err) => {
            log::warn!("[agent] daemon call failed, respawning: {err}");
            daemons.remove(&key);
            let mut new_client = spawn_client_for_key(manager, &key).await?;
            let result = new_client.call::<T>(method, params).await?;
            daemons.insert(key, new_client);
            Ok(result)
        }
    }
}

pub async fn call_daemon_method<T: DeserializeOwned + Send + 'static>(
    manager: &AgentManager,
    driver_key: &str,
    method: AgentMethod,
    params: serde_json::Value,
) -> Result<T, String> {
    call_daemon(manager, driver_key, method.as_str(), params).await
}

pub async fn spawn_client_for_key(manager: &AgentManager, key: &str) -> Result<AgentDriverClient, String> {
    let state = manager.load_state();
    let jre_key = state
        .installed_drivers
        .get(key)
        .map(|driver| driver.jre.as_str())
        .unwrap_or(DEFAULT_JRE_KEY);

    if !manager.is_driver_installed(key) {
        return Err(format!("{key} driver is not installed. Please install it from the Driver Manager."));
    }

    let java = manager.resolve_java_runtime(&state, jre_key)?.to_string_lossy().to_string();
    let jar = manager.driver_jar_path(key).to_string_lossy().to_string();
    let mut client = AgentDriverClient::spawn(&java, &jar).await?;
    client.try_optional_handshake(manager.agent_app_version()).await;
    Ok(client)
}

/// Spawn a fresh agent client using the JAR + JRE associated with `driver_key`.
///
/// Functionally identical to [`spawn_client_for_key`], but exposed under a
/// name that reflects the intent: callers maintaining their own per-source
/// daemon cache pass the driver key here, then store the client under a
/// composite cache key (typically `driver_key:source_id`) of their own.
pub async fn spawn_client_for_driver(manager: &AgentManager, driver_key: &str) -> Result<AgentDriverClient, String> {
    spawn_client_for_key(manager, driver_key).await
}

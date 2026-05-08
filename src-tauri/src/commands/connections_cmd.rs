use tauri::State;

use crate::core::clash_api::ClashClient;
use crate::error::{AppError, AppResult};
use crate::state::AppState;

fn client_from(state: &State<'_, AppState>) -> AppResult<ClashClient> {
    let g = state.core.lock();
    let addr = g
        .clash_api_addr
        .clone()
        .ok_or_else(|| AppError::ClashApi("core not running".into()))?;
    let secret = g
        .clash_api_secret
        .clone()
        .ok_or_else(|| AppError::ClashApi("core not running".into()))?;
    Ok(ClashClient::new(&addr, &secret))
}

#[tauri::command]
pub async fn connections_close(id: String, state: State<'_, AppState>) -> AppResult<()> {
    client_from(&state)?.close_connection(&id).await
}

#[tauri::command]
pub async fn connections_close_all(state: State<'_, AppState>) -> AppResult<()> {
    client_from(&state)?.close_all_connections().await
}

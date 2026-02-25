use leptos::prelude::*;

use yoink_shared::ServerAction;

/// Dispatch a user action to the server.
///
/// Called from the WASM client via `spawn_local`. The server executes the
/// action (mutates state, writes to DB, enqueues downloads, etc.) and fires
/// an SSE notification so all connected clients refresh.
#[server(
    name = DispatchAction,
    prefix = "/leptos",
    input = server_fn::codec::Json,
    output = server_fn::codec::Json
)]
pub async fn dispatch_action(action: ServerAction) -> Result<(), ServerFnError> {
    let ctx = leptos::context::use_context::<yoink_shared::ServerContext>()
        .ok_or_else(|| ServerFnError::new("Missing ServerContext"))?;

    (ctx.dispatch_action)(action)
        .await
        .map_err(ServerFnError::new)
}

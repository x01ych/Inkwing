use std::net::TcpListener;

use crate::error::AppResult;

/// Bind to 127.0.0.1:0, read the kernel-assigned port, drop. Inherently racy
/// (something else could grab it before sing-box does) but acceptable for a
/// single-user desktop app.
pub fn pick_free_port() -> AppResult<u16> {
    let listener = TcpListener::bind("127.0.0.1:0")?;
    let port = listener.local_addr()?.port();
    drop(listener);
    Ok(port)
}

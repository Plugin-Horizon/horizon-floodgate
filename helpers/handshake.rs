fn floodgate_handshake(
    host: &mut String,
    intent: i32,
    cancel: &mut bool,
    reason: &mut Option<horizon_client_core::text::Text>,
) {
    // The connection's own thread (native_handshake): no Game, nothing that blocks.
    let log = |level: &str, line: &str| eprintln!("[{level}] {line}");
    if let Some(text) = horizon_floodgate_core::on_handshake(host, intent, &log) {
        *cancel = true;
        *reason = Some(horizon_client_core::text::Text::literal(text));
    }
}

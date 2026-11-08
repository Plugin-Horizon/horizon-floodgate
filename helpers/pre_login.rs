fn floodgate_pre_login(
    defer: &mut Vec<std::sync::mpsc::Receiver<Result<(), horizon_client_core::text::Text>>>,
) {
    // The same connection thread as the handshake (native_player_pre_login): the Global Linking
    // lookup blocks only this login. A refusal is answered at once.
    let log = |level: &str, line: &str| eprintln!("[{level}] {line}");
    if let Err(text) = horizon_floodgate_core::on_pre_login(&log) {
        let (tx, rx) = std::sync::mpsc::channel();
        let _ = tx.send(Err(horizon_client_core::text::Text::literal(text)));
        defer.push(rx);
    }
}

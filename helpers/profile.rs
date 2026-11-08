fn floodgate_profile(player: u128) {
    // The same connection thread, the player admitted (native_player_profile).
    if let Some(line) = horizon_floodgate_core::on_profile(player) {
        eprintln!("[INFO] {line}");
    }
}

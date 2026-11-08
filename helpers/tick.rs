fn floodgate_tick(game: &mut crate::game::Game<'_>) {
    use horizon_floodgate_core::channels::Out;
    let players = game.native_server_players();
    let online: Vec<u128> = players.iter().map(|p| p.uuid).collect();
    for out in horizon_floodgate_core::channels::flush(&online) {
        match out {
            Out::Payload {
                player,
                channel,
                data,
            } => {
                if let Err(e) = game.native_send_custom_payload_to_player(player, channel, &data) {
                    eprintln!("[WARN] [floodgate] {channel}: {e}");
                }
            }
            Out::Kick { player, reason } => {
                // No kick helper: the vanilla command, with the player's name and Floodgate's text.
                if let Some(p) = players.iter().find(|p| p.uuid == player) {
                    game.native_run_command(&format!("kick {} {reason}", p.name));
                }
            }
            Out::ApplySkin { player, skin } => {
                let value = Some((skin.value.as_str(), Some(skin.signature.as_str())));
                if game.native_set_profile_property(player, "textures", value) {
                    game.native_refresh_player(player);
                }
            }
            Out::Log { level, line } => eprintln!("[{level}] {line}"),
        }
    }
}

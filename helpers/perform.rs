fn floodgate_perform(
    game: &mut crate::game::Game<'_>,
    player: &crate::game::native_server::NativeServerPlayer,
    command: &str,
    ran: &mut bool,
) {
    let config = horizon_floodgate_core::config();
    let sender = horizon_floodgate_core::commands::Sender {
        level: player.permission,
        uuid: player.uuid,
        name: player.name.clone(),
    };
    if let Some(lines) =
        horizon_floodgate_core::commands::run(command, Some(&sender), &config.player_link)
    {
        *ran = true;
        for line in lines {
            game.native_feedback(horizon_client::text::Text::literal(line));
        }
    }
}

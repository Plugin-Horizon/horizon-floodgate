fn floodgate_register(commands: &mut Vec<crate::game::native_server::NativeCommand>) {
    use crate::game::native_server::NativeCommand as C;
    let config = horizon_floodgate_core::config();
    for (usage, level, node) in horizon_floodgate_core::commands::usages(&config.player_link) {
        let command = if level == 0 {
            C::new(usage)
        } else {
            C::op(usage, level)
        };
        commands.push(match node {
            Some(node) => command.permission(node),
            None => command,
        });
    }
}

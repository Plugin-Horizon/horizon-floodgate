fn floodgate_payload(player: u128, channel: &str, data: &[u8]) {
    // native_custom_payload_from_client: every channel outside horizon:, floodgate: ones here.
    if channel.starts_with("floodgate:") {
        horizon_floodgate_core::channels::on_payload(player, channel, data);
    }
}

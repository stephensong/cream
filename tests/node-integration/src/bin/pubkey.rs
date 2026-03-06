fn main() {
    let name = std::env::args().nth(1).expect("Usage: pubkey <name>");
    let vk = if name.eq_ignore_ascii_case("root") {
        // Must match KeyManager::for_root() in the UI — uses root_signing_key's
        // verifying key (not the FROST group key) so admin-check pubkey matches.
        cream_common::identity::root_signing_key().verifying_key()
    } else {
        let (_, vk) = cream_node_integration::make_dummy_user(&name);
        vk
    };
    let hex: String = vk.as_bytes().iter().map(|b| format!("{:02x}", b)).collect();
    println!("{}", hex);
}

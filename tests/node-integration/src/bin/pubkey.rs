fn main() {
    let name = std::env::args().nth(1).expect("Usage: pubkey <name>");
    let (_, vk) = cream_node_integration::make_dummy_user(&name);
    let hex: String = vk.as_bytes().iter().map(|b| format!("{:02x}", b)).collect();
    println!("{}", hex);
}

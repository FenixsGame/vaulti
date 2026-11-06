fn main() {
    match std::env::args().nth(1).as_deref() {
        Some("reset-password") => vaulti_server::reset_password(),
        Some("--version") => {
            println!("{} {}", env!("CARGO_PKG_NAME"), env!("CARGO_PKG_VERSION"));
        }
        _ => vaulti_server::run(),
    }
}

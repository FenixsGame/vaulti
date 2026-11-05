fn main() {
    if std::env::args().nth(1).as_deref() == Some("reset-password") {
        vaulti_server::reset_password();
        return;
    }
    vaulti_server::run()
}

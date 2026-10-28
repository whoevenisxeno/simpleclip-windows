fn main() {
    let text = toml::to_string_pretty(&sc_core::config::Config::default()).unwrap();
    print!("{text}");
}

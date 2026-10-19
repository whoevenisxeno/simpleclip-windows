//! Prints the default config.toml, for the README: `cargo run -p sc-core --example print_default_config`
fn main() {
    let text = toml::to_string_pretty(&sc_core::config::Config::default()).unwrap();
    print!("{text}");
}

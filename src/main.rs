mod config;

use config::Config;

fn main() {
    let config = Config::from_env();

    println!(
        "Configuration loaded. Using CDN domain: {}",
        config.public_cdn_domain
    );
}

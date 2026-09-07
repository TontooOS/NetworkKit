use serde::Deserialize;
use std::sync::OnceLock;

const EN_US: &str = include_str!("../lang/en_us.json");
const DE_DE: &str = include_str!("../lang/de_de.json");

#[derive(Deserialize)]
struct Messages {
    not_available: String,
    permission_denied: String,
    timeout: String,
    command_failed: String,
    parse_error: String,
    io_error: String,
}

impl Messages {
    fn get(&self, key: &str) -> Option<&str> {
        match key {
            "not_available" => Some(&self.not_available),
            "permission_denied" => Some(&self.permission_denied),
            "timeout" => Some(&self.timeout),
            "command_failed" => Some(&self.command_failed),
            "parse_error" => Some(&self.parse_error),
            "io_error" => Some(&self.io_error),
            _ => None,
        }
    }
}

static MESSAGES: OnceLock<Messages> = OnceLock::new();

pub fn current_locale() -> &'static str {
    let lang = std::env::var("LC_ALL")
        .or_else(|_| std::env::var("LC_MESSAGES"))
        .or_else(|_| std::env::var("LANG"))
        .unwrap_or_default();

    if lang.to_lowercase().starts_with("de") {
        "de_de"
    } else {
        "en_us"
    }
}

fn messages() -> &'static Messages {
    MESSAGES.get_or_init(|| {
        let raw = match current_locale() {
            "de_de" => DE_DE,
            _ => EN_US,
        };
        serde_json::from_str(raw).expect("built-in language file is invalid")
    })
}

pub fn t(key: &str) -> String {
    match messages().get(key) {
        Some(msg) => msg.to_string(),
        None => key.to_string(),
    }
}

pub fn t_fmt(key: &str, arg: &str) -> String {
    t(key).replace("{}", arg)
}

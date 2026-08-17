// Prevents additional console window on Windows in release, DO NOT REMOVE!!
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    if std::env::var("HELM_ASKPASS_MODE").as_deref() == Ok("1") {
        let prompt = std::env::args().nth(1).unwrap_or_default();
        let response = std::env::var("HELM_ASKPASS_SECRET")
            .ok()
            .and_then(|secret| helm_lib::askpass::askpass_response(&prompt, &secret));
        match response {
            Some(response) => {
                println!("{response}");
                std::process::exit(0);
            }
            None => std::process::exit(1),
        }
    }
    helm_lib::run()
}

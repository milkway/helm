// Prevents additional console window on Windows in release, DO NOT REMOVE!!
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    if std::env::var("HELM_ASKPASS_MODE").as_deref() == Ok("1") {
        let prompt = std::env::args().nth(1).unwrap_or_default();
        let secret = std::env::var_os("HELM_ASKPASS_SECRET_FILE")
            .map(std::path::PathBuf::from)
            .and_then(|path| helm_lib::askpass::read_secret_file(&path));
        let response = secret
            .as_deref()
            .and_then(|secret| helm_lib::askpass::askpass_response(&prompt, secret));
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

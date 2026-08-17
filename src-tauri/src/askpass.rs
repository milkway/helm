use std::path::Path;
use zeroize::Zeroizing;

/// Responde somente a prompts de senha do OpenSSH.
///
/// Prompts de passphrase de chave e confirmações de host key não recebem
/// resposta, permitindo que o ssh tente o próximo método de autenticação.
pub fn askpass_response<'a>(prompt: &str, secret: &'a str) -> Option<&'a str> {
    let prompt = prompt.to_lowercase();
    const DANGEROUS_OR_NON_PASSWORD: &[&str] = &[
        "new",
        "retype",
        "again",
        "current",
        "change",
        "verification",
        "otp",
        "token",
    ];
    (prompt.contains("password")
        && !DANGEROUS_OR_NON_PASSWORD
            .iter()
            .any(|needle| prompt.contains(needle)))
    .then_some(secret)
}

/// Consome o segredo one-shot e remove o arquivo antes de produzir resposta.
/// Uma segunda invocação do helper não encontra o arquivo e falha fechada.
pub fn read_secret_file(path: &Path) -> Option<Zeroizing<String>> {
    let secret = std::fs::read_to_string(path).ok().map(Zeroizing::new);
    let _ = std::fs::remove_file(path);
    secret
}

#[cfg(test)]
mod tests {
    use super::{askpass_response, read_secret_file};

    #[test]
    fn responde_prompt_de_senha() {
        assert_eq!(
            askpass_response("deploy@example.com's Password:", "s3cr3t"),
            Some("s3cr3t")
        );
    }

    #[test]
    fn rejeita_passphrase_de_chave() {
        assert_eq!(
            askpass_response("Enter passphrase for key '/home/user/.ssh/id_ed25519':", "s3cr3t"),
            None
        );
    }

    #[test]
    fn rejeita_confirmacao_de_host_key() {
        assert_eq!(
            askpass_response(
                "Are you sure you want to continue connecting (yes/no/[fingerprint])?",
                "s3cr3t"
            ),
            None
        );
    }

    #[test]
    fn rejeita_prompt_vazio() {
        assert_eq!(askpass_response("", "s3cr3t"), None);
    }

    #[test]
    fn rejeita_prompts_pam_de_troca_de_senha() {
        for prompt in [
            "New password:",
            "Retype new password:",
            "Current password:",
            "Enter new UNIX password:",
            "Password again:",
            "Change password:",
        ] {
            assert_eq!(askpass_response(prompt, "s3cr3t"), None, "{prompt}");
        }
    }

    #[test]
    fn rejeita_prompts_de_segundo_fator() {
        for prompt in ["Verification password:", "OTP password:", "Token password:"] {
            assert_eq!(askpass_response(prompt, "s3cr3t"), None, "{prompt}");
        }
    }

    #[test]
    fn segredo_em_arquivo_e_one_shot() {
        let path = std::env::temp_dir().join(format!("helm-askpass-test-{}", std::process::id()));
        std::fs::write(&path, "s3cr3t").unwrap();

        let secret = read_secret_file(&path).unwrap();
        assert_eq!(secret.as_str(), "s3cr3t");
        assert!(!path.exists());
        assert!(read_secret_file(&path).is_none());
    }
}

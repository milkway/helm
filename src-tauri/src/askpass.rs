/// Responde somente a prompts de senha do OpenSSH.
///
/// Prompts de passphrase de chave e confirmações de host key não recebem
/// resposta, permitindo que o ssh tente o próximo método de autenticação.
pub fn askpass_response(prompt: &str, secret: &str) -> Option<String> {
    prompt
        .to_lowercase()
        .contains("password")
        .then(|| secret.to_string())
}

#[cfg(test)]
mod tests {
    use super::askpass_response;

    #[test]
    fn responde_prompt_de_senha() {
        assert_eq!(
            askpass_response("deploy@example.com's Password:", "s3cr3t"),
            Some("s3cr3t".to_string())
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
}

//! Autenticación compatible con contraseñas heredadas.

use pos_core::{Capability, RoleKind};
use rusqlite::{Connection, OptionalExtension};
use sha1::{Digest, Sha1};

use crate::error::StoreError;

/// Verifica una contraseña guardada en `people.APPPASSWORD`.
///
/// Formatos heredados: `sha1:<hex>`, `plain:<texto>`, `empty:` y NULL.
/// Cualquier otro valor se trata como texto plano legado.
pub fn verify_password(stored: Option<&str>, input: &str) -> bool {
    match stored {
        None => input.is_empty(),
        Some(value) if value.is_empty() || value == "empty:" => input.is_empty(),
        Some(value) => {
            if let Some(hash) = value.strip_prefix("sha1:") {
                let digest = sha1_hex(input);
                digest.eq_ignore_ascii_case(hash.trim())
            } else if let Some(plain) = value.strip_prefix("plain:") {
                input == plain
            } else {
                input == value
            }
        }
    }
}

/// Comprueba las credenciales contra `people` sin cargar todo el catálogo.
pub fn authenticate(
    conn: &Connection,
    user_id: &str,
    password: &str,
) -> Result<bool, StoreError> {
    let stored: Option<Option<String>> = conn
        .query_row(
            "SELECT APPPASSWORD FROM people WHERE ID = ?1 AND COALESCE(VISIBLE, 1) = 1",
            [user_id],
            |r| r.get(0),
        )
        .optional()?;
    let Some(stored) = stored else {
        return Ok(false);
    };
    Ok(verify_password(stored.as_deref(), password))
}

pub fn require_capability(
    conn: &Connection,
    user_id: &str,
    capability: Capability,
) -> Result<(), StoreError> {
    let role: Option<String> = conn
        .query_row(
            "SELECT COALESCE(ROLE, '') FROM people WHERE ID = ?1 AND COALESCE(VISIBLE, 1) = 1",
            [user_id],
            |r| r.get(0),
        )
        .optional()?;
    if role
        .as_deref()
        .and_then(RoleKind::from_legacy_id)
        .is_some_and(|role| role.can(capability))
    {
        Ok(())
    } else {
        Err(StoreError::PermissionDenied(capability))
    }
}

pub fn sha1_hex(value: &str) -> String {
    let mut hasher = Sha1::new();
    hasher.update(value.as_bytes());
    hex::encode(hasher.finalize())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn acepta_formatos_heredados() {
        assert!(verify_password(None, ""));
        assert!(!verify_password(None, "x"));
        assert!(verify_password(Some("empty:"), ""));
        assert!(verify_password(Some("plain:1234"), "1234"));
        assert!(!verify_password(Some("plain:1234"), "0000"));
        assert!(verify_password(Some("1234"), "1234"));
        let hash = format!("sha1:{}", sha1_hex("secreto"));
        assert!(verify_password(Some(&hash), "secreto"));
        assert!(!verify_password(Some(&hash), "otro"));
    }
}

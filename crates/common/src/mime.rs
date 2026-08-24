use thiserror::Error;

#[derive(Debug, Error, PartialEq, Eq)]
pub enum MimeTypeError {
    #[error(
        "unsupported file type `{0}`; supported types are: application/pdf, image/png, \
         image/jpeg, image/gif, image/webp"
    )]
    Unsupported(String),
}

/// Normalize a supported file extension or IANA media type to its MIME type.
pub fn normalize_mime_type(file_type: &str) -> Result<&'static str, MimeTypeError> {
    let normalized = file_type
        .split(';')
        .next()
        .unwrap_or(file_type)
        .trim()
        .to_ascii_lowercase();
    let normalized = normalized.strip_prefix('.').unwrap_or(&normalized);

    match normalized {
        "pdf" | "application/pdf" => Ok("application/pdf"),
        "png" | "image/png" => Ok("image/png"),
        "jpg" | "jpeg" | "image/jpeg" | "image/jpg" => Ok("image/jpeg"),
        "gif" | "image/gif" => Ok("image/gif"),
        "webp" | "image/webp" => Ok("image/webp"),
        _ => Err(MimeTypeError::Unsupported(file_type.trim().to_string())),
    }
}

#[cfg(test)]
mod tests {
    use super::normalize_mime_type;

    #[test]
    fn normalizes_extensions_and_mime_types() {
        assert_eq!(normalize_mime_type("PDF"), Ok("application/pdf"));
        assert_eq!(normalize_mime_type(".jpg"), Ok("image/jpeg"));
        assert_eq!(normalize_mime_type(" image/webp "), Ok("image/webp"));
        assert_eq!(normalize_mime_type("image/jpg"), Ok("image/jpeg"));
        assert_eq!(
            normalize_mime_type("application/pdf; charset=binary"),
            Ok("application/pdf")
        );
    }

    #[test]
    fn rejects_unsupported_types() {
        assert!(normalize_mime_type("application/octet-stream").is_err());
        assert!(normalize_mime_type("docx").is_err());
    }
}

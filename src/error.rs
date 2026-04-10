use std::io;

use thiserror::Error;

/// Errors that can occur during EPUB repair.
#[derive(Debug, Error)]
#[non_exhaustive]
pub enum RepubError {
    /// An I/O error occurred.
    #[error("I/O error: {source}")]
    Io {
        #[from]
        source: io::Error,
    },

    /// A ZIP archive error occurred.
    #[error("ZIP error: {source}")]
    Zip {
        #[from]
        source: zip::result::ZipError,
    },

    /// An XML parsing error occurred.
    #[error("XML error: {message}")]
    Xml { message: String },

    /// DRM was detected in the EPUB.
    #[error("DRM detected: {message}")]
    DrmDetected { message: String },

    /// The EPUB is invalid and cannot be repaired.
    #[error("invalid EPUB: {message}")]
    InvalidEpub { message: String },
}

impl From<quick_xml::Error> for RepubError {
    fn from(e: quick_xml::Error) -> Self {
        Self::Xml {
            message: e.to_string(),
        }
    }
}

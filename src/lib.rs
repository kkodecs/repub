//! # repub — The missing EPUB repair tool
//!
//! Standalone EPUB repair library that produces pristine, spec-compliant EPUBs.
//!
//! ```no_run
//! use repub::Repub;
//!
//! let report = Repub::new().fix("book.epub", "book.fixed.epub").unwrap();
//! for fix in &report.fixes {
//!     println!("{fix}");
//! }
//! ```

mod content_repair;
mod drm;

/// Extract the local name from a potentially namespace-prefixed XML name.
/// e.g. `dc:language` → `language`, `package` → `package`.
pub(crate) fn xml_local_name(name: &[u8]) -> &[u8] {
    name.iter()
        .position(|&b| b == b':')
        .map_or(name, |pos| &name[pos + 1..])
}
mod error;
mod ncx_repair;
mod opf_repair;
mod zip_repair;

pub use error::RepubError;

use std::fmt;
use std::fs;
use std::path::Path;

/// A fix that was applied to an EPUB.
#[derive(Clone, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum Fix {
    /// Mimetype ZIP entry was rewritten (stored, first entry, correct content).
    MimetypeFixed,
    /// XML declaration was added to an XHTML file.
    XmlDeclarationAdded { file: String },
    /// `dc:language` was added or replaced.
    LanguageAdded { language: String },
    /// `dc:identifier` was generated.
    IdentifierAdded { id: String },
    /// `<package unique-identifier>` reference was fixed.
    UniqueIdentifierFixed,
    /// `dcterms:modified` timestamp was added (EPUB3).
    ModifiedTimestampAdded,
    /// NCX body-ID fragment was stripped.
    NcxBodyIdFixed { file: String },
    /// `<img>` without `src` was removed.
    StrayImgRemoved { file: String },
    /// `<script>` tag was removed.
    ScriptRemoved { file: String },
    /// Vendor-specific or tool-specific metadata was removed.
    ProprietaryMetadataRemoved { detail: String },
}

impl fmt::Display for Fix {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::MimetypeFixed => write!(f, "Fixed mimetype ZIP entry"),
            Self::XmlDeclarationAdded { file } => {
                write!(f, "Added XML declaration to {file}")
            }
            Self::LanguageAdded { language } => {
                write!(f, "Added dc:language: {language}")
            }
            Self::IdentifierAdded { id } => {
                write!(f, "Added dc:identifier: {id}")
            }
            Self::UniqueIdentifierFixed => {
                write!(f, "Fixed package unique-identifier reference")
            }
            Self::ModifiedTimestampAdded => {
                write!(f, "Added dcterms:modified timestamp")
            }
            Self::NcxBodyIdFixed { file } => {
                write!(f, "Fixed NCX body-ID reference in {file}")
            }
            Self::StrayImgRemoved { file } => {
                write!(f, "Removed <img> without src in {file}")
            }
            Self::ScriptRemoved { file } => {
                write!(f, "Removed <script> tag from {file}")
            }
            Self::ProprietaryMetadataRemoved { detail } => {
                write!(f, "Removed proprietary metadata: {detail}")
            }
        }
    }
}

/// Report of all fixes applied to an EPUB.
#[derive(Clone, Debug, Default)]
pub struct RepubReport {
    /// Fixes that were applied.
    pub fixes: Vec<Fix>,
    /// Non-fatal issues noticed but not fixed.
    pub warnings: Vec<String>,
    /// Whether any changes were made.
    pub modified: bool,
}

/// EPUB repair tool.
///
/// # Examples
///
/// ```no_run
/// use repub::Repub;
///
/// // Simple usage
/// let report = Repub::new().fix("book.epub", "book.fixed.epub").unwrap();
///
/// // With options
/// let report = Repub::new()
///     .default_language("fr")
///     .strip_proprietary(true)
///     .fix("book.epub", "book.fixed.epub")
///     .unwrap();
/// ```
pub struct Repub {
    default_language: String,
    strip_proprietary: bool,
}

impl Default for Repub {
    fn default() -> Self {
        Self::new()
    }
}

impl Repub {
    /// Create a new `Repub` instance with default options.
    ///
    /// Defaults: language = `"en"`, strip proprietary = `true`.
    #[must_use]
    pub fn new() -> Self {
        Self {
            default_language: "en".to_owned(),
            strip_proprietary: true,
        }
    }

    /// Set the default language to use when `dc:language` is missing.
    #[must_use]
    pub fn default_language(mut self, language: &str) -> Self {
        language.clone_into(&mut self.default_language);
        self
    }

    /// Whether to strip vendor-specific and tool-specific metadata.
    ///
    /// Default: `true`.
    #[must_use]
    pub fn strip_proprietary(mut self, strip: bool) -> Self {
        self.strip_proprietary = strip;
        self
    }

    /// Fix an EPUB file and write the result to `output`.
    ///
    /// If `output` already exists, a backup is created at `{output}.repub.bak`
    /// before writing. The backup is removed on success.
    ///
    /// # Errors
    ///
    /// Returns [`RepubError::DrmDetected`] if DRM is found.
    /// Returns [`RepubError::InvalidEpub`] if the file is not a valid EPUB.
    /// Returns [`RepubError::Io`] on I/O failures.
    /// Returns [`RepubError::Zip`] on ZIP archive errors.
    /// Returns [`RepubError::Xml`] on XML parsing failures.
    pub fn fix(
        &self,
        input: impl AsRef<Path>,
        output: impl AsRef<Path>,
    ) -> Result<RepubReport, RepubError> {
        let input_bytes = fs::read(input.as_ref())?;
        let (fixed_bytes, report) = self.repair(&input_bytes)?;

        let output = output.as_ref();
        // Append .repub.bak to the full filename (not extension-replace).
        // "book.epub" → "book.epub.repub.bak", "book" → "book.repub.bak"
        let mut backup_name = output.file_name().unwrap_or_default().to_os_string();
        backup_name.push(".repub.bak");
        let backup_path = output.with_file_name(backup_name);

        // Back up existing output file before overwriting
        let backed_up = if output.exists() {
            fs::copy(output, &backup_path)?;
            true
        } else {
            false
        };

        // Write the fixed EPUB
        fs::write(output, &fixed_bytes)?;

        // Success — clean up backup
        if backed_up {
            let _ = fs::remove_file(&backup_path);
        }

        Ok(report)
    }

    /// Fix an EPUB from bytes and return the fixed bytes.
    ///
    /// # Errors
    ///
    /// Same error conditions as [`Repub::fix`].
    pub fn fix_bytes(&self, input: &[u8]) -> Result<Vec<u8>, RepubError> {
        let (fixed, _report) = self.repair(input)?;
        Ok(fixed)
    }

    /// Dry run: report what would be fixed without writing any output.
    ///
    /// # Errors
    ///
    /// Same error conditions as [`Repub::fix`].
    pub fn check(&self, input: impl AsRef<Path>) -> Result<RepubReport, RepubError> {
        let input_bytes = fs::read(input.as_ref())?;
        let (_fixed, report) = self.repair(&input_bytes)?;
        Ok(report)
    }

    fn repair(&self, input: &[u8]) -> Result<(Vec<u8>, RepubReport), RepubError> {
        if !opf_repair::is_valid_language_code(&self.default_language) {
            return Err(RepubError::InvalidEpub {
                message: format!(
                    "invalid default language: {:?} (expected ISO 639-1/639-2 code)",
                    self.default_language
                ),
            });
        }
        zip_repair::repair_epub(input, &self.default_language, self.strip_proprietary)
    }
}

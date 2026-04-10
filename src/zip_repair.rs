use std::collections::HashMap;
use std::io::{Cursor, Read, Write};

use quick_xml::events::Event;
use quick_xml::reader::Reader;
use zip::read::ZipArchive;
use zip::write::{SimpleFileOptions, ZipWriter};
use zip::CompressionMethod;

use crate::content_repair;
use crate::drm;
use crate::ncx_repair;
use crate::opf_repair;
use crate::{Fix, RepubError, RepubReport};

/// Core EPUB repair: selective-load ZIP rewrite.
///
/// Only OPF, XHTML, and NCX files are loaded into memory for inspection.
/// All other entries are copied with `raw_copy_file` (byte-for-byte,
/// preserving timestamps, permissions, extra fields, and compression).
///
/// # Errors
///
/// Returns [`RepubError`] on DRM detection, invalid EPUB structure,
/// I/O errors, or XML parsing failures.
pub(crate) fn repair_epub(
    input: &[u8],
    default_language: &str,
    strip_proprietary: bool,
) -> Result<(Vec<u8>, RepubReport), RepubError> {
    let mut archive = ZipArchive::new(Cursor::new(input))?;
    let mut fixes = Vec::new();
    let mut warnings = Vec::new();

    // 1. DRM check
    drm::check_drm(&mut archive)?;

    // 2. Find OPF path
    let opf_path = find_opf_path(&mut archive)?;

    // 3. Check mimetype
    if mimetype_needs_fix(&mut archive) {
        fixes.push(Fix::MimetypeFixed);
    }

    // 4. Collect entry metadata (no decompression — just names and compression)
    let mut entries = Vec::new();
    for i in 0..archive.len() {
        let entry = archive.by_index_raw(i)?;
        let name = entry.name().to_owned();
        entries.push(EntryInfo { name, index: i });
    }

    // 5. Selectively load only inspectable files (OPF, XHTML, NCX)
    let mut inspectable: HashMap<String, Vec<u8>> = HashMap::new();
    for info in &entries {
        if info.name == opf_path || is_xhtml(&info.name) || is_ncx(&info.name) {
            let data = read_entry(&mut archive, info.index)?;
            inspectable.insert(info.name.clone(), data);
        }
    }

    // 6. Compute all repairs using inspectable files only
    let opf_bytes = inspectable
        .get(&opf_path)
        .ok_or_else(|| RepubError::InvalidEpub {
            message: format!("OPF file not found in archive: {opf_path}"),
        })?;
    let (fixed_opf, opf_fixes) =
        opf_repair::repair_opf(opf_bytes, default_language, strip_proprietary)?;
    let opf_has_fixes = !opf_fixes.is_empty();
    fixes.extend(opf_fixes);

    let mut repaired_files: HashMap<String, Vec<u8>> = HashMap::new();
    for info in &entries {
        if is_xhtml(&info.name) {
            if let Some(raw) = inspectable.get(&info.name) {
                let (repaired, content_fixes, content_warnings) =
                    content_repair::repair_content(raw, &info.name);
                warnings.extend(content_warnings);
                if !content_fixes.is_empty() {
                    fixes.extend(content_fixes);
                    repaired_files.insert(info.name.clone(), repaired);
                }
            }
        } else if is_ncx(&info.name) {
            if let Some(raw) = inspectable.get(&info.name) {
                let (repaired, ncx_fixes) = ncx_repair::repair_ncx(raw, &info.name, &inspectable)?;
                if !ncx_fixes.is_empty() {
                    fixes.extend(ncx_fixes);
                    repaired_files.insert(info.name.clone(), repaired);
                }
            }
        }
    }

    // 7. If no fixes needed, return original bytes unchanged
    if fixes.is_empty() {
        return Ok((
            input.to_vec(),
            RepubReport {
                fixes,
                warnings,
                modified: false,
            },
        ));
    }

    // 8. Write output ZIP
    let mut archive = ZipArchive::new(Cursor::new(input))?;
    let mut output = ZipWriter::new(Cursor::new(Vec::new()));

    // mimetype MUST be first, stored, no extra fields
    // SimpleFileOptions::default() produces no extra fields in the local header
    let mime_opts = SimpleFileOptions::default().compression_method(CompressionMethod::Stored);
    output.start_file("mimetype", mime_opts)?;
    output.write_all(b"application/epub+zip")?;

    for info in &entries {
        if info.name == "mimetype" {
            continue; // Already written first
        }

        // Determine if this entry was modified
        let is_modified_opf = info.name == opf_path && opf_has_fixes;
        let has_repair = repaired_files.contains_key(&info.name);

        if is_modified_opf {
            // Write repaired OPF, preserving original timestamps/permissions
            let opts = options_from_entry(&mut archive, info.index);
            output.start_file(&info.name, opts)?;
            output.write_all(&fixed_opf)?;
        } else if has_repair {
            // Write repaired content/NCX, preserving original timestamps/permissions
            let opts = options_from_entry(&mut archive, info.index);
            let repaired = repaired_files.get(&info.name).expect("checked above");
            output.start_file(&info.name, opts)?;
            output.write_all(repaired)?;
        } else {
            // Unmodified — raw copy preserves everything byte-for-byte
            // (timestamps, permissions, extra fields, compression, comments)
            let entry = archive.by_index_raw(info.index)?;
            output.raw_copy_file(entry)?;
        }
    }

    let cursor = output.finish()?;
    let output_bytes = cursor.into_inner();

    let report = RepubReport {
        fixes,
        warnings,
        modified: true,
    };

    Ok((output_bytes, report))
}

/// Build `SimpleFileOptions` that preserves the original entry's metadata.
fn options_from_entry<R: Read + std::io::Seek>(
    archive: &mut ZipArchive<R>,
    index: usize,
) -> SimpleFileOptions {
    let Ok(entry) = archive.by_index_raw(index) else {
        return SimpleFileOptions::default();
    };
    let mut opts = SimpleFileOptions::default().compression_method(entry.compression());
    if let Some(time) = entry.last_modified() {
        opts = opts.last_modified_time(time);
    }
    if let Some(mode) = entry.unix_mode() {
        opts = opts.unix_permissions(mode);
    }
    opts
}

struct EntryInfo {
    name: String,
    index: usize,
}

fn read_entry<R: Read + std::io::Seek>(
    archive: &mut ZipArchive<R>,
    index: usize,
) -> Result<Vec<u8>, RepubError> {
    let mut entry = archive.by_index(index)?;
    let mut data = Vec::new();
    entry.read_to_end(&mut data)?;
    Ok(data)
}

fn find_opf_path<R: Read + std::io::Seek>(
    archive: &mut ZipArchive<R>,
) -> Result<String, RepubError> {
    let mut entry =
        archive
            .by_name("META-INF/container.xml")
            .map_err(|_| RepubError::InvalidEpub {
                message: "missing META-INF/container.xml".into(),
            })?;

    let mut xml = String::new();
    entry.read_to_string(&mut xml)?;

    let mut reader = Reader::from_str(&xml);
    loop {
        match reader.read_event() {
            Ok(Event::Empty(ref e) | Event::Start(ref e)) => {
                if e.name().as_ref() == b"rootfile" {
                    for attr in e.attributes().flatten() {
                        if attr.key.as_ref() == b"full-path" {
                            return Ok(String::from_utf8_lossy(&attr.value).into_owned());
                        }
                    }
                }
            }
            Ok(Event::Eof) => break,
            Err(e) => {
                return Err(RepubError::Xml {
                    message: e.to_string(),
                })
            }
            _ => {}
        }
    }

    Err(RepubError::InvalidEpub {
        message: "no rootfile found in container.xml".into(),
    })
}

fn mimetype_needs_fix<R: Read + std::io::Seek>(archive: &mut ZipArchive<R>) -> bool {
    let Ok(mut entry) = archive.by_index(0) else {
        return true;
    };
    if entry.name() != "mimetype" {
        return true;
    }
    if entry.compression() != CompressionMethod::Stored {
        return true;
    }
    let mut buf = Vec::new();
    if entry.read_to_end(&mut buf).is_err() {
        return true;
    }
    buf.as_slice() != b"application/epub+zip"
}

#[allow(clippy::case_sensitive_file_extension_comparisons)]
fn is_xhtml(name: &str) -> bool {
    let lower = name.to_lowercase();
    lower.ends_with(".xhtml") || lower.ends_with(".html") || lower.ends_with(".htm")
}

#[allow(clippy::case_sensitive_file_extension_comparisons)]
fn is_ncx(name: &str) -> bool {
    name.to_lowercase().ends_with(".ncx")
}

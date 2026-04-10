use std::collections::HashMap;

use quick_xml::events::{BytesStart, Event};
use quick_xml::reader::Reader;
use quick_xml::writer::Writer;
use std::io::Cursor;

use crate::{xml_local_name as local_name, Fix, RepubError};

/// Repair NCX body-ID fragment references.
///
/// When an NCX `<content src="file.xhtml#id"/>` points at a `<body id="...">`,
/// the fragment is stripped because it crashes some EPUB converters.
///
/// # Errors
///
/// Returns [`RepubError::Xml`] if the NCX cannot be parsed.
pub(crate) fn repair_ncx(
    ncx_bytes: &[u8],
    ncx_name: &str,
    all_files: &HashMap<String, Vec<u8>>,
) -> Result<(Vec<u8>, Vec<Fix>), RepubError> {
    let input_str = std::str::from_utf8(ncx_bytes).map_err(|_| RepubError::InvalidEpub {
        message: format!("NCX file is not valid UTF-8: {ncx_name}"),
    })?;

    let ncx_dir = ncx_name.rsplit_once('/').map_or("", |(dir, _)| dir);

    let mut reader = Reader::from_str(input_str);
    reader.config_mut().trim_text(false);
    let mut writer = Writer::new(Cursor::new(Vec::new()));
    let mut fixes = Vec::new();

    loop {
        match reader.read_event() {
            Ok(Event::Empty(ref e)) => {
                let qname = e.name();
                let local = local_name(qname.as_ref());
                if local == b"content" {
                    if let Some(fixed) =
                        try_fix_content_src(e, ncx_dir, ncx_name, all_files, &mut fixes)
                    {
                        writer
                            .write_event(Event::Empty(fixed))
                            .map_err(|err| RepubError::Xml {
                                message: err.to_string(),
                            })?;
                        continue;
                    }
                }
                writer
                    .write_event(Event::Empty(e.clone().into_owned()))
                    .map_err(|err| RepubError::Xml {
                        message: err.to_string(),
                    })?;
            }
            Ok(Event::Start(ref e)) => {
                let qname = e.name();
                let local = local_name(qname.as_ref());
                if local == b"content" {
                    if let Some(fixed) =
                        try_fix_content_src(e, ncx_dir, ncx_name, all_files, &mut fixes)
                    {
                        // Write as empty element, skip content until </content>
                        writer
                            .write_event(Event::Empty(fixed))
                            .map_err(|err| RepubError::Xml {
                                message: err.to_string(),
                            })?;
                        // Consume until matching End (track depth for nested elements)
                        let mut depth = 1u32;
                        loop {
                            match reader.read_event() {
                                Ok(Event::Start(_)) => depth += 1,
                                Ok(Event::End(_)) => {
                                    depth -= 1;
                                    if depth == 0 {
                                        break;
                                    }
                                }
                                Ok(Event::Eof) | Err(_) => break,
                                _ => {}
                            }
                        }
                        continue;
                    }
                }
                writer
                    .write_event(Event::Start(e.clone().into_owned()))
                    .map_err(|err| RepubError::Xml {
                        message: err.to_string(),
                    })?;
            }
            Ok(Event::Eof) => break,
            Ok(ref e) => {
                writer
                    .write_event(e.clone().into_owned())
                    .map_err(|err| RepubError::Xml {
                        message: err.to_string(),
                    })?;
            }
            Err(e) => {
                return Err(RepubError::Xml {
                    message: e.to_string(),
                })
            }
        }
    }

    let output = writer.into_inner().into_inner();
    Ok((output, fixes))
}

/// If the `<content>` element's `src` attribute has a fragment that targets the
/// `<body>` element's id, return a new element with the fragment stripped.
fn try_fix_content_src(
    e: &BytesStart<'_>,
    ncx_dir: &str,
    ncx_name: &str,
    all_files: &HashMap<String, Vec<u8>>,
    fixes: &mut Vec<Fix>,
) -> Option<BytesStart<'static>> {
    // Find the src attribute
    let src_attr = e
        .attributes()
        .flatten()
        .find(|a| a.key.as_ref() == b"src")?;

    // Work on raw bytes to avoid lossy UTF-8 conversion corrupting paths
    let raw_src = src_attr.value.as_ref();
    let hash_pos = raw_src.iter().position(|&b| b == b'#')?;
    let file_bytes = &raw_src[..hash_pos];
    let fragment_bytes = &raw_src[hash_pos + 1..];

    // Convert to str for path operations (bail if not valid UTF-8)
    let file_part = std::str::from_utf8(file_bytes).ok()?;
    let fragment = std::str::from_utf8(fragment_bytes).ok()?;

    // URL-decode and resolve relative path (collapse . and ..)
    let decoded = percent_decode(file_part);
    let full_path = if ncx_dir.is_empty() {
        normalize_zip_path(&decoded)
    } else {
        normalize_zip_path(&format!("{ncx_dir}/{decoded}"))
    };

    // Look up the XHTML file
    let xhtml_bytes = all_files.get(&full_path)?;

    // Check if the fragment targets <body id="...">
    if !is_body_id(xhtml_bytes, fragment) {
        return None;
    }

    // Build a new <content> element with the fragment stripped
    let ename = e.name();
    let tag = std::str::from_utf8(ename.as_ref()).unwrap_or("content");
    let mut new_elem = BytesStart::new(tag.to_owned());
    for attr in e.attributes().flatten() {
        if attr.key.as_ref() == b"src" {
            // Use raw Attribute to avoid double-escaping already-escaped values
            new_elem.push_attribute(quick_xml::events::attributes::Attribute {
                key: quick_xml::name::QName(b"src"),
                value: std::borrow::Cow::Owned(file_part.as_bytes().to_vec()),
            });
        } else {
            // Preserve raw bytes — no lossy UTF-8 conversion
            new_elem.push_attribute(attr);
        }
    }

    fixes.push(Fix::NcxBodyIdFixed {
        file: ncx_name.to_owned(),
    });

    Some(new_elem)
}

/// Returns `true` if `fragment` matches the `id` attribute of the `<body>`
/// element in the given XHTML.
fn is_body_id(xhtml: &[u8], fragment: &str) -> bool {
    let Ok(s) = std::str::from_utf8(xhtml) else {
        return false;
    };
    let mut reader = Reader::from_str(s);
    loop {
        match reader.read_event() {
            Ok(Event::Start(ref e) | Event::Empty(ref e)) => {
                let qname = e.name();
                if local_name(qname.as_ref()) == b"body" {
                    for attr in e.attributes().flatten() {
                        if attr.key.as_ref() == b"id" {
                            return String::from_utf8_lossy(&attr.value) == fragment;
                        }
                    }
                    return false; // <body> found but no id
                }
            }
            Ok(Event::Eof) | Err(_) => break,
            _ => {}
        }
    }
    false
}

/// Simple percent-decoding for URL-encoded paths (e.g. `chapter%2001.xhtml`).
fn percent_decode(input: &str) -> String {
    let mut out = Vec::with_capacity(input.len());
    let bytes = input.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' && i + 2 < bytes.len() {
            if let (Some(hi), Some(lo)) = (hex_val(bytes[i + 1]), hex_val(bytes[i + 2])) {
                out.push(hi << 4 | lo);
                i += 3;
                continue;
            }
        }
        out.push(bytes[i]);
        i += 1;
    }
    String::from_utf8(out).unwrap_or_else(|_| input.to_owned())
}

fn hex_val(b: u8) -> Option<u8> {
    match b {
        b'0'..=b'9' => Some(b - b'0'),
        b'a'..=b'f' => Some(b - b'a' + 10),
        b'A'..=b'F' => Some(b - b'A' + 10),
        _ => None,
    }
}

/// Normalize a ZIP-internal path by collapsing `.` and `..` segments.
/// e.g. `OEBPS/../Text/chapter01.xhtml` → `Text/chapter01.xhtml`
fn normalize_zip_path(path: &str) -> String {
    let mut parts: Vec<&str> = Vec::new();
    for segment in path.split('/') {
        match segment {
            "" | "." => {}
            ".." => {
                parts.pop();
            }
            other => parts.push(other),
        }
    }
    parts.join("/")
}

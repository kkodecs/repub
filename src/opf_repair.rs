use quick_xml::events::{BytesEnd, BytesStart, BytesText, Event};
use quick_xml::reader::Reader;
use quick_xml::writer::Writer;
use std::io::Cursor;
use uuid::Uuid;

use crate::{xml_local_name as local_name, Fix, RepubError};

// ---------------------------------------------------------------------------
// Analysis (pass 1)
// ---------------------------------------------------------------------------

#[allow(clippy::struct_excessive_bools)]
struct OpfState {
    version: String,
    unique_id_ref: String,
    has_language: bool,
    language_valid: bool,
    has_identifier: bool,
    identifier_ids: Vec<String>,
    vendor_identifier_ids: Vec<String>,
    has_modified: bool,
    /// Contributor IDs that have role=bkp via EPUB3 <meta refines> pattern
    bkp_contributor_ids: Vec<String>,
}

/// Repair OPF metadata.
///
/// # Errors
///
/// Returns [`RepubError::Xml`] if the OPF cannot be parsed.
pub(crate) fn repair_opf(
    input: &[u8],
    default_language: &str,
    strip_proprietary: bool,
) -> Result<(Vec<u8>, Vec<Fix>), RepubError> {
    let input_str = std::str::from_utf8(input).map_err(|_| RepubError::InvalidEpub {
        message: "OPF is not valid UTF-8".into(),
    })?;

    let state = analyze(input_str)?;
    rewrite(input_str, &state, default_language, strip_proprietary)
}

#[allow(clippy::too_many_lines)]
fn analyze(input: &str) -> Result<OpfState, RepubError> {
    let mut reader = Reader::from_str(input);
    reader.config_mut().trim_text(false);

    let mut state = OpfState {
        version: String::new(),
        unique_id_ref: String::new(),
        has_language: false,
        language_valid: false,
        has_identifier: false,
        identifier_ids: Vec::new(),
        vendor_identifier_ids: Vec::new(),
        has_modified: false,
        bkp_contributor_ids: Vec::new(),
    };

    let mut in_metadata = false;
    let mut reading_language = false;
    // Track current identifier for content-based vendor detection (buffer text)
    let mut current_identifier_id: Option<String> = None;
    let mut current_identifier_is_attr_vendor = false;
    let mut current_identifier_text = String::new();
    // Track EPUB3 <meta refines="#id" property="role"> for bkp detection
    let mut current_refines_target: Option<String> = None;
    let mut reading_role_meta = false;

    loop {
        match reader.read_event() {
            Ok(Event::Start(ref e) | Event::Empty(ref e)) => {
                let qname = e.name();
                let local = local_name(qname.as_ref());

                match local {
                    b"package" => {
                        for attr in e.attributes().flatten() {
                            match attr.key.as_ref() {
                                b"version" => {
                                    state.version =
                                        String::from_utf8_lossy(&attr.value).into_owned();
                                }
                                b"unique-identifier" => {
                                    state.unique_id_ref =
                                        String::from_utf8_lossy(&attr.value).into_owned();
                                }
                                _ => {}
                            }
                        }
                    }
                    b"metadata" => in_metadata = true,
                    b"language" if in_metadata => {
                        state.has_language = true;
                        reading_language = true;
                    }
                    b"identifier" if in_metadata => {
                        state.has_identifier = true;
                        let is_attr_vendor = is_vendor_identifier_by_attr(e);
                        current_identifier_is_attr_vendor = is_attr_vendor;
                        current_identifier_id = None;
                        for attr in e.attributes().flatten() {
                            if attr.key.as_ref() == b"id" {
                                let id = String::from_utf8_lossy(&attr.value).into_owned();
                                current_identifier_id = Some(id.clone());
                                state.identifier_ids.push(id);
                            }
                        }
                        if is_attr_vendor {
                            if let Some(ref id) = current_identifier_id {
                                state.vendor_identifier_ids.push(id.clone());
                            }
                        }
                    }
                    b"meta" if in_metadata => {
                        let mut has_role_property = false;
                        let mut refines_id = None;
                        for attr in e.attributes().flatten() {
                            match attr.key.as_ref() {
                                b"property" => {
                                    let val = attr.value.as_ref();
                                    if val == b"dcterms:modified" {
                                        state.has_modified = true;
                                    }
                                    if val == b"role" {
                                        has_role_property = true;
                                    }
                                }
                                b"refines" => {
                                    let val = String::from_utf8_lossy(&attr.value);
                                    if let Some(id) = val.strip_prefix('#') {
                                        refines_id = Some(id.to_owned());
                                    }
                                }
                                _ => {}
                            }
                        }
                        // EPUB3: <meta refines="#id" property="role">bkp</meta>
                        if has_role_property && refines_id.is_some() {
                            current_refines_target = refines_id;
                            reading_role_meta = true;
                        }
                    }
                    _ => {}
                }
            }
            Ok(Event::Text(ref e)) => {
                if reading_language {
                    if let Ok(text) = e.unescape() {
                        // Valid if ANY language element is valid (don't let a later
                        // invalid element overwrite an earlier valid one)
                        if is_valid_language(text.trim()) {
                            state.language_valid = true;
                        }
                    }
                    reading_language = false;
                } else if reading_role_meta {
                    // EPUB3 role meta: check if the role is "bkp"
                    if let Ok(text) = e.unescape() {
                        if text.trim().eq_ignore_ascii_case("bkp") {
                            if let Some(ref id) = current_refines_target {
                                state.bkp_contributor_ids.push(id.clone());
                            }
                        }
                    }
                    reading_role_meta = false;
                    current_refines_target = None;
                } else if current_identifier_id.is_some() && !current_identifier_is_attr_vendor {
                    // Buffer text — check on End event after all chunks collected
                    if let Ok(text) = e.unescape() {
                        current_identifier_text.push_str(&text);
                    }
                }
            }
            Ok(Event::End(ref e)) => {
                let qname = e.name();
                let local = local_name(qname.as_ref());
                if local == b"metadata" {
                    in_metadata = false;
                }
                if local == b"language" && reading_language {
                    reading_language = false;
                }
                if local == b"identifier" {
                    // Content-based vendor check on buffered text
                    if !current_identifier_is_attr_vendor
                        && is_vendor_identifier_by_content(current_identifier_text.trim())
                    {
                        if let Some(ref id) = current_identifier_id {
                            if !state.vendor_identifier_ids.contains(id) {
                                state.vendor_identifier_ids.push(id.clone());
                            }
                        }
                    }
                    current_identifier_id = None;
                    current_identifier_is_attr_vendor = false;
                    current_identifier_text.clear();
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

    Ok(state)
}

// ---------------------------------------------------------------------------
// Rewrite (pass 2)
// ---------------------------------------------------------------------------

#[allow(clippy::too_many_lines)]
fn rewrite(
    input: &str,
    state: &OpfState,
    default_language: &str,
    strip_proprietary: bool,
) -> Result<(Vec<u8>, Vec<Fix>), RepubError> {
    let mut reader = Reader::from_str(input);
    reader.config_mut().trim_text(false);
    let mut writer = Writer::new(Cursor::new(Vec::new()));
    let mut fixes = Vec::new();

    let is_epub3 = state.version.starts_with('3');
    let need_language = !state.has_language || !state.language_valid;

    // Compute identifier state AFTER vendor stripping
    let surviving_ids: Vec<&str> = if strip_proprietary {
        state
            .identifier_ids
            .iter()
            .filter(|id| !state.vendor_identifier_ids.contains(id))
            .map(String::as_str)
            .collect()
    } else {
        state.identifier_ids.iter().map(String::as_str).collect()
    };
    let has_surviving_id = !surviving_ids.is_empty();
    let need_identifier = !has_surviving_id;
    let add_modified = is_epub3 && !state.has_modified;
    let unique_id_broken = (!state.unique_id_ref.is_empty()
        && !surviving_ids.contains(&state.unique_id_ref.as_str()))
        || (state.unique_id_ref.is_empty() && has_surviving_id);

    let new_uuid = Uuid::new_v4();
    let new_id_value = format!("urn:uuid:{new_uuid}");

    let mut in_metadata = false;
    let mut skip_depth: usize = 0;
    let mut replacing_language = false;
    // For content-based identifier detection during rewrite.
    // Buffers ALL events (not just text) between Start and End to handle nested elements.
    let mut reading_identifier = false;
    let mut identifier_text_buf = String::new();
    let mut identifier_event_buf: Vec<Event<'static>> = Vec::new();
    // For contributor detection
    let mut reading_contributor = false;
    let mut contributor_is_bkp = false;
    let mut contributor_text_buf = String::new();
    let mut contributor_event_buf: Vec<Event<'static>> = Vec::new();

    loop {
        match reader.read_event() {
            Ok(Event::Start(ref e)) => {
                if skip_depth > 0 {
                    skip_depth += 1;
                    continue;
                }
                // Buffer nested elements inside identifier/contributor
                if reading_identifier {
                    identifier_event_buf.push(Event::Start(e.clone().into_owned()));
                    continue;
                }
                if reading_contributor {
                    contributor_event_buf.push(Event::Start(e.clone().into_owned()));
                    continue;
                }

                let qname = e.name();
                let local = local_name(qname.as_ref());
                let full_name = qname.as_ref();

                // --- package element: fix unique-identifier + strip vendor xmlns ---
                if local == b"package" {
                    let needs_uid_fix =
                        unique_id_broken || (need_identifier && state.unique_id_ref.is_empty());
                    if needs_uid_fix || strip_proprietary {
                        let target_id = if need_identifier {
                            "repub-id"
                        } else {
                            surviving_ids.first().copied().unwrap_or("repub-id")
                        };
                        let tag = std::str::from_utf8(full_name).unwrap_or("package");
                        let mut elem = BytesStart::new(tag);
                        let mut had_uid = false;
                        for attr in e.attributes().flatten() {
                            if needs_uid_fix && attr.key.as_ref() == b"unique-identifier" {
                                elem.push_attribute(("unique-identifier", target_id));
                                had_uid = true;
                            } else if strip_proprietary
                                && is_vendor_xmlns(attr.key.as_ref(), &attr.value)
                            {
                                fixes.push(Fix::ProprietaryMetadataRemoved {
                                    detail: format!(
                                        "namespace {}",
                                        String::from_utf8_lossy(attr.key.as_ref())
                                    ),
                                });
                            } else if strip_proprietary && attr.key.as_ref() == b"prefix" {
                                // Filter vendor entries from EPUB3 prefix attribute
                                let cleaned =
                                    strip_vendor_prefixes(&String::from_utf8_lossy(&attr.value));
                                if cleaned.is_empty() {
                                    fixes.push(Fix::ProprietaryMetadataRemoved {
                                        detail: "vendor prefix declarations".into(),
                                    });
                                } else {
                                    elem.push_attribute(("prefix", cleaned.as_str()));
                                    if cleaned.len() < String::from_utf8_lossy(&attr.value).len() {
                                        fixes.push(Fix::ProprietaryMetadataRemoved {
                                            detail: "vendor prefix declarations".into(),
                                        });
                                    }
                                }
                            } else {
                                elem.push_attribute(attr);
                            }
                        }
                        if needs_uid_fix && !had_uid {
                            elem.push_attribute(("unique-identifier", target_id));
                        }
                        writer.write_event(Event::Start(elem))?;
                        if needs_uid_fix {
                            fixes.push(Fix::UniqueIdentifierFixed);
                        }
                        continue;
                    }
                    writer.write_event(Event::Start(e.clone().into_owned()))?;
                    continue;
                }

                // --- metadata open: strip vendor xmlns ---
                if local == b"metadata" {
                    in_metadata = true;
                    if strip_proprietary {
                        let tag = std::str::from_utf8(full_name).unwrap_or("metadata");
                        let mut elem = BytesStart::new(tag);
                        for attr in e.attributes().flatten() {
                            if is_vendor_xmlns(attr.key.as_ref(), &attr.value) {
                                fixes.push(Fix::ProprietaryMetadataRemoved {
                                    detail: format!(
                                        "namespace {}",
                                        String::from_utf8_lossy(attr.key.as_ref())
                                    ),
                                });
                            } else {
                                elem.push_attribute(attr);
                            }
                        }
                        writer.write_event(Event::Start(elem))?;
                    } else {
                        writer.write_event(Event::Start(e.clone().into_owned()))?;
                    }
                    continue;
                }

                // --- replace invalid language ---
                if in_metadata
                    && local == b"language"
                    && state.has_language
                    && !state.language_valid
                {
                    replacing_language = true;
                    writer.write_event(Event::Start(e.clone().into_owned()))?;
                    continue;
                }

                // --- skip vendor identifier (attribute-based) ---
                if in_metadata
                    && local == b"identifier"
                    && strip_proprietary
                    && is_vendor_identifier_by_attr(e)
                {
                    let detail = vendor_detail(e);
                    fixes.push(Fix::ProprietaryMetadataRemoved { detail });
                    skip_depth = 1;
                    continue;
                }

                // --- identifier: buffer ALL events for content-based vendor detection ---
                if in_metadata && local == b"identifier" && strip_proprietary {
                    reading_identifier = true;
                    identifier_text_buf.clear();
                    identifier_event_buf.clear();
                    identifier_event_buf.push(Event::Start(e.clone().into_owned()));
                    continue;
                }

                // --- contributor: buffer ALL events for bkp tool detection ---
                if in_metadata && local == b"contributor" && strip_proprietary {
                    contributor_is_bkp =
                        has_role_bkp(e) || has_bkp_via_refines(e, &state.bkp_contributor_ids);
                    reading_contributor = true;
                    contributor_text_buf.clear();
                    contributor_event_buf.clear();
                    contributor_event_buf.push(Event::Start(e.clone().into_owned()));
                    continue;
                }

                // --- pass through ---
                writer.write_event(Event::Start(e.clone().into_owned()))?;
            }

            Ok(Event::Empty(ref e)) => {
                if skip_depth > 0 {
                    continue;
                }
                if reading_identifier {
                    identifier_event_buf.push(Event::Empty(e.clone().into_owned()));
                    continue;
                }
                if reading_contributor {
                    contributor_event_buf.push(Event::Empty(e.clone().into_owned()));
                    continue;
                }

                let qn = e.name();
                let local = local_name(qn.as_ref());

                // --- replace self-closing <dc:language/> ---
                if in_metadata
                    && local == b"language"
                    && state.has_language
                    && !state.language_valid
                {
                    let tag = std::str::from_utf8(qn.as_ref()).unwrap_or("dc:language");
                    writer.write_event(Event::Start(BytesStart::new(tag)))?;
                    writer.write_event(Event::Text(BytesText::new(default_language)))?;
                    writer.write_event(Event::End(BytesEnd::new(tag)))?;
                    fixes.push(Fix::LanguageAdded {
                        language: default_language.to_owned(),
                    });
                    continue;
                }

                if in_metadata && strip_proprietary {
                    if local == b"meta" && is_tool_metadata(e) {
                        let detail = tool_meta_detail(e);
                        fixes.push(Fix::ProprietaryMetadataRemoved { detail });
                        continue;
                    }
                    if local == b"identifier" && is_vendor_identifier_by_attr(e) {
                        let detail = vendor_detail(e);
                        fixes.push(Fix::ProprietaryMetadataRemoved { detail });
                        continue;
                    }
                }

                writer.write_event(Event::Empty(e.clone().into_owned()))?;
            }

            Ok(Event::Text(ref e)) => {
                if skip_depth > 0 {
                    continue;
                }
                if replacing_language {
                    writer.write_event(Event::Text(BytesText::new(default_language)))?;
                    fixes.push(Fix::LanguageAdded {
                        language: default_language.to_owned(),
                    });
                    replacing_language = false;
                    continue;
                }
                if reading_identifier {
                    if let Ok(text) = e.unescape() {
                        identifier_text_buf.push_str(&text);
                    }
                    identifier_event_buf.push(Event::Text(e.clone().into_owned()));
                    continue;
                }
                if reading_contributor {
                    if let Ok(text) = e.unescape() {
                        contributor_text_buf.push_str(&text);
                    }
                    contributor_event_buf.push(Event::Text(e.clone().into_owned()));
                    continue;
                }
                writer.write_event(Event::Text(e.clone().into_owned()))?;
            }

            Ok(Event::End(ref e)) => {
                if skip_depth > 0 {
                    skip_depth -= 1;
                    continue;
                }

                let qn = e.name();
                let local = local_name(qn.as_ref());

                // Buffer nested End events inside identifier/contributor
                if reading_identifier && local != b"identifier" {
                    identifier_event_buf.push(Event::End(e.clone().into_owned()));
                    continue;
                }
                if reading_contributor && local != b"contributor" {
                    contributor_event_buf.push(Event::End(e.clone().into_owned()));
                    continue;
                }

                if local == b"language" && replacing_language {
                    writer.write_event(Event::Text(BytesText::new(default_language)))?;
                    fixes.push(Fix::LanguageAdded {
                        language: default_language.to_owned(),
                    });
                    replacing_language = false;
                }

                // --- flush buffered identifier ---
                if local == b"identifier" && reading_identifier {
                    reading_identifier = false;
                    if is_vendor_identifier_by_content(identifier_text_buf.trim()) {
                        fixes.push(Fix::ProprietaryMetadataRemoved {
                            detail: format!("vendor identifier ({})", identifier_text_buf.trim()),
                        });
                        // Drop entire element — discard the buffer
                    } else {
                        // Not vendor — write all buffered events + the End
                        for buffered in &identifier_event_buf {
                            writer.write_event(buffered.clone())?;
                        }
                        writer.write_event(Event::End(e.clone().into_owned()))?;
                    }
                    identifier_event_buf.clear();
                    continue;
                }

                // --- flush buffered contributor ---
                if local == b"contributor" && reading_contributor {
                    reading_contributor = false;
                    if contributor_is_bkp && is_tool_contributor(contributor_text_buf.trim()) {
                        fixes.push(Fix::ProprietaryMetadataRemoved {
                            detail: format!("tool contributor ({})", contributor_text_buf.trim()),
                        });
                        // Drop entire element
                    } else {
                        // Not a tool stamp — write all buffered events + the End
                        for buffered in &contributor_event_buf {
                            writer.write_event(buffered.clone())?;
                        }
                        writer.write_event(Event::End(e.clone().into_owned()))?;
                    }
                    contributor_event_buf.clear();
                    continue;
                }

                if local == b"metadata" {
                    // Insert missing elements before closing </metadata>
                    if need_language && !state.has_language {
                        write_text(&mut writer, "\n    ")?;
                        write_element(&mut writer, "dc:language", &[], default_language)?;
                        fixes.push(Fix::LanguageAdded {
                            language: default_language.to_owned(),
                        });
                    }
                    if need_identifier {
                        write_text(&mut writer, "\n    ")?;
                        write_element(
                            &mut writer,
                            "dc:identifier",
                            &[("id", "repub-id")],
                            &new_id_value,
                        )?;
                        fixes.push(Fix::IdentifierAdded {
                            id: new_id_value.clone(),
                        });
                    }
                    if add_modified {
                        let ts = utc_timestamp_now();
                        write_text(&mut writer, "\n    ")?;
                        write_element(
                            &mut writer,
                            "meta",
                            &[("property", "dcterms:modified")],
                            &ts,
                        )?;
                        fixes.push(Fix::ModifiedTimestampAdded);
                    }
                    in_metadata = false;
                }

                writer.write_event(Event::End(e.clone().into_owned()))?;
            }

            Ok(Event::Eof) => break,

            Ok(ref e) => {
                if reading_identifier {
                    identifier_event_buf.push(e.clone().into_owned());
                } else if reading_contributor {
                    contributor_event_buf.push(e.clone().into_owned());
                } else if skip_depth == 0 {
                    writer.write_event(e.clone().into_owned())?;
                }
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

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn write_text(writer: &mut Writer<Cursor<Vec<u8>>>, text: &str) -> Result<(), std::io::Error> {
    writer.write_event(Event::Text(BytesText::new(text)))
}

fn write_element(
    writer: &mut Writer<Cursor<Vec<u8>>>,
    tag: &str,
    attrs: &[(&str, &str)],
    text: &str,
) -> Result<(), std::io::Error> {
    let mut elem = BytesStart::new(tag);
    for (k, v) in attrs {
        elem.push_attribute((*k, *v));
    }
    writer.write_event(Event::Start(elem))?;
    writer.write_event(Event::Text(BytesText::new(text)))?;
    writer.write_event(Event::End(BytesEnd::new(tag)))?;
    Ok(())
}

/// Check if a language code is valid (ISO 639-1/639-2, optionally with region subtag).
pub(crate) fn is_valid_language_code(s: &str) -> bool {
    is_valid_language(s)
}

fn is_valid_language(s: &str) -> bool {
    let s = s.trim();
    if s.is_empty() || s.eq_ignore_ascii_case("und") {
        return false;
    }
    let primary = s.split('-').next().unwrap_or("");
    matches!(primary.len(), 2 | 3) && primary.bytes().all(|b| b.is_ascii_alphabetic())
}

// ---------------------------------------------------------------------------
// Vendor xmlns detection
// ---------------------------------------------------------------------------

/// Known vendor namespace URIs to strip. Only strip namespaces we're sure
/// are tool-specific. Do NOT blanket-strip unknown namespaces — EPUB3 uses
/// custom vocabularies via the `prefix` attribute that are legitimate.
const VENDOR_XMLNS_URIS: &[&str] = &[
    "http://calibre.kovidgoyal.net/",
    "http://calibre-ebook.com/",
];

/// Returns true if an attribute is a known vendor xmlns declaration.
fn is_vendor_xmlns(key: &[u8], value: &[u8]) -> bool {
    let key_str = std::str::from_utf8(key).unwrap_or("");
    if !key_str.starts_with("xmlns:") && key_str != "xmlns" {
        return false;
    }
    let uri = std::str::from_utf8(value).unwrap_or("");
    VENDOR_XMLNS_URIS
        .iter()
        .any(|vendor_uri| uri.starts_with(vendor_uri))
}

/// Strip vendor entries from an EPUB3 `prefix` attribute value.
/// Format: `prefix: uri prefix2: uri2` — space-separated pairs.
/// e.g. `calibre: http://calibre.kovidgoyal.net/2009/metadata rendition: http://...`
fn strip_vendor_prefixes(prefix_attr: &str) -> String {
    // Parse prefix declarations: each is "name: URI" separated by whitespace
    let mut result = Vec::new();
    let mut iter = prefix_attr.split_whitespace().peekable();
    while let Some(token) = iter.next() {
        if token.ends_with(':') {
            // This is a prefix name, next token is the URI
            if let Some(uri) = iter.next() {
                let is_vendor = VENDOR_XMLNS_URIS.iter().any(|v| uri.starts_with(v));
                if !is_vendor {
                    result.push(format!("{token} {uri}"));
                }
            }
        }
    }
    result.join(" ")
}

// ---------------------------------------------------------------------------
// Vendor identifier detection
// ---------------------------------------------------------------------------

/// Vendor-specific identifier schemes to strip (attribute-based).
const VENDOR_SCHEMES: &[&str] = &[
    "AMAZON",
    "MOBI-ASIN",
    "GOOGLE",
    "GOODREADS",
    "BARNESNOBLE",
    "calibre",
];

/// Vendor-specific identifier content prefixes.
const VENDOR_CONTENT_PREFIXES: &[&str] = &["urn:amazon:asin:", "calibre:", "google:"];

/// Prefixes that indicate a standard (non-vendor) identifier — always preserve.
const PRESERVE_CONTENT_PREFIXES: &[&str] = &["urn:isbn:", "urn:uuid:", "urn:doi:", "doi:"];

fn is_vendor_identifier_by_attr(e: &BytesStart<'_>) -> bool {
    for attr in e.attributes().flatten() {
        if local_name(attr.key.as_ref()) == b"scheme" {
            let val = String::from_utf8_lossy(&attr.value);
            if VENDOR_SCHEMES
                .iter()
                .any(|s| s.eq_ignore_ascii_case(val.trim()))
            {
                return true;
            }
        }
    }
    false
}

fn is_vendor_identifier_by_content(text: &str) -> bool {
    let lower = text.to_lowercase();
    // Never strip if it matches a preserve prefix
    if PRESERVE_CONTENT_PREFIXES
        .iter()
        .any(|p| lower.starts_with(p))
    {
        return false;
    }
    // Strip if it matches a vendor prefix
    VENDOR_CONTENT_PREFIXES.iter().any(|p| lower.starts_with(p))
}

fn vendor_detail(e: &BytesStart<'_>) -> String {
    for attr in e.attributes().flatten() {
        if local_name(attr.key.as_ref()) == b"scheme" {
            return format!(
                "vendor identifier ({})",
                String::from_utf8_lossy(&attr.value)
            );
        }
    }
    "vendor identifier".into()
}

// ---------------------------------------------------------------------------
// Tool metadata detection
// ---------------------------------------------------------------------------

/// Tool-specific metadata prefixes to strip (case-insensitive).
const STRIP_META_PREFIXES: &[&str] = &[
    "calibre:timestamp",
    "calibre:title_sort",
    "calibre:author_link_map",
    "calibre:rating",
    "calibre:user_categories",
    "calibre:user_metadata:",
];

/// Meta names to preserve (even though they start with "calibre:").
const KEEP_META: &[&str] = &["calibre:series", "calibre:series_index"];

fn is_tool_metadata(e: &BytesStart<'_>) -> bool {
    // Check both EPUB2 `name` attribute and EPUB3 `property` attribute
    let mut name_val = None;
    let mut property_val = None;
    for attr in e.attributes().flatten() {
        match attr.key.as_ref() {
            b"name" => name_val = Some(String::from_utf8_lossy(&attr.value).into_owned()),
            b"property" => property_val = Some(String::from_utf8_lossy(&attr.value).into_owned()),
            _ => {}
        }
    }

    // Check name attribute (EPUB2: <meta name="calibre:timestamp" content="..."/>)
    if let Some(ref name) = name_val {
        if is_strippable_meta_name(name) {
            return true;
        }
    }

    // Check property attribute (EPUB3: <meta property="calibre:timestamp">...</meta>)
    if let Some(ref prop) = property_val {
        if is_strippable_meta_name(prop) {
            return true;
        }
    }

    false
}

fn is_strippable_meta_name(name: &str) -> bool {
    // Preserve calibre:series and calibre:series_index
    if KEEP_META.iter().any(|k| name.eq_ignore_ascii_case(k)) {
        return false;
    }

    let lower = name.to_lowercase();

    // Strip calibre:* and Sigil metadata (case-insensitive)
    if STRIP_META_PREFIXES
        .iter()
        .any(|p| lower.starts_with(&p.to_lowercase()))
    {
        return true;
    }

    // Sigil version
    if lower == "sigil version" {
        return true;
    }

    false
}

fn tool_meta_detail(e: &BytesStart<'_>) -> String {
    for attr in e.attributes().flatten() {
        if attr.key.as_ref() == b"name" {
            return String::from_utf8_lossy(&attr.value).into_owned();
        }
    }
    "tool metadata".into()
}

// ---------------------------------------------------------------------------
// Contributor detection
// ---------------------------------------------------------------------------

/// Known tool names that appear in dc:contributor with opf:role="bkp".
const TOOL_CONTRIBUTOR_NAMES: &[&str] = &[
    "calibre",
    "sigil",
    "indesign",
    "ibooks",
    "vellum",
    "pandoc",
    "kindlegen",
];

fn has_role_bkp(e: &BytesStart<'_>) -> bool {
    for attr in e.attributes().flatten() {
        if local_name(attr.key.as_ref()) == b"role" {
            let val = String::from_utf8_lossy(&attr.value);
            if val.trim().eq_ignore_ascii_case("bkp") {
                return true;
            }
        }
    }
    false
}

/// Check if a contributor element has an `id` that was marked as bkp via EPUB3 refines.
fn has_bkp_via_refines(e: &BytesStart<'_>, bkp_ids: &[String]) -> bool {
    for attr in e.attributes().flatten() {
        if attr.key.as_ref() == b"id" {
            let id = String::from_utf8_lossy(&attr.value);
            return bkp_ids.iter().any(|bkp| bkp == id.as_ref());
        }
    }
    false
}

fn is_tool_contributor(text: &str) -> bool {
    let lower = text.to_lowercase();
    TOOL_CONTRIBUTOR_NAMES
        .iter()
        .any(|name| lower.contains(name))
}

// ---------------------------------------------------------------------------
// Timestamp
// ---------------------------------------------------------------------------

#[allow(
    clippy::many_single_char_names,
    clippy::cast_possible_wrap,
    clippy::cast_sign_loss
)]
fn utc_timestamp_now() -> String {
    use std::time::{SystemTime, UNIX_EPOCH};

    let secs = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();

    let days = secs / 86400;
    let day_secs = secs % 86400;
    let h = day_secs / 3600;
    let m = (day_secs % 3600) / 60;
    let s = day_secs % 60;

    // Howard Hinnant's civil_from_days algorithm
    let z = days as i64 + 719_468;
    let era = (if z >= 0 { z } else { z - 146_096 }) / 146_097;
    let doe = (z - era * 146_097) as u64;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146_096) / 365;
    let y = yoe as i64 + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let mo = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = if mo <= 2 { y + 1 } else { y };

    format!("{y:04}-{mo:02}-{d:02}T{h:02}:{m:02}:{s:02}Z")
}

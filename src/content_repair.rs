use quick_xml::events::Event;
use quick_xml::reader::Reader;
use quick_xml::writer::Writer;
use std::io::Cursor;

use crate::{xml_local_name as local_name, Fix};

/// Repairs XHTML content: adds XML declaration, removes `<img>` without `src`,
/// removes `<script>` tags.
///
/// Strategy:
/// 1. XML declaration fix is always byte-level (works on any file).
/// 2. Attempt a full quick-xml parse. If it succeeds, apply all XML-level
///    fixes (script/img removal) against the parsed representation.
///    If parsing fails, skip XML-level fixes — byte-level still applied.
pub(crate) fn repair_content(content: &[u8], file_name: &str) -> (Vec<u8>, Vec<Fix>, Vec<String>) {
    let mut fixes = Vec::new();
    let mut warnings = Vec::new();
    let mut result = content.to_vec();

    // Step 1: XML declaration (byte-level — always works, even on malformed XHTML)
    if needs_xml_declaration(&result) {
        result = add_xml_declaration(&result);
        fixes.push(Fix::XmlDeclarationAdded {
            file: file_name.to_owned(),
        });
    }

    // Step 2: Attempt structured XML parse for all XML-level fixes.
    // If parsing fails, we keep the byte-level result and warn.
    match try_xml_repairs(&result, file_name) {
        Some((repaired, xml_fixes)) => {
            result = repaired;
            fixes.extend(xml_fixes);
        }
        None if std::str::from_utf8(&result).is_err()
            || result
                .windows(7)
                .any(|w| w.eq_ignore_ascii_case(b"<script"))
            || result.windows(4).any(|w| w.eq_ignore_ascii_case(b"<img")) =>
        {
            warnings.push(format!(
                "{file_name}: XML parsing failed; script/img fixes skipped"
            ));
        }
        None => {} // No issues detected, no warning needed
    }

    (result, fixes, warnings)
}

fn needs_xml_declaration(content: &[u8]) -> bool {
    let content = skip_bom(content);
    !starts_with_xml_decl(content)
}

fn skip_bom(content: &[u8]) -> &[u8] {
    if content.starts_with(&[0xEF, 0xBB, 0xBF]) {
        &content[3..]
    } else {
        content
    }
}

fn starts_with_xml_decl(content: &[u8]) -> bool {
    let trimmed = content
        .iter()
        .copied()
        .skip_while(u8::is_ascii_whitespace)
        .take(5)
        .collect::<Vec<u8>>();
    trimmed.starts_with(b"<?xml") || trimmed.starts_with(b"<?XML")
}

fn add_xml_declaration(content: &[u8]) -> Vec<u8> {
    let decl = b"<?xml version=\"1.0\" encoding=\"utf-8\"?>\n";
    if content.starts_with(&[0xEF, 0xBB, 0xBF]) {
        // BOM present — place declaration after BOM
        let mut result = Vec::with_capacity(content.len() + decl.len());
        result.extend_from_slice(&content[..3]);
        result.extend_from_slice(decl);
        result.extend_from_slice(&content[3..]);
        result
    } else {
        let mut result = Vec::with_capacity(content.len() + decl.len());
        result.extend_from_slice(decl);
        result.extend_from_slice(content);
        result
    }
}

/// Attempt a full XML parse of the XHTML and apply all XML-level fixes.
/// Returns `None` if parsing fails or no fixes are needed — caller keeps
/// the original (byte-level-only) content.
#[allow(clippy::too_many_lines)]
fn try_xml_repairs(content: &[u8], file_name: &str) -> Option<(Vec<u8>, Vec<Fix>)> {
    let content_str = std::str::from_utf8(content).ok()?;
    let mut reader = Reader::from_str(content_str);
    reader.config_mut().trim_text(false);
    // Keep check_end_names = true (default). Unbalanced tags trigger a parse
    // error, which our Err(_) => return None fallback handles safely by
    // preserving the original content byte-for-byte.

    let mut writer = Writer::new(Cursor::new(Vec::new()));
    let mut fixes = Vec::new();
    let mut in_script = false;
    let mut script_depth: usize = 0;
    let mut reported_script = false;
    let mut reported_img = false;
    let mut skip_img_depth: usize = 0;

    loop {
        match reader.read_event() {
            Ok(Event::Start(ref e)) => {
                // Check containment FIRST — nested elements inside removed
                // blocks must increment depth, not reset it.
                if in_script {
                    script_depth += 1;
                    continue;
                }
                if skip_img_depth > 0 {
                    skip_img_depth += 1;
                    continue;
                }

                let qname = e.name();
                let local = local_name(qname.as_ref());

                if local.eq_ignore_ascii_case(b"script") {
                    in_script = true;
                    script_depth = 1;
                    if !reported_script {
                        fixes.push(Fix::ScriptRemoved {
                            file: file_name.to_owned(),
                        });
                        reported_script = true;
                    }
                    continue;
                }

                if local.eq_ignore_ascii_case(b"img") && !has_src_attr(e) {
                    if !reported_img {
                        fixes.push(Fix::StrayImgRemoved {
                            file: file_name.to_owned(),
                        });
                        reported_img = true;
                    }
                    skip_img_depth = 1;
                    continue;
                }

                writer
                    .write_event(Event::Start(e.clone().into_owned()))
                    .ok()?;
            }
            Ok(Event::End(ref e)) => {
                if in_script {
                    script_depth -= 1;
                    if script_depth == 0 {
                        in_script = false;
                    }
                    continue;
                }
                if skip_img_depth > 0 {
                    skip_img_depth -= 1;
                    continue;
                }
                writer
                    .write_event(Event::End(e.clone().into_owned()))
                    .ok()?;
            }
            Ok(Event::Empty(ref e)) => {
                if in_script || skip_img_depth > 0 {
                    continue;
                }
                let qname = e.name();
                let local = local_name(qname.as_ref());

                if local.eq_ignore_ascii_case(b"script") {
                    if !reported_script {
                        fixes.push(Fix::ScriptRemoved {
                            file: file_name.to_owned(),
                        });
                        reported_script = true;
                    }
                    continue;
                }

                if local.eq_ignore_ascii_case(b"img") && !has_src_attr(e) {
                    if !reported_img {
                        fixes.push(Fix::StrayImgRemoved {
                            file: file_name.to_owned(),
                        });
                        reported_img = true;
                    }
                    continue;
                }

                writer
                    .write_event(Event::Empty(e.clone().into_owned()))
                    .ok()?;
            }
            Ok(Event::Text(ref e)) => {
                if in_script || skip_img_depth > 0 {
                    continue;
                }
                writer
                    .write_event(Event::Text(e.clone().into_owned()))
                    .ok()?;
            }
            Ok(Event::Eof) => break,
            Ok(ref e) => {
                if !in_script && skip_img_depth == 0 {
                    writer.write_event(e.clone().into_owned()).ok()?;
                }
            }
            Err(_) => return None, // Parse failed — fall back to byte-level only
        }
    }

    if fixes.is_empty() {
        return None;
    }

    let output = writer.into_inner().into_inner();
    Some((output, fixes))
}

fn has_src_attr(e: &quick_xml::events::BytesStart<'_>) -> bool {
    e.attributes()
        .flatten()
        .any(|a| a.key.as_ref().eq_ignore_ascii_case(b"src") && !a.value.is_empty())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detects_missing_xml_declaration() {
        assert!(needs_xml_declaration(b"<html></html>"));
        assert!(needs_xml_declaration(b"\xEF\xBB\xBF<html></html>"));
    }

    #[test]
    fn detects_existing_xml_declaration() {
        assert!(!needs_xml_declaration(
            b"<?xml version=\"1.0\" encoding=\"utf-8\"?>\n<html></html>"
        ));
    }

    #[test]
    fn adds_xml_declaration() {
        let input = b"<html></html>";
        let result = add_xml_declaration(input);
        assert!(result.starts_with(b"<?xml"));
        assert!(result.ends_with(b"<html></html>"));
    }

    #[test]
    fn adds_xml_declaration_after_bom() {
        let input = b"\xEF\xBB\xBF<html></html>";
        let result = add_xml_declaration(input);
        assert!(result.starts_with(&[0xEF, 0xBB, 0xBF]));
        assert!(result[3..].starts_with(b"<?xml"));
    }

    #[test]
    fn parse_failure_returns_none() {
        // Malformed XHTML that quick-xml can't parse
        let bad = b"<?xml version=\"1.0\"?><html><body><p>unclosed";
        // Should return None (no XML-level fixes), not panic
        assert!(try_xml_repairs(bad, "test.xhtml").is_none());
    }

    #[test]
    fn clean_xhtml_returns_none() {
        // Valid XHTML with no issues — should return None (no fixes needed)
        let clean = b"<?xml version=\"1.0\"?><html><body><p>Hello</p></body></html>";
        assert!(try_xml_repairs(clean, "test.xhtml").is_none());
    }
}

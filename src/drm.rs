use std::io::{Read, Seek};

use quick_xml::events::Event;
use quick_xml::reader::Reader;
use zip::ZipArchive;

use crate::{xml_local_name as local_name, RepubError};

/// Checks for DRM markers in an EPUB archive.
///
/// # Errors
///
/// Returns [`RepubError::DrmDetected`] if DRM is found.
/// Returns [`RepubError::Io`] on read failures.
pub(crate) fn check_drm<R: Read + Seek>(archive: &mut ZipArchive<R>) -> Result<(), RepubError> {
    // rights.xml is a definitive DRM marker
    if archive.by_name("META-INF/rights.xml").is_ok() {
        return Err(RepubError::DrmDetected {
            message: "META-INF/rights.xml present".into(),
        });
    }

    // encryption.xml with non-font entries indicates DRM
    if let Ok(mut entry) = archive.by_name("META-INF/encryption.xml") {
        let mut contents = String::new();
        entry.read_to_string(&mut contents)?;

        if has_non_font_encryption(&contents) {
            return Err(RepubError::DrmDetected {
                message: "META-INF/encryption.xml contains non-font encryption".into(),
            });
        }
    }

    Ok(())
}

/// Font obfuscation algorithm URIs — these are benign, not DRM.
const FONT_OBFUSCATION_ALGORITHMS: &[&str] = &[
    "http://www.idpf.org/2008/embedding",
    "http://ns.adobe.com/pdf/enc#RC",
];

/// Returns `true` if the encryption.xml contains encryption entries that are
/// not font obfuscation.
fn has_non_font_encryption(xml: &str) -> bool {
    // Quick check: no EncryptedData means no encryption at all
    if !xml.contains("EncryptedData") {
        return false;
    }

    // Known DRM namespace — definitive
    if xml.contains("http://ns.adobe.com/adept") {
        return true;
    }

    // Parse structurally: find EncryptionMethod elements and check Algorithm attribute
    let mut reader = Reader::from_str(xml);

    loop {
        match reader.read_event() {
            Ok(Event::Start(ref e) | Event::Empty(ref e)) => {
                let name = e.name();
                let local = local_name(name.as_ref());

                if local == b"EncryptionMethod" {
                    for attr in e.attributes().flatten() {
                        if local_name(attr.key.as_ref()) == b"Algorithm" {
                            let uri = String::from_utf8_lossy(&attr.value);
                            let is_font = FONT_OBFUSCATION_ALGORITHMS
                                .iter()
                                .any(|font_uri| uri.as_ref() == *font_uri);
                            if !is_font {
                                return true;
                            }
                        }
                    }
                }
            }
            Ok(Event::Eof) => break,
            Err(_) => {
                // If we can't parse encryption.xml, fail closed — treat as DRM
                return true;
            }
            _ => {}
        }
    }

    false
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn no_encryption_xml_is_fine() {
        let xml = "";
        assert!(!has_non_font_encryption(xml));
    }

    #[test]
    fn font_obfuscation_only_is_fine() {
        let xml = r#"
        <encryption>
          <EncryptedData>
            <EncryptionMethod Algorithm="http://www.idpf.org/2008/embedding"/>
          </EncryptedData>
        </encryption>
        "#;
        assert!(!has_non_font_encryption(xml));
    }

    #[test]
    fn adobe_adept_is_drm() {
        let xml = r#"
        <encryption xmlns="http://ns.adobe.com/adept">
          <EncryptedData>
            <EncryptionMethod Algorithm="http://www.w3.org/2001/04/xmlenc#aes128-cbc"/>
          </EncryptedData>
        </encryption>
        "#;
        assert!(has_non_font_encryption(xml));
    }

    #[test]
    fn non_font_encryption_is_drm() {
        let xml = r#"
        <encryption>
          <EncryptedData>
            <EncryptionMethod Algorithm="http://www.w3.org/2001/04/xmlenc#aes256-cbc"/>
          </EncryptedData>
        </encryption>
        "#;
        assert!(has_non_font_encryption(xml));
    }

    #[test]
    fn multiline_encryption_method_detected() {
        let xml = r#"
        <encryption>
          <EncryptedData>
            <EncryptionMethod
                Algorithm="http://www.w3.org/2001/04/xmlenc#aes256-cbc"/>
          </EncryptedData>
        </encryption>
        "#;
        assert!(has_non_font_encryption(xml));
    }

    #[test]
    fn multiline_font_obfuscation_allowed() {
        let xml = r#"
        <encryption>
          <EncryptedData>
            <EncryptionMethod
                Algorithm="http://www.idpf.org/2008/embedding"/>
          </EncryptedData>
        </encryption>
        "#;
        assert!(!has_non_font_encryption(xml));
    }

    #[test]
    fn mixed_font_and_drm_detected() {
        let xml = r#"
        <encryption>
          <EncryptedData>
            <EncryptionMethod Algorithm="http://www.idpf.org/2008/embedding"/>
          </EncryptedData>
          <EncryptedData>
            <EncryptionMethod Algorithm="http://www.w3.org/2001/04/xmlenc#aes128-cbc"/>
          </EncryptedData>
        </encryption>
        "#;
        assert!(has_non_font_encryption(xml));
    }

    #[test]
    fn namespaced_encryption_method_detected() {
        let xml = r#"
        <enc:encryption xmlns:enc="http://www.w3.org/2001/04/xmlenc#">
          <enc:EncryptedData>
            <enc:EncryptionMethod Algorithm="http://www.w3.org/2001/04/xmlenc#aes256-cbc"/>
          </enc:EncryptedData>
        </enc:encryption>
        "#;
        assert!(has_non_font_encryption(xml));
    }
}

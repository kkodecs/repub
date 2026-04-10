//! Stress tests: edge cases, malformed input, idempotency, error paths.

use repub::{Fix, Repub, RepubError};
use std::io::{Cursor, Read, Write};
use zip::read::ZipArchive;

#[path = "helpers/mod.rs"]
mod helpers;

// ── Idempotency ─────────────────────────────────────────────────────────

#[test]
fn fix_is_idempotent() {
    // fix(fix(epub)) == fix(epub)
    let epub = helpers::build_epub_compressed_mimetype();
    let once = Repub::new().fix_bytes(&epub).unwrap();
    let twice = Repub::new().fix_bytes(&once).unwrap();
    // Content should be byte-identical (timestamps may differ, so compare structure)
    let a1 = ZipArchive::new(Cursor::new(&once)).unwrap();
    let a2 = ZipArchive::new(Cursor::new(&twice)).unwrap();
    assert_eq!(a1.len(), a2.len());
    for i in 0..a1.len() {
        let e1 = a1.file_names().nth(i).unwrap().to_owned();
        let e2 = a2.file_names().nth(i).unwrap().to_owned();
        assert_eq!(e1, e2, "entry names should match at index {i}");
    }
}

#[test]
fn fix_idempotent_no_extra_fixes_on_second_pass() {
    let epub = helpers::build_epub_missing_language();
    let once = Repub::new().fix_bytes(&epub).unwrap();
    // Second pass: read the fixed EPUB, get a report
    let (_twice, report) = fix_with_report(&once);
    // The only fix on second pass should be MimetypeFixed (ZIP rewrite always happens)
    // or nothing at all if mimetype was already correct
    let non_mimetype_fixes: Vec<_> = report
        .fixes
        .iter()
        .filter(|f| !matches!(f, Fix::MimetypeFixed))
        .collect();
    assert!(
        non_mimetype_fixes.is_empty(),
        "second pass should not produce non-mimetype fixes, got: {non_mimetype_fixes:?}"
    );
}

#[test]
fn fix_valid_epub_reports_no_modifications() {
    let epub = helpers::build_epub_valid();
    let (_, report) = fix_with_report(&epub);
    // A valid EPUB should not need any fixes (or only mimetype if ZIP rewrite differs)
    let significant_fixes: Vec<_> = report
        .fixes
        .iter()
        .filter(|f| !matches!(f, Fix::MimetypeFixed))
        .collect();
    assert!(
        significant_fixes.is_empty(),
        "valid EPUB should need no fixes, got: {significant_fixes:?}"
    );
}

// ── Malformed input ─────────────────────────────────────────────────────

#[test]
fn malformed_xhtml_still_gets_xml_decl() {
    // XHTML that quick-xml can't parse — XML declaration should still be added
    let bad_xhtml = b"<html><body><p>unclosed paragraph<p>another</body></html>";
    let epub = helpers::EpubBuilder::new()
        .add("mimetype", b"application/epub+zip", true)
        .add(
            "META-INF/container.xml",
            helpers::container_xml().as_bytes(),
            false,
        )
        .add("OEBPS/content.opf", helpers::valid_opf().as_bytes(), false)
        .add("OEBPS/chapter01.xhtml", bad_xhtml, false)
        .build();

    let fixed = Repub::new().fix_bytes(&epub).unwrap();
    let mut archive = ZipArchive::new(Cursor::new(&fixed)).unwrap();
    let mut entry = archive.by_name("OEBPS/chapter01.xhtml").unwrap();
    let mut content = String::new();
    entry.read_to_string(&mut content).unwrap();
    assert!(
        content.contains("<?xml"),
        "XML declaration should be added even to malformed XHTML"
    );
}

#[test]
fn not_a_zip_returns_error() {
    let garbage = b"this is not a zip file";
    let result = Repub::new().fix_bytes(garbage);
    assert!(result.is_err());
}

#[test]
fn zip_without_container_xml_returns_error() {
    let epub = helpers::EpubBuilder::new()
        .add("mimetype", b"application/epub+zip", true)
        .add("some_file.txt", b"hello", false)
        .build();
    let result = Repub::new().fix_bytes(&epub);
    assert!(result.is_err());
    match result.unwrap_err() {
        RepubError::InvalidEpub { message } => {
            assert!(
                message.contains("container.xml"),
                "error should mention container.xml"
            );
        }
        other => panic!("expected InvalidEpub, got: {other}"),
    }
}

#[test]
fn empty_opf_metadata_gets_all_required_fields() {
    let opf = r#"<?xml version="1.0" encoding="UTF-8"?>
<package xmlns="http://www.idpf.org/2007/opf" version="3.0">
  <metadata xmlns:dc="http://purl.org/dc/elements/1.1/">
    <dc:title>Minimal</dc:title>
  </metadata>
  <manifest>
    <item id="ch1" href="chapter01.xhtml" media-type="application/xhtml+xml"/>
  </manifest>
  <spine><itemref idref="ch1"/></spine>
</package>"#;

    let epub = helpers::EpubBuilder::new()
        .add("mimetype", b"application/epub+zip", true)
        .add(
            "META-INF/container.xml",
            helpers::container_xml().as_bytes(),
            false,
        )
        .add("OEBPS/content.opf", opf.as_bytes(), false)
        .add(
            "OEBPS/chapter01.xhtml",
            helpers::chapter_xhtml().as_bytes(),
            false,
        )
        .build();

    let fixed = Repub::new().fix_bytes(&epub).unwrap();
    let mut archive = ZipArchive::new(Cursor::new(&fixed)).unwrap();
    let mut entry = archive.by_name("OEBPS/content.opf").unwrap();
    let mut content = String::new();
    entry.read_to_string(&mut content).unwrap();

    assert!(content.contains("dc:language"), "should have dc:language");
    assert!(
        content.contains("dc:identifier"),
        "should have dc:identifier"
    );
    assert!(
        content.contains("unique-identifier"),
        "should have unique-identifier on package"
    );
    assert!(
        content.contains("dcterms:modified"),
        "EPUB3 should have dcterms:modified"
    );
}

// ── BOM handling ────────────────────────────────────────────────────────

#[test]
fn bom_xhtml_gets_xml_decl_after_bom() {
    let mut bom_xhtml = vec![0xEF, 0xBB, 0xBF]; // UTF-8 BOM
    bom_xhtml.extend_from_slice(b"<html><body><p>Hello</p></body></html>");

    let epub = helpers::EpubBuilder::new()
        .add("mimetype", b"application/epub+zip", true)
        .add(
            "META-INF/container.xml",
            helpers::container_xml().as_bytes(),
            false,
        )
        .add("OEBPS/content.opf", helpers::valid_opf().as_bytes(), false)
        .add("OEBPS/chapter01.xhtml", &bom_xhtml, false)
        .build();

    let fixed = Repub::new().fix_bytes(&epub).unwrap();
    let mut archive = ZipArchive::new(Cursor::new(&fixed)).unwrap();
    let mut entry = archive.by_name("OEBPS/chapter01.xhtml").unwrap();
    let mut content = Vec::new();
    entry.read_to_end(&mut content).unwrap();

    assert_eq!(&content[..3], &[0xEF, 0xBB, 0xBF], "BOM should be first");
    assert!(
        content[3..].starts_with(b"<?xml"),
        "XML declaration should follow BOM"
    );
}

// ── Multiple content files ──────────────────────────────────────────────

#[test]
fn multiple_xhtml_files_each_get_xml_decl() {
    let no_decl = b"<html><body><p>No decl</p></body></html>";

    let opf = r#"<?xml version="1.0" encoding="UTF-8"?>
<package xmlns="http://www.idpf.org/2007/opf" unique-identifier="BookId" version="3.0">
  <metadata xmlns:dc="http://purl.org/dc/elements/1.1/">
    <dc:title>Multi</dc:title>
    <dc:language>en</dc:language>
    <dc:identifier id="BookId">urn:uuid:aaaaaaaa-bbbb-cccc-dddd-eeeeeeeeeeee</dc:identifier>
    <meta property="dcterms:modified">2026-01-01T00:00:00Z</meta>
  </metadata>
  <manifest>
    <item id="ch1" href="ch01.xhtml" media-type="application/xhtml+xml"/>
    <item id="ch2" href="ch02.xhtml" media-type="application/xhtml+xml"/>
    <item id="ch3" href="ch03.xhtml" media-type="application/xhtml+xml"/>
  </manifest>
  <spine>
    <itemref idref="ch1"/>
    <itemref idref="ch2"/>
    <itemref idref="ch3"/>
  </spine>
</package>"#;

    let epub = helpers::EpubBuilder::new()
        .add("mimetype", b"application/epub+zip", true)
        .add(
            "META-INF/container.xml",
            helpers::container_xml().as_bytes(),
            false,
        )
        .add("OEBPS/content.opf", opf.as_bytes(), false)
        .add("OEBPS/ch01.xhtml", no_decl, false)
        .add("OEBPS/ch02.xhtml", no_decl, false)
        .add("OEBPS/ch03.xhtml", no_decl, false)
        .build();

    let fixed = Repub::new().fix_bytes(&epub).unwrap();
    let mut archive = ZipArchive::new(Cursor::new(&fixed)).unwrap();

    for name in ["OEBPS/ch01.xhtml", "OEBPS/ch02.xhtml", "OEBPS/ch03.xhtml"] {
        let mut entry = archive.by_name(name).unwrap();
        let mut content = String::new();
        entry.read_to_string(&mut content).unwrap();
        assert!(
            content.contains("<?xml"),
            "{name} should have XML declaration"
        );
    }
}

// ── NCX edge cases ──────────────────────────────────────────────────────

#[test]
fn ncx_non_body_fragment_preserved() {
    // Fragment that targets an internal anchor, NOT a body id — should be preserved
    let xhtml = r#"<?xml version="1.0" encoding="utf-8"?>
<html xmlns="http://www.w3.org/1999/xhtml">
<head><title>Ch1</title></head>
<body>
<h1 id="section1">Section 1</h1>
<p>Content</p>
</body>
</html>"#;

    let ncx = r#"<?xml version="1.0" encoding="UTF-8"?>
<ncx xmlns="http://www.daisy.org/z3986/2005/ncx/" version="2005-1">
  <navMap>
    <navPoint id="np1">
      <navLabel><text>Section 1</text></navLabel>
      <content src="chapter01.xhtml#section1"/>
    </navPoint>
  </navMap>
</ncx>"#;

    let opf = r#"<?xml version="1.0" encoding="UTF-8"?>
<package xmlns="http://www.idpf.org/2007/opf" unique-identifier="BookId" version="2.0">
  <metadata xmlns:dc="http://purl.org/dc/elements/1.1/">
    <dc:title>Test</dc:title>
    <dc:language>en</dc:language>
    <dc:identifier id="BookId">urn:uuid:12345678-1234-1234-1234-123456789abc</dc:identifier>
  </metadata>
  <manifest>
    <item id="ch1" href="chapter01.xhtml" media-type="application/xhtml+xml"/>
    <item id="ncx" href="toc.ncx" media-type="application/x-dtbncx+xml"/>
  </manifest>
  <spine toc="ncx"><itemref idref="ch1"/></spine>
</package>"#;

    let epub = helpers::EpubBuilder::new()
        .add("mimetype", b"application/epub+zip", true)
        .add(
            "META-INF/container.xml",
            helpers::container_xml().as_bytes(),
            false,
        )
        .add("OEBPS/content.opf", opf.as_bytes(), false)
        .add("OEBPS/chapter01.xhtml", xhtml.as_bytes(), false)
        .add("OEBPS/toc.ncx", ncx.as_bytes(), false)
        .build();

    let fixed = Repub::new().fix_bytes(&epub).unwrap();
    let mut archive = ZipArchive::new(Cursor::new(&fixed)).unwrap();
    let mut entry = archive.by_name("OEBPS/toc.ncx").unwrap();
    let mut content = String::new();
    entry.read_to_string(&mut content).unwrap();

    assert!(
        content.contains("#section1"),
        "non-body fragment should be preserved"
    );
}

// ── DRM edge cases ──────────────────────────────────────────────────────

#[test]
fn encryption_xml_with_only_fonts_is_allowed() {
    let encryption_xml = r#"<?xml version="1.0" encoding="UTF-8"?>
<encryption xmlns="urn:oasis:names:tc:opendocument:xmlns:container">
  <EncryptedData>
    <EncryptionMethod Algorithm="http://www.idpf.org/2008/embedding"/>
    <CipherData><CipherReference URI="fonts/myfont.otf"/></CipherData>
  </EncryptedData>
</encryption>"#;

    let epub = helpers::EpubBuilder::new()
        .add("mimetype", b"application/epub+zip", true)
        .add(
            "META-INF/container.xml",
            helpers::container_xml().as_bytes(),
            false,
        )
        .add("META-INF/encryption.xml", encryption_xml.as_bytes(), false)
        .add("OEBPS/content.opf", helpers::valid_opf().as_bytes(), false)
        .add(
            "OEBPS/chapter01.xhtml",
            helpers::chapter_xhtml().as_bytes(),
            false,
        )
        .build();

    // Should succeed — font obfuscation is not DRM
    let result = Repub::new().fix_bytes(&epub);
    assert!(result.is_ok(), "font-only encryption should be allowed");
}

// ── All fixes at once ───────────────────────────────────────────────────

#[test]
fn epub_with_all_issues_gets_all_fixes() {
    let opf = r#"<?xml version="1.0" encoding="UTF-8"?>
<package xmlns="http://www.idpf.org/2007/opf" unique-identifier="BadRef" version="3.0">
  <metadata xmlns:dc="http://purl.org/dc/elements/1.1/" xmlns:opf="http://www.idpf.org/2007/opf">
    <dc:title>Broken Book</dc:title>
    <dc:identifier opf:scheme="AMAZON">B00BROKEN</dc:identifier>
    <meta name="calibre:timestamp" content="2020-01-01T00:00:00+00:00"/>
  </metadata>
  <manifest>
    <item id="ch1" href="chapter01.xhtml" media-type="application/xhtml+xml"/>
  </manifest>
  <spine><itemref idref="ch1"/></spine>
</package>"#;

    let xhtml = b"<html><body><img /><script>alert('x');</script><p>Text</p></body></html>";

    let epub = helpers::EpubBuilder::new()
        .add("mimetype", b"application/epub+zip", false) // compressed!
        .add(
            "META-INF/container.xml",
            helpers::container_xml().as_bytes(),
            false,
        )
        .add("OEBPS/content.opf", opf.as_bytes(), false)
        .add("OEBPS/chapter01.xhtml", xhtml, false)
        .build();

    let (fixed_bytes, report) = fix_with_report(&epub);
    assert!(report.modified);

    // Check that we got a substantial number of fixes
    assert!(
        report.fixes.len() >= 5,
        "expected at least 5 fixes, got {}: {:?}",
        report.fixes.len(),
        report.fixes
    );

    // Verify the output is a valid EPUB structure
    let archive = ZipArchive::new(Cursor::new(&fixed_bytes)).unwrap();
    assert!(archive.len() >= 3);
}

// ── Language edge cases ─────────────────────────────────────────────────

#[test]
fn custom_default_language_is_used() {
    let epub = helpers::build_epub_missing_language();
    let fixed = Repub::new()
        .default_language("ja")
        .fix_bytes(&epub)
        .unwrap();

    let mut archive = ZipArchive::new(Cursor::new(&fixed)).unwrap();
    let mut entry = archive.by_name("OEBPS/content.opf").unwrap();
    let mut content = String::new();
    entry.read_to_string(&mut content).unwrap();
    assert!(content.contains(">ja<"), "should use custom language 'ja'");
}

#[test]
fn empty_language_element_gets_replaced() {
    let opf = r#"<?xml version="1.0" encoding="UTF-8"?>
<package xmlns="http://www.idpf.org/2007/opf" unique-identifier="BookId" version="3.0">
  <metadata xmlns:dc="http://purl.org/dc/elements/1.1/">
    <dc:title>Test</dc:title>
    <dc:language></dc:language>
    <dc:identifier id="BookId">urn:uuid:12345678-1234-1234-1234-123456789abc</dc:identifier>
    <meta property="dcterms:modified">2026-01-01T00:00:00Z</meta>
  </metadata>
  <manifest>
    <item id="ch1" href="chapter01.xhtml" media-type="application/xhtml+xml"/>
  </manifest>
  <spine><itemref idref="ch1"/></spine>
</package>"#;

    let epub = helpers::EpubBuilder::new()
        .add("mimetype", b"application/epub+zip", true)
        .add(
            "META-INF/container.xml",
            helpers::container_xml().as_bytes(),
            false,
        )
        .add("OEBPS/content.opf", opf.as_bytes(), false)
        .add(
            "OEBPS/chapter01.xhtml",
            helpers::chapter_xhtml().as_bytes(),
            false,
        )
        .build();

    let fixed = Repub::new().fix_bytes(&epub).unwrap();
    let mut archive = ZipArchive::new(Cursor::new(&fixed)).unwrap();
    let mut entry = archive.by_name("OEBPS/content.opf").unwrap();
    let mut content = String::new();
    entry.read_to_string(&mut content).unwrap();
    assert!(
        content.contains(">en<"),
        "empty language should be replaced with default"
    );
}

// ── Proprietary edge cases ──────────────────────────────────────────────

#[test]
fn multiple_vendor_identifiers_all_removed() {
    let opf = r#"<?xml version="1.0" encoding="UTF-8"?>
<package xmlns="http://www.idpf.org/2007/opf" unique-identifier="BookId" version="3.0">
  <metadata xmlns:dc="http://purl.org/dc/elements/1.1/" xmlns:opf="http://www.idpf.org/2007/opf">
    <dc:title>Test</dc:title>
    <dc:language>en</dc:language>
    <dc:identifier id="BookId">urn:uuid:12345678-1234-1234-1234-123456789abc</dc:identifier>
    <dc:identifier opf:scheme="AMAZON">B00AAA</dc:identifier>
    <dc:identifier opf:scheme="GOOGLE">GOOGLE123</dc:identifier>
    <dc:identifier opf:scheme="GOODREADS">12345</dc:identifier>
    <dc:identifier opf:scheme="BARNESNOBLE">BN123</dc:identifier>
    <meta property="dcterms:modified">2026-01-01T00:00:00Z</meta>
  </metadata>
  <manifest>
    <item id="ch1" href="chapter01.xhtml" media-type="application/xhtml+xml"/>
  </manifest>
  <spine><itemref idref="ch1"/></spine>
</package>"#;

    let epub = helpers::EpubBuilder::new()
        .add("mimetype", b"application/epub+zip", true)
        .add(
            "META-INF/container.xml",
            helpers::container_xml().as_bytes(),
            false,
        )
        .add("OEBPS/content.opf", opf.as_bytes(), false)
        .add(
            "OEBPS/chapter01.xhtml",
            helpers::chapter_xhtml().as_bytes(),
            false,
        )
        .build();

    let fixed = Repub::new().fix_bytes(&epub).unwrap();
    let mut archive = ZipArchive::new(Cursor::new(&fixed)).unwrap();
    let mut entry = archive.by_name("OEBPS/content.opf").unwrap();
    let mut content = String::new();
    entry.read_to_string(&mut content).unwrap();

    assert!(!content.contains("AMAZON"));
    assert!(!content.contains("GOOGLE"));
    assert!(!content.contains("GOODREADS"));
    assert!(!content.contains("BARNESNOBLE"));
    assert!(content.contains("urn:uuid:12345678"), "UUID should survive");
}

// ── Regression: Gemini CRITICAL — void <img> must not eat content ────────

#[test]
fn void_img_without_src_does_not_eat_following_content() {
    // A non-self-closing <img> (Start event, not Empty) without src.
    // skip_to_end_tag used to consume everything after it looking for </img>.
    let xhtml = r#"<?xml version="1.0" encoding="utf-8"?>
<html xmlns="http://www.w3.org/1999/xhtml">
<head><title>Test</title></head>
<body>
<p>Before image.</p>
<img alt="no src"/>
<p>After image — this must survive.</p>
<img></img>
<p>This must also survive.</p>
</body>
</html>"#;

    let epub = helpers::EpubBuilder::new()
        .add("mimetype", b"application/epub+zip", true)
        .add(
            "META-INF/container.xml",
            helpers::container_xml().as_bytes(),
            false,
        )
        .add("OEBPS/content.opf", helpers::valid_opf().as_bytes(), false)
        .add("OEBPS/chapter01.xhtml", xhtml.as_bytes(), false)
        .build();

    let fixed = Repub::new().fix_bytes(&epub).unwrap();
    let mut archive = ZipArchive::new(Cursor::new(&fixed)).unwrap();
    let mut entry = archive.by_name("OEBPS/chapter01.xhtml").unwrap();
    let mut content = String::new();
    entry.read_to_string(&mut content).unwrap();

    assert!(
        content.contains("After image"),
        "content after <img/> must survive, got: {content}"
    );
    assert!(
        content.contains("This must also survive"),
        "content after <img></img> must survive, got: {content}"
    );
    assert!(
        content.contains("Before image"),
        "content before img must survive"
    );
}

// ── Helper ──────────────────────────────────────────────────────────────

fn fix_with_report(input: &[u8]) -> (Vec<u8>, repub::RepubReport) {
    // We need both bytes and report. Use check for report, fix_bytes for bytes.
    // Since there's no combined API, we run the repair twice (cheap for test EPUBs).
    use tempfile::NamedTempFile;

    let mut input_file = NamedTempFile::new().expect("create temp input");
    input_file.write_all(input).expect("write temp input");
    let output_file = NamedTempFile::new().expect("create temp output");

    let report = Repub::new()
        .fix(input_file.path(), output_file.path())
        .expect("fix epub");
    let fixed = std::fs::read(output_file.path()).expect("read fixed epub");
    (fixed, report)
}

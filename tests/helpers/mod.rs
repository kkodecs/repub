//! Programmatic EPUB builder for tests. No binary fixtures.

#![allow(dead_code)]

use std::io::{Cursor, Write};
use zip::write::{SimpleFileOptions, ZipWriter};
use zip::CompressionMethod;

const CONTAINER_XML: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<container version="1.0" xmlns="urn:oasis:names:tc:opendocument:xmlns:container">
  <rootfiles>
    <rootfile full-path="OEBPS/content.opf" media-type="application/oebps-package+xml"/>
  </rootfiles>
</container>"#;

const CHAPTER_XHTML: &str = r#"<?xml version="1.0" encoding="utf-8"?>
<!DOCTYPE html>
<html xmlns="http://www.w3.org/1999/xhtml">
<head><title>Chapter 1</title></head>
<body>
<h1>Chapter 1</h1>
<p>Hello world.</p>
</body>
</html>"#;

const VALID_OPF: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<package xmlns="http://www.idpf.org/2007/opf" unique-identifier="BookId" version="3.0">
  <metadata xmlns:dc="http://purl.org/dc/elements/1.1/">
    <dc:title>Test Book</dc:title>
    <dc:language>en</dc:language>
    <dc:identifier id="BookId">urn:uuid:12345678-1234-1234-1234-123456789abc</dc:identifier>
    <meta property="dcterms:modified">2026-01-01T00:00:00Z</meta>
  </metadata>
  <manifest>
    <item id="ch1" href="chapter01.xhtml" media-type="application/xhtml+xml"/>
  </manifest>
  <spine>
    <itemref idref="ch1"/>
  </spine>
</package>"#;

pub fn container_xml() -> &'static str {
    CONTAINER_XML
}

pub fn valid_opf() -> &'static str {
    VALID_OPF
}

pub fn chapter_xhtml() -> &'static str {
    CHAPTER_XHTML
}

/// A helper to build minimal EPUBs with specific defects.
pub struct EpubBuilder {
    entries: Vec<(String, Vec<u8>, bool)>, // (name, content, stored)
}

impl EpubBuilder {
    pub fn new() -> Self {
        Self {
            entries: Vec::new(),
        }
    }

    pub fn add(mut self, name: &str, content: &[u8], stored: bool) -> Self {
        self.entries
            .push((name.to_owned(), content.to_vec(), stored));
        self
    }

    pub fn build(self) -> Vec<u8> {
        let buf = Cursor::new(Vec::new());
        let mut zip = ZipWriter::new(buf);

        for (name, content, stored) in &self.entries {
            let method = if *stored {
                CompressionMethod::Stored
            } else {
                CompressionMethod::Deflated
            };
            let opts = SimpleFileOptions::default().compression_method(method);
            zip.start_file(name, opts)
                .expect("start_file should succeed");
            zip.write_all(content).expect("write_all should succeed");
        }

        zip.finish().expect("finish should succeed").into_inner()
    }
}

// ── Ready-made defective EPUBs ──────────────────────────────────────────

/// A valid EPUB3 with no issues.
pub fn build_epub_valid() -> Vec<u8> {
    EpubBuilder::new()
        .add("mimetype", b"application/epub+zip", true)
        .add("META-INF/container.xml", CONTAINER_XML.as_bytes(), false)
        .add("OEBPS/content.opf", VALID_OPF.as_bytes(), false)
        .add("OEBPS/chapter01.xhtml", CHAPTER_XHTML.as_bytes(), false)
        .build()
}

/// Mimetype entry is compressed (should be stored).
pub fn build_epub_compressed_mimetype() -> Vec<u8> {
    EpubBuilder::new()
        .add("mimetype", b"application/epub+zip", false) // deflated!
        .add("META-INF/container.xml", CONTAINER_XML.as_bytes(), false)
        .add("OEBPS/content.opf", VALID_OPF.as_bytes(), false)
        .add("OEBPS/chapter01.xhtml", CHAPTER_XHTML.as_bytes(), false)
        .build()
}

/// Missing dc:language in OPF.
pub fn build_epub_missing_language() -> Vec<u8> {
    let opf = r#"<?xml version="1.0" encoding="UTF-8"?>
<package xmlns="http://www.idpf.org/2007/opf" unique-identifier="BookId" version="3.0">
  <metadata xmlns:dc="http://purl.org/dc/elements/1.1/">
    <dc:title>Test Book</dc:title>
    <dc:identifier id="BookId">urn:uuid:12345678-1234-1234-1234-123456789abc</dc:identifier>
    <meta property="dcterms:modified">2026-01-01T00:00:00Z</meta>
  </metadata>
  <manifest>
    <item id="ch1" href="chapter01.xhtml" media-type="application/xhtml+xml"/>
  </manifest>
  <spine>
    <itemref idref="ch1"/>
  </spine>
</package>"#;

    EpubBuilder::new()
        .add("mimetype", b"application/epub+zip", true)
        .add("META-INF/container.xml", CONTAINER_XML.as_bytes(), false)
        .add("OEBPS/content.opf", opf.as_bytes(), false)
        .add("OEBPS/chapter01.xhtml", CHAPTER_XHTML.as_bytes(), false)
        .build()
}

/// XHTML file without XML declaration.
pub fn build_epub_no_xml_decl() -> Vec<u8> {
    let xhtml_no_decl = r#"<!DOCTYPE html>
<html xmlns="http://www.w3.org/1999/xhtml">
<head><title>Chapter 1</title></head>
<body>
<h1>Chapter 1</h1>
<p>Hello world.</p>
</body>
</html>"#;

    EpubBuilder::new()
        .add("mimetype", b"application/epub+zip", true)
        .add("META-INF/container.xml", CONTAINER_XML.as_bytes(), false)
        .add("OEBPS/content.opf", VALID_OPF.as_bytes(), false)
        .add("OEBPS/chapter01.xhtml", xhtml_no_decl.as_bytes(), false)
        .build()
}

/// OPF with vendor-specific identifiers, tool metadata, xmlns, and contributor.
pub fn build_epub_with_vendor_ids() -> Vec<u8> {
    let opf = r#"<?xml version="1.0" encoding="UTF-8"?>
<package xmlns="http://www.idpf.org/2007/opf" unique-identifier="BookId" version="3.0">
  <metadata xmlns:dc="http://purl.org/dc/elements/1.1/" xmlns:opf="http://www.idpf.org/2007/opf" xmlns:calibre="http://calibre.kovidgoyal.net/2009/metadata">
    <dc:title>Test Book</dc:title>
    <dc:language>en</dc:language>
    <dc:identifier id="BookId">urn:uuid:12345678-1234-1234-1234-123456789abc</dc:identifier>
    <dc:identifier opf:scheme="AMAZON">B00TESTID</dc:identifier>
    <dc:identifier opf:scheme="calibre">deadbeef-cafe-1234-5678-abcdef012345</dc:identifier>
    <dc:identifier>urn:amazon:asin:B00CONTENT</dc:identifier>
    <dc:identifier>urn:isbn:9781234567890</dc:identifier>
    <dc:contributor opf:role="bkp">calibre (6.0.0) [https://calibre-ebook.com]</dc:contributor>
    <dc:contributor opf:role="trl">Jane Smith</dc:contributor>
    <meta property="dcterms:modified">2026-01-01T00:00:00Z</meta>
    <meta name="calibre:timestamp" content="2026-01-01T00:00:00+00:00"/>
    <meta name="calibre:title_sort" content="Test Book, The"/>
    <meta name="calibre:series" content="Test Series"/>
    <meta name="calibre:series_index" content="1"/>
  </metadata>
  <manifest>
    <item id="ch1" href="chapter01.xhtml" media-type="application/xhtml+xml"/>
  </manifest>
  <spine>
    <itemref idref="ch1"/>
  </spine>
</package>"#;

    EpubBuilder::new()
        .add("mimetype", b"application/epub+zip", true)
        .add("META-INF/container.xml", CONTAINER_XML.as_bytes(), false)
        .add("OEBPS/content.opf", opf.as_bytes(), false)
        .add("OEBPS/chapter01.xhtml", CHAPTER_XHTML.as_bytes(), false)
        .build()
}

/// EPUB with DRM markers (META-INF/rights.xml).
pub fn build_epub_with_drm() -> Vec<u8> {
    EpubBuilder::new()
        .add("mimetype", b"application/epub+zip", true)
        .add("META-INF/container.xml", CONTAINER_XML.as_bytes(), false)
        .add("META-INF/rights.xml", b"<rights/>", false)
        .add("OEBPS/content.opf", VALID_OPF.as_bytes(), false)
        .add("OEBPS/chapter01.xhtml", CHAPTER_XHTML.as_bytes(), false)
        .build()
}

/// Missing dc:identifier, broken unique-identifier reference.
pub fn build_epub_missing_identifier() -> Vec<u8> {
    let opf = r#"<?xml version="1.0" encoding="UTF-8"?>
<package xmlns="http://www.idpf.org/2007/opf" unique-identifier="BadRef" version="3.0">
  <metadata xmlns:dc="http://purl.org/dc/elements/1.1/">
    <dc:title>Test Book</dc:title>
    <dc:language>en</dc:language>
    <meta property="dcterms:modified">2026-01-01T00:00:00Z</meta>
  </metadata>
  <manifest>
    <item id="ch1" href="chapter01.xhtml" media-type="application/xhtml+xml"/>
  </manifest>
  <spine>
    <itemref idref="ch1"/>
  </spine>
</package>"#;

    EpubBuilder::new()
        .add("mimetype", b"application/epub+zip", true)
        .add("META-INF/container.xml", CONTAINER_XML.as_bytes(), false)
        .add("OEBPS/content.opf", opf.as_bytes(), false)
        .add("OEBPS/chapter01.xhtml", CHAPTER_XHTML.as_bytes(), false)
        .build()
}

/// XHTML with <img> tags missing src and <script> tags.
pub fn build_epub_bad_content() -> Vec<u8> {
    let xhtml = r#"<?xml version="1.0" encoding="utf-8"?>
<!DOCTYPE html>
<html xmlns="http://www.w3.org/1999/xhtml">
<head><title>Chapter 1</title></head>
<body>
<h1>Chapter 1</h1>
<img />
<img src="real.png" alt="real"/>
<p>Hello world.</p>
<script type="text/javascript">alert('hi');</script>
</body>
</html>"#;

    EpubBuilder::new()
        .add("mimetype", b"application/epub+zip", true)
        .add("META-INF/container.xml", CONTAINER_XML.as_bytes(), false)
        .add("OEBPS/content.opf", VALID_OPF.as_bytes(), false)
        .add("OEBPS/chapter01.xhtml", xhtml.as_bytes(), false)
        .build()
}

/// NCX with body-ID fragment reference.
pub fn build_epub_ncx_body_id() -> Vec<u8> {
    let xhtml = r#"<?xml version="1.0" encoding="utf-8"?>
<html xmlns="http://www.w3.org/1999/xhtml">
<head><title>Chapter 1</title></head>
<body id="main-body">
<h1>Chapter 1</h1>
<p>Hello world.</p>
</body>
</html>"#;

    let ncx = r#"<?xml version="1.0" encoding="UTF-8"?>
<ncx xmlns="http://www.daisy.org/z3986/2005/ncx/" version="2005-1">
  <navMap>
    <navPoint id="navpoint-1">
      <navLabel><text>Chapter 1</text></navLabel>
      <content src="chapter01.xhtml#main-body"/>
    </navPoint>
  </navMap>
</ncx>"#;

    let opf = r#"<?xml version="1.0" encoding="UTF-8"?>
<package xmlns="http://www.idpf.org/2007/opf" unique-identifier="BookId" version="2.0">
  <metadata xmlns:dc="http://purl.org/dc/elements/1.1/">
    <dc:title>Test Book</dc:title>
    <dc:language>en</dc:language>
    <dc:identifier id="BookId">urn:uuid:12345678-1234-1234-1234-123456789abc</dc:identifier>
  </metadata>
  <manifest>
    <item id="ch1" href="chapter01.xhtml" media-type="application/xhtml+xml"/>
    <item id="ncx" href="toc.ncx" media-type="application/x-dtbncx+xml"/>
  </manifest>
  <spine toc="ncx">
    <itemref idref="ch1"/>
  </spine>
</package>"#;

    EpubBuilder::new()
        .add("mimetype", b"application/epub+zip", true)
        .add("META-INF/container.xml", CONTAINER_XML.as_bytes(), false)
        .add("OEBPS/content.opf", opf.as_bytes(), false)
        .add("OEBPS/chapter01.xhtml", xhtml.as_bytes(), false)
        .add("OEBPS/toc.ncx", ncx.as_bytes(), false)
        .build()
}

/// EPUB2 (version 2.0) — dcterms:modified should NOT be added.
pub fn build_epub2_valid() -> Vec<u8> {
    let opf = r#"<?xml version="1.0" encoding="UTF-8"?>
<package xmlns="http://www.idpf.org/2007/opf" unique-identifier="BookId" version="2.0">
  <metadata xmlns:dc="http://purl.org/dc/elements/1.1/">
    <dc:title>Test Book</dc:title>
    <dc:language>en</dc:language>
    <dc:identifier id="BookId">urn:uuid:12345678-1234-1234-1234-123456789abc</dc:identifier>
  </metadata>
  <manifest>
    <item id="ch1" href="chapter01.xhtml" media-type="application/xhtml+xml"/>
  </manifest>
  <spine>
    <itemref idref="ch1"/>
  </spine>
</package>"#;

    EpubBuilder::new()
        .add("mimetype", b"application/epub+zip", true)
        .add("META-INF/container.xml", CONTAINER_XML.as_bytes(), false)
        .add("OEBPS/content.opf", opf.as_bytes(), false)
        .add("OEBPS/chapter01.xhtml", CHAPTER_XHTML.as_bytes(), false)
        .build()
}

/// EPUB3 missing dcterms:modified.
pub fn build_epub3_missing_modified() -> Vec<u8> {
    let opf = r#"<?xml version="1.0" encoding="UTF-8"?>
<package xmlns="http://www.idpf.org/2007/opf" unique-identifier="BookId" version="3.0">
  <metadata xmlns:dc="http://purl.org/dc/elements/1.1/">
    <dc:title>Test Book</dc:title>
    <dc:language>en</dc:language>
    <dc:identifier id="BookId">urn:uuid:12345678-1234-1234-1234-123456789abc</dc:identifier>
  </metadata>
  <manifest>
    <item id="ch1" href="chapter01.xhtml" media-type="application/xhtml+xml"/>
  </manifest>
  <spine>
    <itemref idref="ch1"/>
  </spine>
</package>"#;

    EpubBuilder::new()
        .add("mimetype", b"application/epub+zip", true)
        .add("META-INF/container.xml", CONTAINER_XML.as_bytes(), false)
        .add("OEBPS/content.opf", opf.as_bytes(), false)
        .add("OEBPS/chapter01.xhtml", CHAPTER_XHTML.as_bytes(), false)
        .build()
}

/// Language is "und" (undefined) — should be replaced.
pub fn build_epub_und_language() -> Vec<u8> {
    let opf = r#"<?xml version="1.0" encoding="UTF-8"?>
<package xmlns="http://www.idpf.org/2007/opf" unique-identifier="BookId" version="3.0">
  <metadata xmlns:dc="http://purl.org/dc/elements/1.1/">
    <dc:title>Test Book</dc:title>
    <dc:language>und</dc:language>
    <dc:identifier id="BookId">urn:uuid:12345678-1234-1234-1234-123456789abc</dc:identifier>
    <meta property="dcterms:modified">2026-01-01T00:00:00Z</meta>
  </metadata>
  <manifest>
    <item id="ch1" href="chapter01.xhtml" media-type="application/xhtml+xml"/>
  </manifest>
  <spine>
    <itemref idref="ch1"/>
  </spine>
</package>"#;

    EpubBuilder::new()
        .add("mimetype", b"application/epub+zip", true)
        .add("META-INF/container.xml", CONTAINER_XML.as_bytes(), false)
        .add("OEBPS/content.opf", opf.as_bytes(), false)
        .add("OEBPS/chapter01.xhtml", CHAPTER_XHTML.as_bytes(), false)
        .build()
}

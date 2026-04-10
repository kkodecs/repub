//! Tier 2 tests: dcterms:modified, NCX body-ID, img without src, script removal.

use repub::Repub;
use std::io::{Cursor, Read};
use zip::read::ZipArchive;

#[path = "helpers/mod.rs"]
mod helpers;

// ── dcterms:modified ────────────────────────────────────────────────────

#[test]
fn fix_missing_modified_epub3() {
    let epub = helpers::build_epub3_missing_modified();
    let fixed = Repub::new().fix_bytes(&epub).unwrap();

    let mut archive = ZipArchive::new(Cursor::new(&fixed)).unwrap();
    let mut entry = archive.by_name("OEBPS/content.opf").unwrap();
    let mut content = String::new();
    entry.read_to_string(&mut content).unwrap();
    assert!(content.contains("dcterms:modified"));
}

#[test]
fn epub2_does_not_get_modified_timestamp() {
    let epub = helpers::build_epub2_valid();
    let fixed = Repub::new().fix_bytes(&epub).unwrap();

    let mut archive = ZipArchive::new(Cursor::new(&fixed)).unwrap();
    let mut entry = archive.by_name("OEBPS/content.opf").unwrap();
    let mut content = String::new();
    entry.read_to_string(&mut content).unwrap();
    assert!(!content.contains("dcterms:modified"));
}

// ── NCX body-ID ─────────────────────────────────────────────────────────

#[test]
fn fix_ncx_body_id_fragment() {
    let epub = helpers::build_epub_ncx_body_id();
    let fixed = Repub::new().fix_bytes(&epub).unwrap();

    let mut archive = ZipArchive::new(Cursor::new(&fixed)).unwrap();
    let mut entry = archive.by_name("OEBPS/toc.ncx").unwrap();
    let mut content = String::new();
    entry.read_to_string(&mut content).unwrap();

    // Fragment should be stripped
    assert!(
        content.contains(r#"src="chapter01.xhtml""#),
        "NCX should have fragment stripped, got: {content}"
    );
    assert!(
        !content.contains("#main-body"),
        "NCX should not contain body fragment"
    );
}

// ── <img> without src ───────────────────────────────────────────────────

#[test]
fn fix_img_without_src() {
    let epub = helpers::build_epub_bad_content();
    let fixed = Repub::new().fix_bytes(&epub).unwrap();

    let mut archive = ZipArchive::new(Cursor::new(&fixed)).unwrap();
    let mut entry = archive.by_name("OEBPS/chapter01.xhtml").unwrap();
    let mut content = String::new();
    entry.read_to_string(&mut content).unwrap();

    // img with src should still be present
    assert!(content.contains("real.png"), "img with src should be kept");
}

// ── <script> removal ────────────────────────────────────────────────────

#[test]
fn fix_removes_script_tags() {
    let epub = helpers::build_epub_bad_content();
    let fixed = Repub::new().fix_bytes(&epub).unwrap();

    let mut archive = ZipArchive::new(Cursor::new(&fixed)).unwrap();
    let mut entry = archive.by_name("OEBPS/chapter01.xhtml").unwrap();
    let mut content = String::new();
    entry.read_to_string(&mut content).unwrap();

    assert!(
        !content.contains("<script"),
        "script tags should be removed"
    );
    assert!(
        !content.contains("alert"),
        "script content should be removed"
    );
}

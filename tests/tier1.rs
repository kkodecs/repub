//! Tier 1 tests: mimetype, XML declaration, dc:language, dc:identifier, DRM.

use repub::{Repub, RepubError};
use std::io::{Cursor, Read};
use zip::read::ZipArchive;

#[path = "helpers/mod.rs"]
mod helpers;

// ── Mimetype ────────────────────────────────────────────────────────────

#[test]
fn fix_compressed_mimetype() {
    let epub = helpers::build_epub_compressed_mimetype();
    let repub = Repub::new();
    let fixed = repub.fix_bytes(&epub).unwrap();

    let mut archive = ZipArchive::new(Cursor::new(&fixed)).unwrap();
    let entry = archive.by_index(0).unwrap();
    assert_eq!(entry.name(), "mimetype");
    assert_eq!(entry.compression(), zip::CompressionMethod::Stored);
    drop(entry);

    let mut entry = archive.by_index(0).unwrap();
    let mut content = Vec::new();
    entry.read_to_end(&mut content).unwrap();
    assert_eq!(content, b"application/epub+zip");
}

#[test]
fn valid_mimetype_not_flagged() {
    let epub = helpers::build_epub_valid();
    let fixed = Repub::new().fix_bytes(&epub).unwrap();
    let archive = ZipArchive::new(Cursor::new(&fixed)).unwrap();
    assert!(!archive.is_empty());
}

// ── XML Declaration ─────────────────────────────────────────────────────

#[test]
fn fix_missing_xml_declaration() {
    let epub = helpers::build_epub_no_xml_decl();
    let repub = Repub::new();
    let fixed = repub.fix_bytes(&epub).unwrap();

    let mut archive = ZipArchive::new(Cursor::new(&fixed)).unwrap();
    let mut entry = archive.by_name("OEBPS/chapter01.xhtml").unwrap();
    let mut content = String::new();
    entry.read_to_string(&mut content).unwrap();
    assert!(content.starts_with("<?xml"));
}

#[test]
fn existing_xml_declaration_not_duplicated() {
    let epub = helpers::build_epub_valid();
    let fixed = Repub::new().fix_bytes(&epub).unwrap();

    let mut archive = ZipArchive::new(Cursor::new(&fixed)).unwrap();
    let mut entry = archive.by_name("OEBPS/chapter01.xhtml").unwrap();
    let mut content = String::new();
    entry.read_to_string(&mut content).unwrap();

    // Should have exactly one XML declaration
    let count = content.matches("<?xml").count();
    assert_eq!(count, 1);
}

// ── dc:language ─────────────────────────────────────────────────────────

#[test]
fn fix_missing_language() {
    let epub = helpers::build_epub_missing_language();
    let repub = Repub::new();
    let fixed = repub.fix_bytes(&epub).unwrap();

    let mut archive = ZipArchive::new(Cursor::new(&fixed)).unwrap();
    let mut entry = archive.by_name("OEBPS/content.opf").unwrap();
    let mut content = String::new();
    entry.read_to_string(&mut content).unwrap();
    assert!(content.contains("<dc:language>en</dc:language>"));
}

#[test]
fn fix_und_language() {
    let epub = helpers::build_epub_und_language();
    let repub = Repub::new().default_language("fr");
    let fixed = repub.fix_bytes(&epub).unwrap();

    let mut archive = ZipArchive::new(Cursor::new(&fixed)).unwrap();
    let mut entry = archive.by_name("OEBPS/content.opf").unwrap();
    let mut content = String::new();
    entry.read_to_string(&mut content).unwrap();
    assert!(content.contains("fr"));
    assert!(!content.contains(">und<"));
}

#[test]
fn valid_language_not_changed() {
    let epub = helpers::build_epub_valid();
    let fixed = Repub::new().fix_bytes(&epub).unwrap();

    let mut archive = ZipArchive::new(Cursor::new(&fixed)).unwrap();
    let mut entry = archive.by_name("OEBPS/content.opf").unwrap();
    let mut content = String::new();
    entry.read_to_string(&mut content).unwrap();
    assert!(content.contains("<dc:language>en</dc:language>"));
}

// ── dc:identifier ───────────────────────────────────────────────────────

#[test]
fn fix_missing_identifier() {
    let epub = helpers::build_epub_missing_identifier();
    let repub = Repub::new();
    let fixed = repub.fix_bytes(&epub).unwrap();

    let mut archive = ZipArchive::new(Cursor::new(&fixed)).unwrap();
    let mut entry = archive.by_name("OEBPS/content.opf").unwrap();
    let mut content = String::new();
    entry.read_to_string(&mut content).unwrap();
    assert!(content.contains("dc:identifier"));
    assert!(content.contains("urn:uuid:"));
    assert!(content.contains("repub-id"));
}

// ── DRM Detection ───────────────────────────────────────────────────────

#[test]
fn drm_epub_returns_error() {
    let epub = helpers::build_epub_with_drm();
    let result = Repub::new().fix_bytes(&epub);
    assert!(result.is_err());
    match result.unwrap_err() {
        RepubError::DrmDetected { .. } => {} // expected
        other => panic!("expected DrmDetected, got: {other}"),
    }
}

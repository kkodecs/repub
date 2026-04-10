//! Tier 3 tests: proprietary metadata removal.

use repub::Repub;
use std::io::{Cursor, Read};
use zip::read::ZipArchive;

#[path = "helpers/mod.rs"]
mod helpers;

// ── Vendor identifiers ──────────────────────────────────────────────────

#[test]
fn fix_removes_vendor_identifiers() {
    let epub = helpers::build_epub_with_vendor_ids();
    let fixed = Repub::new().fix_bytes(&epub).unwrap();

    let mut archive = ZipArchive::new(Cursor::new(&fixed)).unwrap();
    let mut entry = archive.by_name("OEBPS/content.opf").unwrap();
    let mut content = String::new();
    entry.read_to_string(&mut content).unwrap();

    assert!(
        !content.contains("AMAZON"),
        "Amazon identifier should be removed"
    );
    assert!(
        !content.contains("B00TESTID"),
        "Amazon ASIN should be removed"
    );
    assert!(
        !content.contains(r#"scheme="calibre""#),
        "calibre identifier should be removed"
    );

    // Standard identifier should be preserved
    assert!(
        content.contains("urn:uuid:12345678"),
        "UUID identifier should be preserved"
    );
}

// ── Tool metadata ───────────────────────────────────────────────────────

#[test]
fn fix_removes_tool_metadata() {
    let epub = helpers::build_epub_with_vendor_ids();
    let fixed = Repub::new().fix_bytes(&epub).unwrap();

    let mut archive = ZipArchive::new(Cursor::new(&fixed)).unwrap();
    let mut entry = archive.by_name("OEBPS/content.opf").unwrap();
    let mut content = String::new();
    entry.read_to_string(&mut content).unwrap();

    assert!(
        !content.contains("calibre:timestamp"),
        "calibre:timestamp should be removed"
    );
    assert!(
        !content.contains("calibre:title_sort"),
        "calibre:title_sort should be removed"
    );
}

// ── Preserved metadata ──────────────────────────────────────────────────

#[test]
fn fix_preserves_series_metadata() {
    let epub = helpers::build_epub_with_vendor_ids();
    let fixed = Repub::new().fix_bytes(&epub).unwrap();

    let mut archive = ZipArchive::new(Cursor::new(&fixed)).unwrap();
    let mut entry = archive.by_name("OEBPS/content.opf").unwrap();
    let mut content = String::new();
    entry.read_to_string(&mut content).unwrap();

    assert!(
        content.contains("calibre:series"),
        "calibre:series should be preserved"
    );
    assert!(
        content.contains("calibre:series_index"),
        "calibre:series_index should be preserved"
    );
}

// ── Content-based vendor identifier ─────────────────────────────────────

#[test]
fn fix_removes_content_based_vendor_identifier() {
    let epub = helpers::build_epub_with_vendor_ids();
    let fixed = Repub::new().fix_bytes(&epub).unwrap();

    let mut archive = ZipArchive::new(Cursor::new(&fixed)).unwrap();
    let mut entry = archive.by_name("OEBPS/content.opf").unwrap();
    let mut content = String::new();
    entry.read_to_string(&mut content).unwrap();

    assert!(
        !content.contains("urn:amazon:asin"),
        "content-based Amazon ASIN should be removed"
    );
    assert!(
        content.contains("urn:isbn:9781234567890"),
        "ISBN should be preserved"
    );
}

// ── xmlns stripping ────────────────────────────────────────────────────

#[test]
fn fix_removes_vendor_xmlns() {
    let epub = helpers::build_epub_with_vendor_ids();
    let fixed = Repub::new().fix_bytes(&epub).unwrap();

    let mut archive = ZipArchive::new(Cursor::new(&fixed)).unwrap();
    let mut entry = archive.by_name("OEBPS/content.opf").unwrap();
    let mut content = String::new();
    entry.read_to_string(&mut content).unwrap();

    assert!(
        !content.contains("xmlns:calibre"),
        "calibre namespace should be removed"
    );
    // Standard namespaces should survive
    assert!(
        content.contains("xmlns:dc"),
        "dc namespace should be preserved"
    );
}

// ── Contributor stripping ──────────────────────────────────────────────

#[test]
fn fix_removes_tool_contributor() {
    let epub = helpers::build_epub_with_vendor_ids();
    let fixed = Repub::new().fix_bytes(&epub).unwrap();

    let mut archive = ZipArchive::new(Cursor::new(&fixed)).unwrap();
    let mut entry = archive.by_name("OEBPS/content.opf").unwrap();
    let mut content = String::new();
    entry.read_to_string(&mut content).unwrap();

    assert!(
        !content.contains("calibre (6.0.0)"),
        "calibre contributor should be removed"
    );
}

#[test]
fn fix_preserves_human_contributor() {
    let epub = helpers::build_epub_with_vendor_ids();
    let fixed = Repub::new().fix_bytes(&epub).unwrap();

    let mut archive = ZipArchive::new(Cursor::new(&fixed)).unwrap();
    let mut entry = archive.by_name("OEBPS/content.opf").unwrap();
    let mut content = String::new();
    entry.read_to_string(&mut content).unwrap();

    assert!(
        content.contains("Jane Smith"),
        "human translator contributor should be preserved"
    );
}

// ── strip_proprietary = false ───────────────────────────────────────────

#[test]
fn no_strip_preserves_everything() {
    let epub = helpers::build_epub_with_vendor_ids();
    let fixed = Repub::new()
        .strip_proprietary(false)
        .fix_bytes(&epub)
        .unwrap();

    let mut archive = ZipArchive::new(Cursor::new(&fixed)).unwrap();
    let mut entry = archive.by_name("OEBPS/content.opf").unwrap();
    let mut content = String::new();
    entry.read_to_string(&mut content).unwrap();

    assert!(
        content.contains("AMAZON"),
        "Should preserve vendor IDs when strip is off"
    );
    assert!(
        content.contains("calibre:timestamp"),
        "Should preserve tool metadata when strip is off"
    );
}

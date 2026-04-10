# repub — Standalone EPUB Repair Crate

## What
The first standalone EPUB repair library in any language. Produces pristine,
spec-compliant EPUBs that pass validation and render flawlessly on every
device and reading platform. Library + CLI.

No equivalent exists in Rust, Python, JavaScript, Go, or any other
language. Existing tools handle repair as features within larger apps
(Calibre, Sigil). repub is purpose-built for one job: take an EPUB with
issues, make it pristine.

## Repo
- GitHub: kkodecs/repub
- License: MIT
- Rust edition: 2021
- MSRV: 1.75 (or whatever current stable is)

## Guiding Principles

1. **Pristine metadata.** Every EPUB that passes through repub should have
   complete, valid, spec-compliant metadata. Not just "good enough to not
   get rejected" — actually correct.

2. **Flawless reading experience.** Fixes should improve how the book
   renders on devices. Remove things that interfere with reading (broken
   links, stray elements, scripts that get stripped anyway).

3. **Always reversible.** Never modify the original file. Write to a new
   file or return new bytes. The caller decides whether to replace. In CLI
   mode, default to `book.repub.epub` alongside the original.

4. **Remove proprietary fingerprints.** Strip anything proprietary that
   doesn't have functional value. Vendor-specific identifiers, tool-specific
   metadata, provenance data, non-standard namespace declarations, and
   tool stamps in contributor fields. The output should be a clean,
   vendor-neutral EPUB. If a proprietary element is load-bearing (e.g.,
   CSS class names used by stylesheets), leave it alone.

5. **First, do no harm.** If an EPUB is too broken to fix safely, report
   what's wrong and bail — don't produce a worse file. Detect DRM and
   refuse to process (with a clear message).

## Dependencies
- `zip` (2.x) — ZIP reading/writing
- `quick-xml` (0.37) — OPF/NCX/XHTML parsing
- `uuid` (1.x, features = ["v4"]) — generate identifiers
- `clap` (4.x) — CLI argument parsing (binary only)

No async. Pure synchronous. Users wrap in spawn_blocking if needed.

## API Design

### Library
```rust
use repub::{Repub, RepubOptions, RepubReport, Fix};

// Simple — fix and write to new file
let report = Repub::new().fix("book.epub", "book.fixed.epub")?;

// With options
let report = Repub::new()
    .default_language("en")
    .strip_proprietary(true)  // default: true
    .fix("book.epub", "book.fixed.epub")?;

// In-memory (for servers — no disk I/O)
let fixed_bytes = Repub::new()
    .default_language("en")
    .fix_bytes(&original_bytes)?;

// Dry run — report what would be fixed without writing
let report = Repub::new().check("book.epub")?;

// Report
assert!(!report.fixes.is_empty());
for fix in &report.fixes {
    println!("{fix}");
    // "Added XML declaration to OEBPS/chapter01.xhtml"
    // "Added dc:language: en"
    // "Removed vendor-specific identifier"
}
println!("{} issues fixed", report.fixes.len());
println!("modified: {}", report.modified); // false if no changes needed
```

### RepubReport struct
```rust
pub struct RepubReport {
    pub fixes: Vec<Fix>,      // What was changed
    pub warnings: Vec<String>, // Non-fatal issues noticed but not fixed
    pub modified: bool,        // Whether any changes were made
}
```

### Fix enum — every possible fix the tool can make
```rust
pub enum Fix {
    MimetypeFixed,                          // Rewritten as first entry, stored
    XmlDeclarationAdded { file: String },   // Added <?xml?> to XHTML
    LanguageAdded { language: String },     // Added dc:language
    IdentifierAdded { id: String },         // Added dc:identifier (UUID)
    UniqueIdentifierFixed,                  // Fixed package unique-identifier ref
    ModifiedTimestampAdded,                 // Added dcterms:modified
    NcxBodyIdFixed { file: String },        // Stripped body fragment from NCX
    StrayImgRemoved { file: String },       // Removed <img> without src
    ScriptRemoved { file: String },         // Removed <script> tag
    ProprietaryMetadataRemoved { detail: String }, // Removed vendor metadata
}

impl std::fmt::Display for Fix { ... } // Human-readable descriptions
```

### CLI
```
$ repub fix book.epub
  + Fixed mimetype ZIP entry (was compressed)
  + Added XML declaration to OEBPS/chapter01.xhtml
  + Added XML declaration to OEBPS/chapter02.xhtml
  + Added dc:language: en
  + Removed vendor-specific metadata: calibre:timestamp
  + Removed vendor-specific identifier
  6 fixes applied -> book.repub.epub

$ repub fix book.epub -o fixed/book.epub
  # writes to specific output path

$ repub fix book.epub --in-place
  # overwrites original (explicit opt-in only)

$ repub check book.epub
  ! Missing XML declaration in OEBPS/chapter01.xhtml
  ! Missing dc:language
  ! Vendor-specific identifier present
  3 issues found (use 'repub fix' to repair)

$ repub fix *.epub
  # batch mode — process multiple files

$ repub fix book.epub --language fr
  # override default language
```

## Fixes to Implement (Priority Order)

### Tier 1: Required (prevents ingestion failures)

1. **Mimetype ZIP entry** — Must be the first entry in the ZIP archive,
   stored (not compressed), no extra fields in local header, exact content
   `application/epub+zip` (no trailing whitespace/newline). Rewrite the
   ZIP to ensure all of this.

2. **XML encoding declarations** — Prepend
   `<?xml version="1.0" encoding="utf-8"?>` followed by a newline to all
   .xhtml and .html content files that lack it. Check for existing
   declaration first (don't duplicate). If a BOM is present, place the
   declaration after it. Without this declaration, some converters assume
   ISO-8859-1 and corrupt non-ASCII characters (curly quotes, em dashes,
   accented characters).

3. **`dc:language`** — Ensure present in OPF <metadata>. If missing,
   insert with configured default (default: "en"). Must be valid ISO
   639-1 (2-char) or ISO 639-2 (3-char) code. If present but empty or
   "UND"/"und", replace with default.

4. **`dc:identifier`** with proper referencing — Ensure at least one
   dc:identifier exists. If none present, generate:
   `<dc:identifier id="repub-id">urn:uuid:{v4}</dc:identifier>`. Ensure
   `<package unique-identifier="X">` attribute references a real
   `<dc:identifier id="X">`. If broken or missing, fix the reference.

5. **DRM detection** — Check for Adobe DRM (META-INF/encryption.xml with
   non-font entries, META-INF/rights.xml) and other DRM markers. If
   detected, return an error immediately — do not attempt to process.

### Tier 2: Should have (spec compliance + edge-case fixes)

6. **`dcterms:modified`** — Add or update
   `<meta property="dcterms:modified">` with current UTC timestamp in
   ISO 8601 format (e.g., `2026-04-10T00:00:00Z`). Required by EPUB3
   spec. Only add for EPUB3 files (check `<package version="3.0">`).

7. **NCX body-ID link fix** — In .ncx files, find
   `<content src="file.xhtml#id">` entries. For each, check if the
   fragment target is the `<body>` element's id attribute (not an internal
   anchor). If so, strip the fragment. These crash some EPUB converters.
   To check: read the referenced XHTML file, parse for
   `<body id="...">`, compare.

8. **Remove `<img>` without `src`** — In XHTML content files, remove
   `<img>` and `<img />` tags that have no `src` attribute. These cause
   converter failures.

9. **Remove `<script>` tags** — Strip `<script>...</script>` and
   `<script .../>` from XHTML content files. E-readers don't execute
   JavaScript; removing scripts reduces file size and avoids converter
   issues.

### Tier 3: Proprietary data removal

10. **Remove vendor-specific identifiers** — Strip `<dc:identifier>`
    elements with vendor-specific schemes, including but not limited to:
    - `opf:scheme="AMAZON"` or `opf:scheme="MOBI-ASIN"`
    - Content starting with `urn:amazon:asin:`
    - `opf:scheme="GOOGLE"`
    - `opf:scheme="GOODREADS"`
    - `opf:scheme="BARNESNOBLE"`
    - `opf:scheme="calibre"`

    Preserve:
    - ISBN identifiers (any scheme)
    - UUID identifiers
    - DOI identifiers

11. **Remove tool-specific metadata** — Strip:
    - `<meta name="calibre:timestamp" ...>`
    - `<meta name="calibre:title_sort" ...>`
    - `<meta name="calibre:author_link_map" ...>`
    - `<meta name="calibre:rating" ...>`
    - `<meta name="calibre:user_categories" ...>`
    - `<meta name="calibre:user_metadata:*" ...>` (any user metadata)
    - `xmlns:calibre="..."` namespace declarations
    - `<dc:contributor>` elements referencing specific tools
    - `<meta content="..." name="Sigil version"/>`

    Do NOT remove:
    - `<meta name="calibre:series" ...>` (de facto standard for series)
    - `<meta name="calibre:series_index" ...>` (same)
    These are the de facto standard for series metadata in EPUBs and are
    widely consumed by reading apps.

## Implementation Architecture

### Project Structure
```
repub/
├── Cargo.toml
├── LICENSE
├── README.md
├── src/
│   ├── lib.rs          # Public API (Repub, RepubReport, Fix)
│   ├── main.rs         # CLI binary
│   ├── zip_repair.rs   # Mimetype + ZIP rewriting
│   ├── opf_repair.rs   # OPF metadata fixes (dc:*, dcterms:*, proprietary)
│   ├── content_repair.rs # XHTML fixes (XML decl, img, script)
│   ├── ncx_repair.rs   # NCX body-ID link fixes
│   ├── drm.rs          # DRM detection
│   └── error.rs        # Error types
└── tests/
    ├── integration.rs  # End-to-end tests
    └── fixtures.rs     # Programmatic test EPUB builder
```

### Core Algorithm (single-pass ZIP rewrite)

```
fn fix(input, output):
    1. Open input as ZipArchive
    2. Check for DRM -> bail if found
    3. Create ZipWriter for output
    4. Write mimetype entry FIRST (stored, no extra fields, exact content)
    5. Find and read the OPF file (via META-INF/container.xml)
    6. Parse OPF, collect fixes needed
    7. Iterate all ZIP entries (skip mimetype, already written):
       - OPF -> apply metadata fixes, write fixed version
       - .xhtml/.html -> check/add XML decl, remove bad img/script, write
       - .ncx -> fix body-ID fragments, write
       - Everything else -> copy as-is, preserving compression
    8. Close ZipWriter
    9. Return RepubReport with all fixes applied
```

### Important Implementation Details

- **Preserve everything we don't fix.** Images, fonts, CSS, other XML —
  copy byte-for-byte. Preserve original compression method on unmodified
  entries.

- **XML declaration fix is byte-level, not XML-level.** Don't parse XHTML
  as XML to add the declaration — many broken EPUBs have malformed XHTML
  that won't parse. Just check if the file starts with `<?xml` (after
  optional BOM) and prepend if not. This is intentionally simple and safe.

- **OPF fixes use quick-xml DOM-style parsing.** Read all events, find
  metadata section, manipulate, rewrite. Same approach as Livrarr's
  update_opf_metadata (battle-tested).

- **Handle both EPUB2 and EPUB3.** Check `<package version="...">`.
  dcterms:modified is EPUB3 only. unique-identifier fixing applies to both.

- **Never add duplicate fixes.** If dc:language already exists and is
  valid, don't touch it. If XML declaration exists, skip. Check before fix.

- **Temp file for safety.** For file-based output, write to a temp file in
  the same directory, then rename. This prevents corruption if the process
  is interrupted.

## Test Strategy

Build test EPUBs programmatically — do NOT commit binary fixtures. Create
a test helper that generates minimal valid EPUBs with specific defects:

```rust
// in tests/fixtures.rs
fn build_epub_missing_language() -> Vec<u8> { ... }
fn build_epub_compressed_mimetype() -> Vec<u8> { ... }
fn build_epub_no_xml_decl() -> Vec<u8> { ... }
fn build_epub_with_vendor_ids() -> Vec<u8> { ... }
fn build_epub_valid() -> Vec<u8> { ... } // no fixes needed
fn build_epub_with_drm() -> Vec<u8> { ... }
```

### Test cases per fix:
- Fix IS applied when the issue exists
- Fix is NOT applied when the issue doesn't exist (no-op)
- Verify by re-reading the fixed EPUB and checking content
- Verify RepubReport accurately describes what was done

### Integration tests:
- Fix an EPUB with ALL issues -> verify all fixes in report
- Fix a valid EPUB -> verify report.modified == false, output == input
- DRM EPUB -> verify error returned, no output produced
- Round-trip: fix(fix(epub)) == fix(epub) (idempotent)
- Batch mode: fix multiple files

## Quality Gate
- `cargo fmt --all -- --check` — zero diffs
- `cargo clippy -- -D warnings` — zero warnings
- `cargo test` — zero failures
- All three must pass before done

## README Content
- Tagline: "The missing EPUB repair tool"
- What it does (one paragraph)
- Why it exists (no standalone EPUB repair library exists in any language)
- Install: `cargo install repub` / `cargo add repub`
- CLI usage with examples
- Library usage with examples
- Complete list of fixes with explanations
- Guiding principles
- License (MIT)
- Contributing section

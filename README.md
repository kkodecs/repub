# repub

The missing EPUB repair tool.

A standalone EPUB repair library and CLI that produces pristine, spec-compliant EPUBs. No equivalent exists in any language — existing tools handle repair as features within larger apps (Calibre, Sigil). repub is purpose-built for one job: take an EPUB with issues, make it pristine.

## Install

```
cargo install repub
```

Or as a library dependency:

```
cargo add repub
```

## CLI Usage

```bash
# Fix an EPUB (writes to book.repub.epub)
repub fix book.epub

# Fix with specific output path
repub fix book.epub -o fixed/book.epub

# Fix in place (overwrites original, backup created)
repub fix book.epub --in-place

# Fix multiple files
repub fix *.epub

# Override default language
repub fix book.epub --language fr

# Dry run — report issues without writing
repub check book.epub
```

## Library Usage

```rust
use repub::Repub;

// Simple — fix and write to new file
let report = Repub::new().fix("book.epub", "book.fixed.epub")?;

// With options
let report = Repub::new()
    .default_language("en")
    .strip_proprietary(true)
    .fix("book.epub", "book.fixed.epub")?;

// In-memory (for servers — no disk I/O)
let fixed_bytes = Repub::new().fix_bytes(&original_bytes)?;

// Dry run
let report = Repub::new().check("book.epub")?;

// Report
for fix in &report.fixes {
    println!("{fix}");
}
```

## What It Fixes

### Tier 1: Required (prevents ingestion failures)
- **Mimetype ZIP entry** — rewritten as first entry, stored, correct content
- **XML declarations** — added to XHTML files that lack them
- **`dc:language`** — added or replaced when missing/invalid
- **`dc:identifier`** — generated with proper `unique-identifier` reference
- **DRM detection** — refuses to process DRM-protected files

### Tier 2: Spec compliance
- **`dcterms:modified`** — added for EPUB3 files
- **NCX body-ID links** — strips fragment references to `<body>` elements
- **Stray `<img>` tags** — removes `<img>` without `src` attribute
- **`<script>` tags** — removed (e-readers don't execute JavaScript)

### Tier 3: Proprietary data removal
- **Vendor identifiers** — strips Amazon, Google, Goodreads, B&N, calibre schemes and content-based patterns (preserves ISBN, UUID, DOI)
- **Tool metadata** — strips calibre timestamps, Sigil versions, etc. (preserves `calibre:series` and `calibre:series_index`)
- **Vendor namespaces** — strips `xmlns:calibre` and similar declarations
- **Tool contributors** — strips `dc:contributor` entries with `role="bkp"` matching known tools

## Guiding Principles

1. **Pristine metadata.** Actually correct, not just good enough.
2. **Flawless reading experience.** Remove things that interfere with reading.
3. **Always reversible.** Never modify the original file.
4. **Remove proprietary fingerprints.** Strip anything proprietary without functional value.
5. **First, do no harm.** If too broken to fix safely, report and bail.

## License

MIT

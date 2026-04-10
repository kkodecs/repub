use std::path::{Path, PathBuf};
use std::process;

use clap::{Parser, Subcommand};
use repub::Repub;

#[derive(Parser)]
#[command(name = "repub", about = "The missing EPUB repair tool")]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Fix one or more EPUB files.
    Fix {
        /// EPUB files to fix.
        #[arg(required = true)]
        files: Vec<PathBuf>,

        /// Output path (single file only).
        #[arg(short, long)]
        output: Option<PathBuf>,

        /// Overwrite the original file.
        #[arg(long)]
        in_place: bool,

        /// Default language when dc:language is missing.
        #[arg(long, default_value = "en")]
        language: String,

        /// Do not strip proprietary metadata.
        #[arg(long)]
        keep_proprietary: bool,
    },
    /// Check an EPUB without modifying it.
    Check {
        /// EPUB file to check.
        #[arg(required = true)]
        file: PathBuf,

        /// Default language when dc:language is missing.
        #[arg(long, default_value = "en")]
        language: String,

        /// Do not flag proprietary metadata.
        #[arg(long)]
        keep_proprietary: bool,
    },
}

fn main() {
    let cli = Cli::parse();

    match cli.command {
        Command::Fix {
            files,
            output,
            in_place,
            language,
            keep_proprietary,
        } => {
            if output.is_some() && files.len() > 1 {
                eprintln!("error: --output can only be used with a single file");
                process::exit(1);
            }

            let mut any_error = false;
            for file in &files {
                let out = if in_place {
                    file.clone()
                } else if let Some(ref o) = output {
                    o.clone()
                } else {
                    default_output_path(file)
                };

                let repub = Repub::new()
                    .default_language(&language)
                    .strip_proprietary(!keep_proprietary);

                match repub.fix(file, &out) {
                    Ok(report) => {
                        if report.fixes.is_empty() {
                            println!("  No issues found in {}", file.display());
                        } else {
                            for fix in &report.fixes {
                                println!("  + {fix}");
                            }
                            println!(
                                "  {} fixes applied -> {}",
                                report.fixes.len(),
                                out.display()
                            );
                        }
                    }
                    Err(e) => {
                        eprintln!("error: {}: {e}", file.display());
                        any_error = true;
                    }
                }
            }

            if any_error {
                process::exit(1);
            }
        }
        Command::Check {
            file,
            language,
            keep_proprietary,
        } => {
            let repub = Repub::new()
                .default_language(&language)
                .strip_proprietary(!keep_proprietary);
            match repub.check(&file) {
                Ok(report) => {
                    if report.fixes.is_empty() {
                        println!("  No issues found");
                    } else {
                        for fix in &report.fixes {
                            println!("  ! {fix}");
                        }
                        println!(
                            "  {} issues found (use 'repub fix' to repair)",
                            report.fixes.len()
                        );
                    }
                }
                Err(e) => {
                    eprintln!("error: {e}");
                    process::exit(1);
                }
            }
        }
    }
}

fn default_output_path(input: &Path) -> PathBuf {
    let stem = input
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("output");
    let ext = input.extension().and_then(|s| s.to_str()).unwrap_or("epub");
    input.with_file_name(format!("{stem}.repub.{ext}"))
}

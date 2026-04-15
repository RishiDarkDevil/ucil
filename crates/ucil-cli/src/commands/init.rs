//! `ucil init` — detect project languages, create `.ucil/`, write `ucil.toml`.

use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context as _, Result};
use clap::Args;
use serde::Serialize;

use ucil_core::schema_migration::SCHEMA_VERSION;

/// Arguments for `ucil init`.
#[derive(Args, Debug)]
pub struct InitArgs {
    /// Directory to initialise (defaults to current directory).
    #[arg(long, default_value = ".")]
    pub dir: PathBuf,
}

// ── Language detection ────────────────────────────────────────────────────────

/// Languages that UCIL can detect and index.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Language {
    Rust,
    TypeScript,
    Python,
    Go,
}

impl std::fmt::Display for Language {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Language::Rust => write!(f, "rust"),
            Language::TypeScript => write!(f, "typescript"),
            Language::Python => write!(f, "python"),
            Language::Go => write!(f, "go"),
        }
    }
}

/// Scans `dir` for language marker files and returns detected languages.
///
/// Detection rules:
/// - Rust: `Cargo.toml` present OR any `*.rs` file
/// - TypeScript: `package.json` present OR any `*.ts` / `*.tsx` file
/// - Python: `pyproject.toml` present OR any `*.py` file
/// - Go: `go.mod` present OR any `*.go` file
///
/// # Examples
///
/// ```text
/// // Calling from within the crate:
/// let langs = detect_languages(Path::new("/path/to/my-rust-project"));
/// // Returns [Language::Rust] when Cargo.toml is present in that dir.
/// ```
pub fn detect_languages(dir: &Path) -> Vec<Language> {
    let mut langs = std::collections::BTreeSet::new();

    // Walk the top-level directory only (avoid recursing deep into deps).
    // Recursion is limited to 2 levels to keep init fast.
    walk_for_langs(dir, 2, &mut langs);

    langs.into_iter().collect()
}

fn walk_for_langs(dir: &Path, depth: u32, langs: &mut std::collections::BTreeSet<Language>) {
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };

    for entry in entries.flatten() {
        let path = entry.path();
        let name = entry.file_name();
        let name = name.to_string_lossy();

        // Skip hidden dirs and common large dep dirs.
        if name.starts_with('.') || name == "node_modules" || name == "target" || name == ".git" {
            continue;
        }

        if path.is_file() {
            classify_file(&name, langs);
        } else if path.is_dir() && depth > 0 {
            walk_for_langs(&path, depth - 1, langs);
        }
    }
}

fn classify_file(name: &str, langs: &mut std::collections::BTreeSet<Language>) {
    if name == "Cargo.toml" || name.ends_with(".rs") {
        langs.insert(Language::Rust);
    }
    if name == "package.json" || name.ends_with(".ts") || name.ends_with(".tsx") {
        langs.insert(Language::TypeScript);
    }
    if name == "pyproject.toml" || name.ends_with(".py") {
        langs.insert(Language::Python);
    }
    if name == "go.mod" || name.ends_with(".go") {
        langs.insert(Language::Go);
    }
}

// ── ucil.toml serialisation ───────────────────────────────────────────────────

#[derive(Debug, Serialize)]
struct UcilConfig {
    project: ProjectSection,
}

#[derive(Debug, Serialize)]
struct ProjectSection {
    name: String,
    languages: Vec<String>,
    schema_version: String,
}

// ── Command runner ────────────────────────────────────────────────────────────

/// Run `ucil init`.
///
/// # Errors
///
/// Returns an error if the `.ucil/` directory or `ucil.toml` cannot be created.
pub async fn run(args: InitArgs) -> Result<()> {
    let dir = args.dir.canonicalize().unwrap_or(args.dir);

    // Determine project name from directory basename.
    let name = dir
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| "project".to_owned());

    // Detect languages.
    let languages = detect_languages(&dir);
    let lang_strings: Vec<String> = languages.iter().map(|l| l.to_string()).collect();

    // Create .ucil/ (idempotent).
    let ucil_dir = dir.join(".ucil");
    if !ucil_dir.exists() {
        fs::create_dir_all(&ucil_dir)
            .with_context(|| format!("failed to create {}", ucil_dir.display()))?;
    }

    // Write ucil.toml.
    let config = UcilConfig {
        project: ProjectSection {
            name: name.clone(),
            languages: lang_strings.clone(),
            schema_version: SCHEMA_VERSION.to_owned(),
        },
    };
    let toml_content = toml::to_string_pretty(&config).context("failed to serialise ucil.toml")?;
    let toml_path = ucil_dir.join("ucil.toml");
    fs::write(&toml_path, toml_content)
        .with_context(|| format!("failed to write {}", toml_path.display()))?;

    // Human-readable summary.
    let langs_display = if lang_strings.is_empty() {
        "none detected".to_owned()
    } else {
        lang_strings.join(", ")
    };
    println!("ucil init: created .ucil/ for {name} [{langs_display}]");

    Ok(())
}

// ── Unit tests ────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use tempfile::TempDir;

    use super::{detect_languages, Language};

    fn tmp() -> TempDir {
        TempDir::new().expect("temp dir")
    }

    #[test]
    fn detects_rust_from_cargo_toml() {
        let dir = tmp();
        std::fs::write(dir.path().join("Cargo.toml"), "[package]").unwrap();
        let langs = detect_languages(dir.path());
        assert!(langs.contains(&Language::Rust));
    }

    #[test]
    fn detects_python_from_pyproject() {
        let dir = tmp();
        std::fs::write(dir.path().join("pyproject.toml"), "[project]").unwrap();
        let langs = detect_languages(dir.path());
        assert!(langs.contains(&Language::Python));
    }

    #[test]
    fn detects_typescript_from_package_json() {
        let dir = tmp();
        std::fs::write(dir.path().join("package.json"), "{}").unwrap();
        let langs = detect_languages(dir.path());
        assert!(langs.contains(&Language::TypeScript));
    }

    #[test]
    fn detects_go_from_go_mod() {
        let dir = tmp();
        std::fs::write(dir.path().join("go.mod"), "module example.com/m\ngo 1.21\n").unwrap();
        let langs = detect_languages(dir.path());
        assert!(langs.contains(&Language::Go));
    }

    #[test]
    fn empty_dir_detects_nothing() {
        let dir = tmp();
        let langs = detect_languages(dir.path());
        assert!(langs.is_empty());
    }
}

mod tokenizer;
mod tree;

use std::{
    error::Error,
    fs,
    path::{Path, PathBuf},
};

#[derive(Default)]
pub(super) struct Counts {
    pub total: usize,
    pub passed: usize,
    pub shape_passed: usize,
    pub unsupported: usize,
}

impl Counts {
    fn report(&self, name: &str) {
        let percent = if self.total == 0 {
            0.0
        } else {
            self.passed as f64 / self.total as f64 * 100.0
        };
        println!(
            "{name}: {}/{} ({percent:.2}%); output-only {}; unsupported {}",
            self.passed, self.total, self.shape_passed, self.unsupported
        );
    }
}

pub fn run() -> Result<(), Box<dyn Error>> {
    let fixtures = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/html5lib");
    if !fixtures.join("tokenizer/test1.test").is_file() {
        return Err(
            "missing html5lib submodule; run git submodule update --init --recursive".into(),
        );
    }
    let (tokens, core) = tokenizer::run(&fixtures.join("tokenizer"))?;
    let (documents, fragments) = tree::run(&fixtures.join("tree-construction"))?;
    println!("html5lib revision 9329e64694e7835d0dcff9811e22856ef6ad16f9");
    tokens.report("Tokenizer");
    core.report("Core data/tag/attribute cases (regression gate)");
    documents.report("Tree construction (documents)");
    fragments.report("Tree construction (fragments)");
    println!(
        "Passes require output AND parse errors; character chunks 1, 7, 64 must agree with whole input."
    );
    println!(
        "Fragment contexts are not implemented (6.15): inputs exercise the current backend but are never credited as passes."
    );
    println!(
        "Unsupported tokenizer initial states and non-scalar inputs count as failures, not exclusions."
    );
    println!(
        "Core gate selects Data-state cases without '&', declarations, or expected comment/doctype tokens."
    );
    println!(
        "Exclusions: xmlViolation.test (optional XML infoset coercion); tree-construction/scripted/ (script execution, 10.6)."
    );
    if std::env::var_os("HTML5LIB_STRICT").is_some() {
        assert_eq!(tokens.passed, tokens.total, "tokenizer conformance");
        assert_eq!(documents.passed, documents.total, "document conformance");
        assert_eq!(fragments.passed, fragments.total, "fragment conformance");
    }
    Ok(())
}

pub(super) fn files(path: &Path, extension: &str) -> Result<Vec<PathBuf>, Box<dyn Error>> {
    let mut paths = fs::read_dir(path)?
        .map(|entry| entry.map(|entry| entry.path()))
        .collect::<Result<Vec<_>, _>>()?;
    paths.retain(|path| path.extension().is_some_and(|ext| ext == extension));
    paths.sort();
    if paths.is_empty() {
        return Err(format!("no {extension} fixtures in {}", path.display()).into());
    }
    Ok(paths)
}

pub(super) fn chunks(source: &str, size: usize) -> Vec<&str> {
    let mut starts = source
        .char_indices()
        .map(|(index, _)| index)
        .step_by(size)
        .collect::<Vec<_>>();
    starts.push(source.len());
    starts
        .windows(2)
        .map(|pair| &source[pair[0]..pair[1]])
        .collect()
}

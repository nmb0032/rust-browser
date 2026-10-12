//! Local WPT-format reftests. This is not wptrunner or a WPT conformance gate.
use std::{
    collections::{HashMap, HashSet},
    fs,
    io::{self, Read, Write},
    path::{Path, PathBuf},
};

use url::Url;

use crate::{
    dom::Document,
    engine::document_base_url,
    screenshot::{self, Options, Result, Screenshot},
};

const MAX_FILES: usize = 2048;
const MAX_GRAPH: usize = 128;
const MAX_SCREENSHOT_BYTES: usize = 128 * 1024 * 1024;
const MAX_EVIDENCE_BYTES: usize = 256 * 1024 * 1024;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Relation {
    Match,
    Mismatch,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Metrics {
    pub max_difference: u8,
    pub total_pixels: u32,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Fuzzy {
    pub max_difference: (u8, u8),
    pub total_pixels: (u32, u32),
}

impl Fuzzy {
    pub fn accepts(self, metrics: Metrics) -> bool {
        (self.max_difference.0..=self.max_difference.1).contains(&metrics.max_difference)
            && (self.total_pixels.0..=self.total_pixels.1).contains(&metrics.total_pixels)
    }

    /// WPT scalar values mean exactly that value, not an implicit 0..N range.
    pub fn parse(value: &str) -> Result<Self> {
        fn range(value: &str, limit: u32) -> Result<(u32, u32)> {
            let (low, high) = value
                .trim()
                .split_once('-')
                .unwrap_or((value.trim(), value.trim()));
            let low: u32 = low.trim().parse()?;
            let high: u32 = high.trim().parse()?;
            if low > high || high > limit {
                return Err(io::Error::other("invalid fuzzy range").into());
            }
            Ok((low, high))
        }
        let parts = value.split(';').collect::<Vec<_>>();
        if parts.len() != 2 {
            return Err(io::Error::other("fuzzy requires maxDifference and totalPixels").into());
        }
        let mut values = [None, None];
        let mut positional = 0;
        for part in parts {
            let (index, value) = if let Some((name, value)) = part.trim().split_once('=') {
                let index = match name.trim() {
                    "maxDifference" => 0,
                    "totalPixels" => 1,
                    _ => return Err(io::Error::other("unknown fuzzy parameter").into()),
                };
                (index, value)
            } else {
                while positional < 2 && values[positional].is_some() {
                    positional += 1;
                }
                let index = positional;
                positional += 1;
                (index, part)
            };
            if index >= 2 || values[index].is_some() {
                return Err(io::Error::other("duplicate fuzzy parameter").into());
            }
            values[index] = Some(range(value, if index == 0 { 255 } else { 4096 * 4096 })?);
        }
        let difference = values[0].ok_or_else(|| io::Error::other("missing maxDifference"))?;
        Ok(Self {
            max_difference: (difference.0 as u8, difference.1 as u8),
            total_pixels: values[1].ok_or_else(|| io::Error::other("missing totalPixels"))?,
        })
    }
}

/// Exact device-pixel comparison; one differing pixel counts once, regardless
/// of how many RGB channels differ. Alpha is required to be opaque/normalized.
pub fn compare(actual: &Screenshot, expected: &Screenshot) -> Result<(Metrics, Screenshot)> {
    actual.validate()?;
    expected.validate()?;
    if actual.color_profile != expected.color_profile {
        return Err(io::Error::other("reference color profile mismatch").into());
    }
    if (actual.width, actual.height) != (expected.width, expected.height) {
        return Err(io::Error::other(format!(
            "reference size mismatch: actual {}x{}, expected {}x{}",
            actual.width, actual.height, expected.width, expected.height
        ))
        .into());
    }
    let mut metrics = Metrics::default();
    let mut rgba = Vec::with_capacity(actual.rgba.len());
    for (a, b) in actual
        .rgba
        .as_chunks::<4>()
        .0
        .iter()
        .zip(expected.rgba.as_chunks::<4>().0)
    {
        if a[3] != 255 || b[3] != 255 {
            return Err(io::Error::other("comparison requires opaque normalized pixels").into());
        }
        let delta = [
            a[0].abs_diff(b[0]),
            a[1].abs_diff(b[1]),
            a[2].abs_diff(b[2]),
        ];
        let max = *delta.iter().max().unwrap();
        metrics.max_difference = metrics.max_difference.max(max);
        metrics.total_pixels += u32::from(max != 0);
        rgba.extend_from_slice(&[delta[0], delta[1], delta[2], 255]);
    }
    Ok((
        metrics,
        Screenshot {
            width: actual.width,
            height: actual.height,
            rgba,
            color_profile: actual.color_profile,
        },
    ))
}

#[derive(Clone, Debug)]
pub struct Reference {
    pub url: Url,
    pub relation: Relation,
    pub fuzzy: Fuzzy,
}

#[derive(Clone, Debug, Default)]
pub struct Metadata {
    pub references: Vec<Reference>,
}

pub fn metadata(document: &Document, url: &Url) -> Result<Metadata> {
    let base = document_base_url(document, url);
    let mut references = Vec::new();
    let mut fuzzy = HashMap::new();
    for id in document.preorder() {
        let Some(element) = document.node(id).and_then(|n| n.as_element()) else {
            continue;
        };
        if element.tag_name().eq_ignore_ascii_case("link")
            && let Some(rel) = element.attribute("rel")
        {
            let tokens = rel.split_ascii_whitespace().collect::<Vec<_>>();
            let matches = tokens.iter().any(|t| t.eq_ignore_ascii_case("match"));
            let mismatches = tokens.iter().any(|t| t.eq_ignore_ascii_case("mismatch"));
            if matches || mismatches {
                if matches && mismatches {
                    return Err(io::Error::other("ambiguous match/mismatch link").into());
                }
                let href = element
                    .attribute("href")
                    .filter(|href| !href.trim().is_empty())
                    .ok_or_else(|| io::Error::other("reftest link missing href"))?;
                if href.len() > 8192 {
                    return Err(io::Error::other("reference URL exceeds 8192 bytes").into());
                }
                references.push((
                    base.join(href)?,
                    if matches {
                        Relation::Match
                    } else {
                        Relation::Mismatch
                    },
                ));
            }
        }
        if element.tag_name().eq_ignore_ascii_case("meta")
            && element.attribute("name") == Some("fuzzy")
        {
            let value = element
                .attribute("content")
                .ok_or_else(|| io::Error::other("fuzzy missing content"))?;
            let (key, value) = if let Some((reference, value)) = value.rsplit_once(':') {
                (Some(base.join(reference.trim())?), value)
            } else {
                (None, value)
            };
            if fuzzy.insert(key, Fuzzy::parse(value)?).is_some() {
                return Err(io::Error::other("duplicate fuzzy metadata").into());
            }
        }
    }
    if references.len() > MAX_GRAPH
        || fuzzy
            .keys()
            .flatten()
            .any(|url| !references.iter().any(|(r, _)| r == url))
    {
        return Err(
            io::Error::other("too many references or fuzzy targets a missing reference").into(),
        );
    }
    Ok(Metadata {
        references: references
            .into_iter()
            .map(|(url, relation)| Reference {
                fuzzy: fuzzy
                    .get(&Some(url.clone()))
                    .or_else(|| fuzzy.get(&None))
                    .copied()
                    .unwrap_or_default(),
                url,
                relation,
            })
            .collect(),
    })
}

fn local_path(url: &Url, root: &Path) -> Result<PathBuf> {
    let policy = crate::engine::resource_policy::ResourcePolicy::Local(root.to_owned());
    policy.check(url)?;
    Ok(url
        .to_file_path()
        .map_err(|_| io::Error::other("invalid local URL"))?
        .canonicalize()?)
}

fn read_metadata(url: &Url, root: &Path) -> Result<Metadata> {
    let path = local_path(url, root)?;
    let mut bytes = Vec::new();
    fs::File::open(path)?
        .take(crate::net::MAX_HTML_BYTES + 1)
        .read_to_end(&mut bytes)?;
    if bytes.len() as u64 > crate::net::MAX_HTML_BYTES {
        return Err(io::Error::other("reftest HTML exceeds 16 MiB").into());
    }
    let mut parser = crate::html::encoding::HtmlByteParser::new(Some("text/html"));
    parser
        .feed(&bytes)
        .map_err(|e| io::Error::other(format!("HTML parse failed: {e:?}")))?;
    let document = parser
        .finish()
        .map_err(|e| io::Error::other(format!("HTML parse failed: {e:?}")))?
        .parsed
        .document;
    metadata(&document, url)
}

pub fn discover(suite: &Path, resource_root: &Path) -> Result<Vec<PathBuf>> {
    fn walk(path: &Path, files: &mut Vec<PathBuf>, count: &mut usize, depth: usize) -> Result<()> {
        if depth > 16 {
            return Err(io::Error::other("reftest directory depth exceeds 16").into());
        }
        for entry in fs::read_dir(path)? {
            let entry = entry?;
            let kind = entry.file_type()?;
            *count += 1;
            if *count > MAX_FILES || kind.is_symlink() {
                return Err(io::Error::other(
                    "reftest enumeration exceeds 2048 entries or contains symlinks",
                )
                .into());
            }
            if kind.is_dir() {
                walk(&entry.path(), files, count, depth + 1)?;
            } else if kind.is_file() && is_html(&entry.path()) {
                files.push(entry.path());
            }
        }
        Ok(())
    }
    let root = resource_root.canonicalize()?;
    let suite = suite.canonicalize()?;
    if !suite.starts_with(&root) {
        return Err(io::Error::other("suite escapes resource root").into());
    }
    let mut files = Vec::new();
    walk(&suite, &mut files, &mut 0, 0)?;
    files.sort();
    let mut tests = Vec::new();
    for file in files {
        let url = Url::from_file_path(&file).map_err(|_| io::Error::other("invalid test path"))?;
        if !read_metadata(&url, &root)?.references.is_empty() {
            tests.push(file);
        }
    }
    if tests.is_empty() {
        return Err(io::Error::other("no reftests discovered (no rel=match/mismatch)").into());
    }
    Ok(tests)
}

fn is_html(path: &Path) -> bool {
    path.extension()
        .and_then(|s| s.to_str())
        .is_some_and(|s| s.eq_ignore_ascii_case("html") || s.eq_ignore_ascii_case("htm"))
}

#[derive(Debug)]
pub struct Outcome {
    pub path: PathBuf,
    pub passed: bool,
    pub error: Option<String>,
    pub comparisons: Vec<String>,
}

#[derive(Debug)]
pub struct Report {
    pub outcomes: Vec<Outcome>,
    pub manifest: PathBuf,
}

impl Report {
    pub fn passed(&self) -> bool {
        self.outcomes.iter().all(|test| test.passed)
    }
}

struct Runner<'a> {
    root: &'a Path,
    options: &'a Options,
    artifacts: PathBuf,
    pages: HashMap<Url, (Metadata, Screenshot)>,
    active: HashSet<Url>,
    completed: HashMap<Url, bool>,
    bytes: usize,
    comparisons: Vec<String>,
    evidence_bytes: usize,
    comparison_count: usize,
}

impl Runner<'_> {
    fn page(&mut self, url: &Url) -> Result<()> {
        if self.pages.contains_key(url) {
            return Ok(());
        }
        let path = local_path(url, self.root)?;
        if self.pages.len() >= MAX_GRAPH {
            return Err(io::Error::other("reference graph exceeds 128 pages").into());
        }
        let (metadata, image) = if path
            .extension()
            .is_some_and(|s| s.eq_ignore_ascii_case("png"))
        {
            (
                Metadata::default(),
                Screenshot::read_png_profile(&path, self.options.color_profile)?,
            )
        } else if is_html(&path) {
            let mut frame = screenshot::load_url(url, self.root, self.options)?;
            (
                metadata(&frame.document, url)?,
                screenshot::render(&mut frame, self.options)?,
            )
        } else {
            return Err(io::Error::other("references must be local HTML or PNG").into());
        };
        self.bytes += image.rgba.len();
        if self.bytes > MAX_SCREENSHOT_BYTES {
            return Err(io::Error::other("reference screenshots exceed 128 MiB").into());
        }
        self.pages.insert(url.clone(), (metadata, image));
        Ok(())
    }

    fn evaluate(&mut self, url: &Url) -> Result<bool> {
        if self.active.contains(url) {
            return Err(io::Error::other(format!("reference cycle: {url}")).into());
        }
        if let Some(result) = self.completed.get(url) {
            return Ok(*result);
        }
        self.page(url)?;
        self.active.insert(url.clone());
        let references = self.pages[url].0.references.clone();
        let mut has_match = false;
        let mut match_pass = false;
        let mut mismatch_pass = true;
        for reference in references {
            self.comparison_count += 1;
            if self.comparison_count > 256 {
                return Err(io::Error::other("reference graph exceeds 256 comparisons").into());
            }
            let child_valid = self.evaluate(&reference.url)?;
            let actual = &self.pages[url].1;
            let expected = &self.pages[&reference.url].1;
            self.evidence_bytes += actual.rgba.len() + expected.rgba.len() + actual.rgba.len();
            if self.evidence_bytes > MAX_EVIDENCE_BYTES {
                return Err(
                    io::Error::other("reference evidence exceeds 256 MiB decoded size").into(),
                );
            }
            let comparison = compare(actual, expected);
            let index = self.comparisons.len();
            let stem = self.artifacts.join(format!("comparison-{index:03}"));
            let (passed, details, diff, error) = match comparison {
                Ok((metrics, diff)) => {
                    let equal = reference.fuzzy.accepts(metrics);
                    let passed = child_valid
                        && match reference.relation {
                            Relation::Match => equal,
                            Relation::Mismatch => !equal,
                        };
                    (
                        passed,
                        format!(
                            "maxDifference={} totalPixels={} fuzzy={:?}",
                            metrics.max_difference, metrics.total_pixels, reference.fuzzy
                        ),
                        Some(diff),
                        None,
                    )
                }
                Err(error) => (false, error.to_string(), None, Some(error)),
            };
            // Keep evidence for every comparison: an OR alternative may fail
            // while the test passes; a failed aggregate still has all its edges.
            actual.save(&stem.with_extension("actual.png"))?;
            expected.save(&stem.with_extension("expected.png"))?;
            if let Some(diff) = diff {
                diff.save(&stem.with_extension("diff.png"))?;
            } else {
                let path = stem.with_extension("diff.png");
                if fs::symlink_metadata(&path).is_ok_and(|m| m.file_type().is_symlink()) {
                    return Err(io::Error::other("diff artifact is a symlink").into());
                }
                if path.is_file() {
                    fs::remove_file(path)?;
                }
            }
            self.comparisons.push(format!(
                "{} {:?} {} => {} {details}; evidence={}",
                url,
                reference.relation,
                reference.url,
                if passed { "PASS" } else { "FAIL" },
                stem.display()
            ));
            if let Some(error) = error {
                return Err(error);
            }
            match reference.relation {
                Relation::Match => {
                    has_match = true;
                    match_pass |= passed;
                }
                Relation::Mismatch => mismatch_pass &= passed,
            }
        }
        let passed = (!has_match || match_pass) && mismatch_pass;
        self.active.remove(url);
        self.completed.insert(url.clone(), passed);
        Ok(passed)
    }
}

fn resolve_destination(path: &Path) -> Result<PathBuf> {
    match path.canonicalize() {
        Ok(path) => Ok(path),
        Err(error) if error.kind() == io::ErrorKind::NotFound => {
            let parent = path
                .parent()
                .filter(|p| !p.as_os_str().is_empty())
                .unwrap_or(Path::new("."));
            Ok(resolve_destination(parent)?.join(
                path.file_name()
                    .ok_or_else(|| io::Error::other("invalid artifact path"))?,
            ))
        }
        Err(error) => Err(error.into()),
    }
}

/// Artifacts must be a dedicated resolved directory outside the suite. Existing
/// unrelated files are never deleted; each test uses its stable sorted index.
pub fn run(
    suite: &Path,
    resource_root: &Path,
    artifacts: &Path,
    options: &Options,
) -> Result<Report> {
    options.dimensions()?;
    let suite = suite.canonicalize()?;
    let root = resource_root.canonicalize()?;
    let tests = discover(&suite, &root)?;
    if fs::symlink_metadata(artifacts).is_ok_and(|m| m.file_type().is_symlink()) {
        return Err(io::Error::other("artifact directory is a symlink").into());
    }
    let resolved = resolve_destination(artifacts)?;
    if resolved.starts_with(&suite)
        || suite.starts_with(&resolved)
        || resolved == root
        || (resolved.starts_with(&root) && !resolved.starts_with(root.join("target")))
        || fs::symlink_metadata(&resolved).is_ok_and(|m| m.file_type().is_symlink())
    {
        return Err(io::Error::other(
            "artifact directory must be outside the suite, not a root or symlink",
        )
        .into());
    }
    fs::create_dir_all(&resolved)?;
    let resolved = resolved.canonicalize()?;
    let manifest = resolved.join("manifest.txt");
    // Refuse artifact symlinks rather than following them on write.
    if fs::symlink_metadata(&manifest).is_ok_and(|m| m.file_type().is_symlink()) {
        return Err(io::Error::other("manifest is a symlink").into());
    }
    let mut output = fs::File::create(&manifest)?;
    writeln!(
        output,
        "native-reftest-v1\nsuite={}\nresource-root={}\noptions={options:?}\nfont=bundled Noto Sans + document URL fonts; sRGB RGBA8; exact unless explicit WPT fuzzy",
        suite.display(),
        root.display()
    )?;
    let mut outcomes = Vec::new();
    for (index, path) in tests.into_iter().enumerate() {
        let directory = resolved.join(format!("test-{index:03}"));
        if fs::symlink_metadata(&directory).is_ok_and(|m| m.file_type().is_symlink()) {
            return Err(io::Error::other("test artifact directory is a symlink").into());
        }
        fs::create_dir_all(&directory)?;
        let mut runner = Runner {
            root: &root,
            options,
            artifacts: directory,
            pages: HashMap::new(),
            active: HashSet::new(),
            completed: HashMap::new(),
            bytes: 0,
            comparisons: Vec::new(),
            evidence_bytes: 0,
            comparison_count: 0,
        };
        let url = Url::from_file_path(&path).map_err(|_| io::Error::other("invalid test path"))?;
        let result = runner.evaluate(&url);
        let error = result.as_ref().err().map(ToString::to_string);
        let error = if let Some(error) = &error
            && let Some((_, actual)) = runner.pages.get(&url)
        {
            let path = runner.artifacts.join("actual.png");
            actual.save(&path)?;
            Some(format!("{error}; actual={}", path.display()))
        } else {
            error
        };
        let outcome = Outcome {
            path,
            passed: result.as_ref().is_ok_and(|passed| *passed),
            error,
            comparisons: runner.comparisons,
        };
        writeln!(
            output,
            "{} {} {}",
            if outcome.passed { "PASS" } else { "FAIL" },
            outcome.path.display(),
            outcome.error.as_deref().unwrap_or("")
        )?;
        for comparison in &outcome.comparisons {
            writeln!(output, "  {comparison}")?;
        }
        outcomes.push(outcome);
    }
    Ok(Report { outcomes, manifest })
}

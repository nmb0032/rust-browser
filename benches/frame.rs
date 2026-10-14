//! Offline frame benchmark for UI responsiveness.
//!
//! Runs the same pipeline as the window event loop (`render_for_window_size`
//! + rasterization) at 1280x800 @2x on:
//! - deterministic synthetic pages shaped like the profiled sites
//!   (`synthetic-wiki`: ~19k nodes, hundreds of `:root` custom properties;
//!   `synthetic-hn`: ~1.3k-node table layout), generated into `target/`;
//! - any real captures in `tests/fixtures/perf/<name>/index.html`
//!   (see `tools/capture_perf_fixture.py`; captures are not committed).
//!
//! `cargo bench --bench frame [-- <name-filter>]`

use std::{
    error::Error,
    fmt::Write as _,
    fs,
    hint::black_box,
    path::{Path, PathBuf},
    time::{Duration, Instant},
};

use rust_browser::{
    css::selectors::InteractionState, dom::NodeId, engine::Frame, layout::ScrollOffset, paint,
    screenshot, text::FontContext, ui::window::render_for_window_size,
};
use winit::dpi::PhysicalSize;

const WIDTH: u32 = 1280;
const HEIGHT: u32 = 800;
const SCALE: f64 = 2.0;
/// Stop sampling a scenario once this much time has been spent on it.
const BUDGET: Duration = Duration::from_secs(20);
const MAX_SAMPLES: usize = 7;

fn main() -> Result<(), Box<dyn Error>> {
    let filter = std::env::args()
        .skip(1)
        .find(|arg| !arg.starts_with('-'))
        .unwrap_or_default();
    let font = FontContext::system();
    let profile = if cfg!(debug_assertions) {
        "Debug"
    } else {
        "Release"
    };
    println!(
        "{profile} frame benchmark, {WIDTH}x{HEIGHT} @{SCALE}x, system fonts; median of up to {MAX_SAMPLES} samples (budget {}s/scenario).",
        BUDGET.as_secs()
    );
    println!(
        "full = restyle-free layout+display list from a fresh snapshot; hover = interaction restyle + render; scroll = display list + raster at a new offset;\nhit test/wheel = UI-thread input handling on the retained layout.\n"
    );

    let mut pages = synthetic_pages()?;
    pages.extend(captured_pages());
    pages.retain(|(name, _)| name.contains(&filter));

    for (name, path) in pages {
        let root = path.parent().expect("page directory");
        let options = screenshot::Options {
            width: WIDTH,
            height: HEIGHT,
            scale: SCALE,
            ..Default::default()
        };
        let started = Instant::now();
        let frame = match screenshot::load(&path, root, &options) {
            Ok(frame) => frame,
            Err(error) => {
                println!("{name}: load failed: {error}\n");
                continue;
            }
        };
        let load = started.elapsed();
        report_page(&name, frame, load, &font);
    }
    Ok(())
}

fn report_page(name: &str, frame: Frame, load: Duration, font: &FontContext) {
    let size = PhysicalSize::new(
        (f64::from(WIDTH) * SCALE) as u32,
        (f64::from(HEIGHT) * SCALE) as u32,
    );
    let nodes = frame.document.node_count();
    println!("== {name}: {nodes} DOM nodes, load {}", ms(load));

    let clone = sample(|| black_box(frame.clone()));
    println!("  frame clone            {}", clone.show());

    let mut stages = None;
    let full = sample(|| {
        let mut fresh = frame.clone();
        let started = Instant::now();
        let rendered = render_for_window_size(&mut fresh, size, SCALE, font, false, 0.0);
        let elapsed = started.elapsed();
        stages = Some(fresh.stage_timings());
        black_box(rendered);
        elapsed
    });
    println!("  full render            {}", full.show());
    if let Some(stages) = stages {
        println!("    stages: {}", stages.summary());
    }

    let mut warm = frame.clone();
    let (tree, _, _) = render_for_window_size(&mut warm, size, SCALE, font, false, 0.0);

    for (label, target) in [
        ("hover (no state rule)", hover_target(&warm, false)),
        ("hover (matching rule)", hover_target(&warm, true)),
    ] {
        let Some(target) = target else {
            println!("  {label:<22} n/a");
            continue;
        };
        let mut restyle = Duration::ZERO;
        let hover = sample(|| {
            let mut fresh = warm.clone();
            let started = Instant::now();
            let changed = fresh.set_interaction_state(InteractionState {
                hovered: Some(target),
                ..fresh.interaction_state().clone()
            });
            restyle = started.elapsed();
            // Like the UI: an unaffected state change keeps the current paint.
            if changed {
                black_box(render_for_window_size(
                    &mut fresh, size, SCALE, font, false, 0.0,
                ));
            }
            started.elapsed()
        });
        println!("  {label:<22} {} (restyle {})", hover.show(), ms(restyle));
    }

    let mut scrolled = tree;
    let logical = (WIDTH as f32, HEIGHT as f32);
    let mut pixels = vec![0u32; (size.width * size.height) as usize];
    let mut offset = 0.0;
    let scroll = sample(|| {
        offset += 120.0;
        scrolled.set_viewport_scroll(ScrollOffset { x: 0.0, y: offset });
        let started = Instant::now();
        let list = paint::build_display_list(&scrolled, logical.0, logical.1, false);
        pixels.fill(0x00FF_FFFF);
        paint::rasterize_display_list(
            &list,
            font,
            &mut pixels,
            size.width as usize,
            size.height as usize,
            SCALE,
            scrolled.viewport_scroll().y,
        );
        started.elapsed()
    });
    println!("  scroll repaint         {}", scroll.show());

    // UI-thread input: pointer hit test (hover/cursor) and a wheel step.
    let (cx, cy) = (WIDTH as f32 * 0.3, HEIGHT as f32 * 0.4);
    let hit = sample(|| {
        let started = Instant::now();
        black_box(scrolled.hit_test_viewport(cx, cy, scrolled.viewport_scroll().y));
        started.elapsed()
    });
    println!("  pointer hit test       {}", hit.show());
    let mut direction = 1.0;
    let wheel = sample(|| {
        direction = -direction;
        let started = Instant::now();
        black_box(scrolled.scroll_wheel(
            cx,
            cy,
            ScrollOffset {
                x: 0.0,
                y: 120.0 * direction,
            },
        ));
        started.elapsed()
    });
    println!("  wheel step             {}\n", wheel.show());
}

/// An early in-content element for hover: an `<a>` (sites style `a:hover`)
/// or a `<p>`/`<span>` (typically no state rule).
fn hover_target(frame: &Frame, link: bool) -> Option<NodeId> {
    let tags: &[&str] = if link { &["a"] } else { &["p", "span"] };
    tags.iter().find_map(|tag| {
        frame
            .document
            .preorder()
            .filter(|&id| {
                frame
                    .document
                    .node(id)
                    .and_then(|node| node.as_element())
                    .is_some_and(|element| element.tag_name().eq_ignore_ascii_case(tag))
            })
            .nth(3)
    })
}

struct Samples(Vec<Duration>);

impl Samples {
    fn show(&self) -> String {
        let mut sorted = self.0.clone();
        sorted.sort();
        format!(
            "{:>10} (min {}, n={})",
            ms(sorted[sorted.len() / 2]),
            ms(sorted[0]),
            sorted.len()
        )
    }
}

trait IntoElapsed {
    fn elapsed(self, started: Instant) -> Duration;
}

impl IntoElapsed for Duration {
    fn elapsed(self, _: Instant) -> Duration {
        self
    }
}

impl IntoElapsed for Frame {
    fn elapsed(self, started: Instant) -> Duration {
        let elapsed = started.elapsed();
        drop(self);
        elapsed
    }
}

fn sample<T: IntoElapsed>(mut run: impl FnMut() -> T) -> Samples {
    let begun = Instant::now();
    let mut samples = Vec::new();
    while samples.len() < MAX_SAMPLES && (samples.is_empty() || begun.elapsed() < BUDGET) {
        let started = Instant::now();
        samples.push(run().elapsed(started));
    }
    Samples(samples)
}

fn ms(duration: Duration) -> String {
    format!("{:.1}ms", duration.as_secs_f64() * 1e3)
}

fn captured_pages() -> Vec<(String, PathBuf)> {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/perf");
    let Ok(entries) = fs::read_dir(root) else {
        return Vec::new();
    };
    let mut pages: Vec<_> = entries
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .filter(|dir| dir.join("index.html").is_file())
        .map(|dir| {
            (
                dir.file_name().unwrap().to_string_lossy().into_owned(),
                dir.join("index.html"),
            )
        })
        .collect();
    pages.sort();
    pages
}

fn synthetic_pages() -> Result<Vec<(String, PathBuf)>, Box<dyn Error>> {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("target/perf-fixtures");
    let mut pages = Vec::new();
    for (name, source) in [
        ("synthetic-hn", synthetic_hn()),
        ("synthetic-wiki", synthetic_wiki()),
    ] {
        let dir = root.join(name);
        fs::create_dir_all(&dir)?;
        let path = dir.join("index.html");
        fs::write(&path, source)?;
        pages.push((name.to_string(), path));
    }
    Ok(pages)
}

/// Deterministic xorshift so generated pages are identical across runs.
struct Rng(u64);

impl Rng {
    fn next(&mut self, bound: usize) -> usize {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        (self.0 % bound as u64) as usize
    }

    fn words(&mut self, count: usize) -> String {
        const WORDS: &[&str] = &[
            "memory",
            "safety",
            "ownership",
            "borrow",
            "checker",
            "compiler",
            "the",
            "of",
            "and",
            "language",
            "concurrency",
            "systems",
            "performance",
            "a",
            "is",
            "with",
            "zero-cost",
            "abstractions",
            "traits",
            "generics",
            "lifetimes",
            "crate",
            "in",
            "package",
            "manager",
            "community",
            "release",
            "stable",
            "macro",
            "pattern",
        ];
        let mut out = String::new();
        for index in 0..count {
            if index > 0 {
                out.push(' ');
            }
            out.push_str(WORDS[self.next(WORDS.len())]);
        }
        out
    }
}

/// ~19k nodes; large `:root` custom-property set referenced throughout,
/// link-dense paragraphs, infobox tables, lists, a navigation sidebar.
fn synthetic_wiki() -> String {
    let mut rng = Rng(0x9E37_79B9_7F4A_7C15);
    let mut css = String::from(":root{");
    for index in 0..400 {
        if index % 4 == 3 {
            let _ = write!(css, "--wiki-v{index}:var(--wiki-v{});", index - 3);
        } else {
            let _ = write!(
                css,
                "--wiki-v{index}:#{:06x};",
                (index * 2_654_435_761usize) & 0xFF_FFFF
            );
        }
    }
    css.push_str("--link:#36c;--link-hover:#447ff5;--gap:0.5em}\n");
    css.push_str(
        "body{margin:0;font-family:sans-serif;font-size:14px;line-height:1.6;color:var(--wiki-v0)}\
         .page{display:flex}.sidebar{width:176px;flex:none;padding:var(--gap)}\
         .content{flex:1;min-width:0;padding:1em 2em}\
         a{color:var(--link);text-decoration:none}a:hover{text-decoration:underline;color:var(--link-hover)}\
         .sidebar li:hover>a{background:var(--wiki-v5)}\
         h2{border-bottom:1px solid var(--wiki-v9);font-family:serif;font-weight:normal}\
         .infobox{float:right;width:22em;margin:0 0 1em 1em;border:1px solid var(--wiki-v13);border-collapse:collapse}\
         .infobox th,.infobox td{padding:2px 4px;text-align:left;vertical-align:top}\
         sup.reference{font-size:0.75em;line-height:1}\
         .hatnote{font-style:italic;padding-left:1.6em}\n",
    );
    for index in 0..1200 {
        let _ = writeln!(
            css,
            ".content .c{} .k{}{{color:var(--wiki-v{});margin-left:{}px}}",
            index % 97,
            index,
            index % 400,
            index % 7
        );
    }

    let mut html = format!(
        "<!doctype html><html><head><title>Synthetic wiki</title><style>{css}</style></head><body><div class=page><nav class=sidebar><ul>"
    );
    for index in 0..160 {
        let _ = write!(html, "<li><a href='#n{index}'>{}</a></li>", rng.words(2));
    }
    html.push_str("</ul></nav><main class=content><h1>Synthetic article</h1>");
    for section in 0..70 {
        let _ = write!(
            html,
            "<h2 id=s{section}><span>{}</span></h2><div class=hatnote>{} <a href='#x'>{}</a></div>",
            rng.words(3),
            rng.words(6),
            rng.words(2)
        );
        if section % 4 == 0 {
            html.push_str("<table class=infobox><tbody>");
            for _ in 0..20 {
                let _ = write!(
                    html,
                    "<tr><th>{}</th><td><a href='#i'>{}</a>, {}</td></tr>",
                    rng.words(2),
                    rng.words(1),
                    rng.words(2)
                );
            }
            html.push_str("</tbody></table>");
        }
        for paragraph in 0..6 {
            let _ = write!(html, "<p class='c{}'>", (section * 6 + paragraph) % 97);
            for inline in 0..8 {
                let words = rng.words(4);
                let text = rng.words(1);
                match (inline + paragraph) % 5 {
                    0 => {
                        let _ = write!(
                            html,
                            "{words} <a href='#w' class=k{}>{text}</a> ",
                            rng.next(1200)
                        );
                    }
                    1 => {
                        let _ = write!(html, "{words} <b>{text}</b> ");
                    }
                    2 => {
                        let _ = write!(html, "{words} <i><a href='#v'>{text}</a></i> ");
                    }
                    3 => {
                        let _ = write!(
                            html,
                            "{words}<sup class=reference><a href='#r'>[{}]</a></sup> ",
                            rng.next(300)
                        );
                    }
                    _ => {
                        let _ = write!(
                            html,
                            "{words} <span class=k{}>{text}</span> ",
                            rng.next(1200)
                        );
                    }
                }
            }
            html.push_str("</p>");
        }
        html.push_str("<ul>");
        for _ in 0..10 {
            let _ = write!(
                html,
                "<li><a href='#l'>{}</a> {}</li>",
                rng.words(2),
                rng.words(5)
            );
        }
        html.push_str("</ul>");
    }
    html.push_str("</main></div></body></html>");
    html
}

/// ~1.3k nodes; table layout with 30 stories, `a:hover`-free like HN but
/// with link color rules.
fn synthetic_hn() -> String {
    let mut rng = Rng(0xD1B5_4A32_D192_ED03);
    let css = "body{font-family:Verdana,Geneva,sans-serif;font-size:10pt;color:#828282}\
               td{font-family:Verdana,Geneva,sans-serif;font-size:10pt;color:#828282}\
               a:link{color:#000;text-decoration:none}a:visited{color:#828282;text-decoration:none}\
               .title{font-size:10pt;color:#828282}.titleline>a{color:#000}\
               .subtext{font-size:7pt}.subtext a:link{color:#828282}.subtext a:hover{text-decoration:underline}\
               .hnname a:link{color:#000}.pagetop{font-size:10pt;line-height:12px;color:#222}\
               .votearrow{width:10px;height:10px;border:0;margin:3px 2px 6px;background:#ccc}\
               .rank{color:#828282}.spacer{height:5px}#hnmain{background:#f6f6ef;width:85%}";
    let mut html = format!(
        "<!doctype html><html><head><title>Synthetic HN</title><style>{css}</style></head><body><center><table id=hnmain border=0 cellpadding=0 cellspacing=0><tr><td bgcolor=#ff6600><table border=0 cellpadding=0 cellspacing=0 width=100%><tr><td class=pagetop><b class=hnname><a href=news>Hacker News</a></b> <a href=new>new</a> | <a href=past>past</a> | <a href=ask>ask</a></td></tr></table></td></tr><tr class=spacer><td></td></tr><tr><td><table border=0 cellpadding=0 cellspacing=0>"
    );
    for story in 1..=30 {
        let _ = write!(
            html,
            "<tr class=athing><td align=right valign=top class=title><span class=rank>{story}.</span></td>\
             <td valign=top class=votelinks><center><a href='vote?id={story}'><div class=votearrow title=upvote></div></a></center></td>\
             <td class=title><span class=titleline><a href='https://example.com/{story}'>{}</a><span class=sitebit> (<a href=from><span class=sitestr>example.com</span></a>)</span></span></td></tr>\
             <tr><td colspan=2></td><td class=subtext><span class=subline><span class=score>{} points</span> by <a href=user class=hnuser>{}</a> <span class=age><a href=item>{} hours ago</a></span> | <a href=hide>hide</a> | <a href=item>{} comments</a></span></td></tr>\
             <tr class=spacer style='height:5px'></tr>",
            rng.words(8),
            rng.next(900),
            rng.words(1),
            rng.next(24),
            rng.next(400)
        );
    }
    html.push_str("</table></td></tr></table></center></body></html>");
    html
}

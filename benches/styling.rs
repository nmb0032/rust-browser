use std::{
    error::Error,
    fmt::Write,
    hint::black_box,
    time::{Duration, Instant},
};

use rust_browser::{css, html::parser, layout, style};

const SAMPLES: usize = 9;

fn main() -> Result<(), Box<dyn Error>> {
    let font = rust_browser::text::FontContext::bundled();
    let profile = if cfg!(debug_assertions) {
        "Debug-mode"
    } else {
        "Release-mode"
    };
    println!("{profile} medians of {SAMPLES} samples, no networking or rasterization.");
    println!(
        "Style resolution includes sheet collection, CSS/selector/value parsing, matching, and cascade."
    );
    println!("HTML parsing and font loading are outside the timed regions.\n");
    println!("Elements / rules       CSS parsing    Style resolution    Styled box layout");
    for (elements, rules) in [(100, 10), (1000, 100)] {
        let mut css_source = String::new();
        for index in 0..rules {
            writeln!(
                css_source,
                ".wrapper p.c{index} {{color:green; padding:0.1em; margin:1px; width:80%}}"
            )?;
        }
        let mut source = format!("<style>{css_source}</style><div class='wrapper'>");
        for index in 0..elements {
            write!(
                source,
                "<p class='c{}'>Styled words for layout.</p>",
                index % rules
            )?;
        }
        source.push_str("</div>");
        let document = parser::parse(&source).expect("benchmark HTML must parse");
        let resolved = style::resolve(&document);
        assert!(resolved.stylesheets.diagnostics.is_empty());
        let mut reference = layout::build_layout_tree_with_styles(&document, &resolved.styles);
        reference.layout(800.0, &font);
        let wrapper_id = document
            .preorder()
            .find(|&node| {
                document
                    .node(node)
                    .unwrap()
                    .as_element()
                    .is_some_and(|element| element.tag_name() == "div")
            })
            .expect("benchmark wrapper");
        let wrapper = find_box(reference.root(), wrapper_id).expect("wrapper layout box");
        let paragraph = &wrapper.children()[0];
        assert_eq!(paragraph.style().color, Some(0x008000));
        assert!(
            (paragraph.content_rect().width - wrapper.content_rect().width * 0.8).abs() < 0.001
        );
        let mut css_times = Vec::new();
        let mut style_times = Vec::new();
        let mut layout_times = Vec::new();
        for sample in 0..SAMPLES {
            // Alternate stage order to reduce warm-cache/order bias.
            for stage in if sample % 2 == 0 {
                [0, 1, 2]
            } else {
                [2, 1, 0]
            } {
                match stage {
                    0 => css_times.push(time(|| css::parse(black_box(&css_source)).unwrap())),
                    1 => style_times.push(time(|| style::resolve(black_box(&document)))),
                    _ => layout_times.push(time(|| {
                        let mut tree = layout::build_layout_tree_with_styles(
                            black_box(&document),
                            black_box(&resolved.styles),
                        );
                        tree.layout(800.0, black_box(&font));
                        tree
                    })),
                }
            }
        }
        println!(
            "{elements:>7} / {rules:<5}    {:>9.3} ms {:>15.3} ms {:>16.3} ms",
            millis(median(css_times)),
            millis(median(style_times)),
            millis(median(layout_times))
        );
    }
    #[cfg(feature = "style-bench-reference")]
    matching_comparison()?;
    #[cfg(not(feature = "style-bench-reference"))]
    println!(
        "\nFull matching comparison: cargo bench --bench styling --features style-bench-reference"
    );
    Ok(())
}

#[cfg(feature = "style-bench-reference")]
fn matching_comparison() -> Result<(), Box<dyn Error>> {
    use style::{BenchmarkMatching, resolve_for_benchmark};

    const BRANCHES: usize = 400;
    const PER_BRANCH: usize = 20;
    const RULES: usize = 500;
    println!(
        "\nMatching comparison: {} paragraphs + {BRANCHES} branches, {RULES} author rules.",
        BRANCHES * PER_BRANCH
    );
    println!("Same DOM/CSS per mode; full resolution, including collection and compilation.");
    println!("Rotating timed mode order; all computed styles compared outside timing.");
    println!("Rightmost keys      Unindexed      Index only     Index + Bloom   Total speedup");
    for selective in [true, false] {
        let mut source = String::from("<!doctype html><style>");
        for rule in 0..RULES {
            if selective {
                write!(source, ".branch p.c{rule}")?;
            } else {
                write!(source, ".branch.b{rule} p")?;
            }
            write!(
                source,
                " {{color:green; padding:0.1em; margin:1px; width:80%}}"
            )?;
        }
        source.push_str("</style><main>");
        for branch in 0..BRANCHES {
            write!(source, "<section class='branch b{branch}'>")?;
            for child in 0..PER_BRANCH {
                write!(
                    source,
                    "<p class='c{}'>Styled words.</p>",
                    (branch * PER_BRANCH + child) % RULES
                )?;
            }
            source.push_str("</section>");
        }
        source.push_str("</main>");
        let document = parser::parse(&source).expect("matching benchmark HTML must parse");
        let reference = resolve_for_benchmark(&document, BenchmarkMatching::Unindexed);
        assert!(reference.stylesheets.diagnostics.is_empty());
        for mode in [
            BenchmarkMatching::Indexed,
            BenchmarkMatching::IndexedWithBloom,
        ] {
            let result = resolve_for_benchmark(&document, mode);
            assert!(result.stylesheets.diagnostics.is_empty());
            for node in document.preorder() {
                assert_eq!(result.styles.get(node), reference.styles.get(node));
            }
        }
        let mut times: [Vec<Duration>; 3] = std::array::from_fn(|_| Vec::with_capacity(SAMPLES));
        for sample in 0..SAMPLES {
            for offset in 0..3 {
                let mode = (sample + offset) % 3;
                times[mode].push(time(|| match mode {
                    0 => resolve_for_benchmark(black_box(&document), BenchmarkMatching::Unindexed),
                    1 => resolve_for_benchmark(black_box(&document), BenchmarkMatching::Indexed),
                    _ => style::resolve(black_box(&document)),
                }));
            }
        }
        let [baseline, indexed, optimized] = times.map(median).map(millis);
        let label = if selective {
            "Class-selective"
        } else {
            "Shared tag / ancestors"
        };
        println!(
            "{label:<22} {baseline:>9.3} ms {indexed:>11.3} ms {optimized:>13.3} ms {:>10.2}x",
            baseline / optimized,
        );
    }
    Ok(())
}

fn time<T>(operation: impl FnOnce() -> T) -> Duration {
    let start = Instant::now();
    let result = operation();
    black_box(&result);
    start.elapsed()
}

fn find_box(root: &layout::LayoutBox, id: rust_browser::dom::NodeId) -> Option<&layout::LayoutBox> {
    let mut pending = vec![root];
    while let Some(node) = pending.pop() {
        if node.dom_node_id() == Some(id) {
            return Some(node);
        }
        pending.extend(node.children());
    }
    None
}

fn median(mut samples: Vec<Duration>) -> Duration {
    samples.sort_unstable();
    samples[samples.len() / 2]
}

fn millis(duration: Duration) -> f64 {
    duration.as_secs_f64() * 1000.0
}

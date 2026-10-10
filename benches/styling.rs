use std::{
    error::Error,
    fmt::Write,
    hint::black_box,
    time::{Duration, Instant},
};

use rust_browser::{css, html::parser, layout, style};

const SAMPLES: usize = 9;

fn main() -> Result<(), Box<dyn Error>> {
    let font = fontdue::Font::from_bytes(
        include_bytes!("../assets/fonts/noto-sans/NotoSans.ttf").as_slice(),
        fontdue::FontSettings::default(),
    )
    .map_err(std::io::Error::other)?;
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
        let wrapper = &reference.root().children()[0];
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
    Ok(())
}

fn time<T>(operation: impl FnOnce() -> T) -> Duration {
    let start = Instant::now();
    let result = operation();
    black_box(&result);
    start.elapsed()
}

fn median(mut samples: Vec<Duration>) -> Duration {
    samples.sort_unstable();
    samples[samples.len() / 2]
}

fn millis(duration: Duration) -> f64 {
    duration.as_secs_f64() * 1000.0
}

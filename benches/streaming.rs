use std::{
    error::Error,
    hint::black_box,
    io::{self, BufRead, BufReader, Write},
    net::TcpListener,
    thread,
    time::{Duration, Instant},
};

use rust_browser::{
    dom::{Document, NodeKind},
    html::parser::{self, HtmlParser},
    net,
};

const CHUNK_BYTES: usize = 8192;
const SAMPLES: usize = 7;

fn main() -> Result<(), Box<dyn Error>> {
    println!("Release-mode medians of {SAMPLES} samples; 8 KiB parser chunks.");
    println!("CPU timings exclude fetching, layout, painting, and final DOM destruction.");
    println!("Snapshots clone after each feed that mutates the DOM, plus the final clone.\n");
    println!("Input                    Buffered parse   Stream parse   Stream + snapshots");
    for (label, source) in [
        ("64 KiB paragraphs", paragraphs(64 * 1024)),
        ("1 MiB paragraphs", paragraphs(1024 * 1024)),
        (
            "1 MiB single attribute",
            format!("<p title=\"{}\">text</p>", "x".repeat(1024 * 1024)),
        ),
    ] {
        let expected = parser::parse(&source).expect("benchmark HTML should parse");
        assert_eq!(
            format!("{:?}", parse_chunks(&source, false)),
            format!("{expected:?}")
        );
        assert_eq!(
            format!("{:?}", parse_chunks(&source, true)),
            format!("{expected:?}")
        );

        let mut buffered = Vec::new();
        let mut streamed = Vec::new();
        let mut snapshots = Vec::new();
        for _ in 0..SAMPLES {
            buffered.push(time_parse(|| {
                parser::parse(black_box(&source)).expect("benchmark HTML should parse")
            }));
            streamed.push(time_parse(|| parse_chunks(black_box(&source), false)));
            snapshots.push(time_parse(|| parse_chunks(black_box(&source), true)));
        }
        println!(
            "{label:<24} {:>10.3} ms {:>10.3} ms {:>15.3} ms",
            millis(median(buffered)),
            millis(median(streamed)),
            millis(median(snapshots))
        );
    }

    http_timings()?;
    document_format_timings();
    Ok(())
}

fn document_format_timings() {
    use rust_browser::html::encoding::HtmlByteParser;
    let source = paragraphs(1024 * 1024);
    let legacy = b"<p>caf\xe9 &amp; text</p>".repeat(1024 * 1024 / 24);
    let utf16 = [
        vec![0xFF, 0xFE],
        source.encode_utf16().flat_map(u16::to_le_bytes).collect(),
    ]
    .concat();
    println!("\nDocument decoding and parsing, seven-sample medians:");
    for (label, bytes, mime) in [
        (
            "UTF-8 HTML",
            source.as_bytes(),
            Some("text/html; charset=utf-8"),
        ),
        ("Windows-1252 HTML", legacy.as_slice(), None),
        ("UTF-16 HTML", utf16.as_slice(), None),
    ] {
        let samples = (0..SAMPLES)
            .map(|_| {
                time_parse(|| {
                    let mut parser = HtmlByteParser::new(mime);
                    for chunk in black_box(bytes).chunks(CHUNK_BYTES) {
                        parser.feed(chunk).unwrap();
                    }
                    parser.finish().unwrap().parsed.document
                })
            })
            .collect();
        println!(
            "{label:<24} {:>10.3} ms ({} input bytes)",
            millis(median(samples)),
            bytes.len()
        );
    }
    let xml = format!(
        "<root xmlns:p='urn:p'>{}</root>",
        "<p:item value='1'>text &amp; café</p:item>".repeat(25000)
    );
    let samples = (0..SAMPLES)
        .map(|_| time_parse(|| rust_browser::xml::parse(black_box(&xml)).unwrap()))
        .collect();
    println!(
        "{:<24} {:>10.3} ms ({} UTF-8 bytes)",
        "XML namespaces/entities",
        millis(median(samples)),
        xml.len()
    );
}

fn paragraphs(bytes: usize) -> String {
    let mut row = String::from("<p title=\"left > right\">Streaming content: café 世界. ");
    row.push_str(&"x".repeat(128 - row.len() - "</p>\r\n".len()));
    row.push_str("</p>\r\n");
    assert_eq!(row.len(), 128);
    row.repeat(bytes / row.len())
}

fn parse_chunks(source: &str, snapshots: bool) -> Document {
    let mut parser = HtmlParser::new();
    let mut position = 0;
    let mut last_revision = parser.revision();
    while position < source.len() {
        let mut end = (position + CHUNK_BYTES).min(source.len());
        while !source.is_char_boundary(end) {
            end -= 1;
        }
        parser
            .feed(&source[position..end])
            .expect("benchmark chunk should parse");
        if snapshots && parser.revision() != last_revision {
            black_box(parser.document().clone());
            last_revision = parser.revision();
        }
        position = end;
    }
    let document = parser.finish().expect("benchmark HTML should finish");
    if snapshots {
        black_box(document.clone());
    }
    document
}

fn time_parse(parse: impl FnOnce() -> Document) -> Duration {
    let start = Instant::now();
    let document = parse();
    black_box(&document);
    start.elapsed()
}

fn median(mut samples: Vec<Duration>) -> Duration {
    samples.sort_unstable();
    samples[samples.len() / 2]
}

fn millis(duration: Duration) -> f64 {
    duration.as_secs_f64() * 1000.0
}

struct LoadTiming {
    first_text: Duration,
    complete: Duration,
}

fn http_timings() -> Result<(), Box<dyn Error>> {
    let source = paragraphs(64 * 1024);
    let listener = TcpListener::bind(("127.0.0.1", 0))?;
    let location = format!("http://{}/", listener.local_addr()?);
    let server_source = source.clone();
    let server = thread::spawn(move || -> io::Result<()> {
        for _ in 0..SAMPLES * 2 {
            let (mut stream, _) = listener.accept()?;
            stream.set_read_timeout(Some(Duration::from_secs(5)))?;
            stream.set_write_timeout(Some(Duration::from_secs(5)))?;
            let mut request = BufReader::new(&stream);
            loop {
                let mut line = String::new();
                if request.read_line(&mut line)? == 0 {
                    return Err(io::Error::from(io::ErrorKind::UnexpectedEof));
                }
                if line == "\r\n" {
                    break;
                }
            }
            thread::sleep(Duration::from_millis(40));
            write!(
                stream,
                "HTTP/1.1 200 OK\r\nContent-Type: text/html; charset=utf-8\r\n\
                 Content-Length: {}\r\nConnection: close\r\n\r\n",
                server_source.len()
            )?;
            for (index, chunk) in server_source.as_bytes().chunks(CHUNK_BYTES).enumerate() {
                if index > 0 {
                    thread::sleep(Duration::from_millis(20));
                }
                stream.write_all(chunk)?;
            }
        }
        Ok(())
    });

    let mut buffered_first = Vec::new();
    let mut streamed_first = Vec::new();
    let mut buffered_complete = Vec::new();
    let mut streamed_complete = Vec::new();
    for index in 0..SAMPLES {
        // Alternate order to reduce warm-cache/order bias.
        for streaming in if index % 2 == 0 {
            [false, true]
        } else {
            [true, false]
        } {
            let (timing, document) = time_http_load(&location, streaming)?;
            assert_eq!(
                format!("{document:?}"),
                format!("{:?}", parser::parse(&source).unwrap())
            );
            if streaming {
                streamed_first.push(timing.first_text);
                streamed_complete.push(timing.complete);
            } else {
                buffered_first.push(timing.first_text);
                buffered_complete.push(timing.complete);
            }
        }
    }
    server.join().expect("HTTP benchmark server panicked")?;

    let buffered_first = median(buffered_first);
    let streamed_first = median(streamed_first);
    println!("\nLoopback HTTP: 64 KiB, 40 ms before headers, eight 8 KiB writes 20 ms apart.");
    println!("Streaming includes DOM snapshots, but not the UI channel/layout/paint.");
    println!("Mode                First text in DOM     Complete DOM");
    println!(
        "Buffered            {:>12.3} ms {:>13.3} ms",
        millis(buffered_first),
        millis(median(buffered_complete))
    );
    println!(
        "Streaming           {:>12.3} ms {:>13.3} ms",
        millis(streamed_first),
        millis(median(streamed_complete))
    );
    println!(
        "First-text saving: {:.3} ms ({:.1}%, {:.2}x earlier). Not a first-paint measurement.",
        millis(buffered_first) - millis(streamed_first),
        100.0 * (1.0 - streamed_first.as_secs_f64() / buffered_first.as_secs_f64()),
        buffered_first.as_secs_f64() / streamed_first.as_secs_f64()
    );
    Ok(())
}

fn time_http_load(
    location: &str,
    streaming: bool,
) -> Result<(LoadTiming, Document), Box<dyn Error>> {
    let start = Instant::now();
    let mut first_text = None;
    let document = if streaming {
        let mut parser = HtmlParser::new();
        let mut last_revision = parser.revision();
        net::stream_html(location, CHUNK_BYTES, |chunk| {
            parser.feed(chunk).map_err(parse_error)?;
            if parser.revision() != last_revision {
                let snapshot = black_box(parser.document().clone());
                if first_text.is_none() && contains_first_paragraph_text(&snapshot) {
                    first_text = Some(start.elapsed());
                }
                last_revision = parser.revision();
            }
            Ok(())
        })?;
        let document = parser.finish().map_err(parse_error)?;
        black_box(document.clone());
        document
    } else {
        parser::parse(&net::fetch_html(location)?).map_err(parse_error)?
    };
    let complete = start.elapsed();
    if !streaming {
        assert!(contains_first_paragraph_text(&document));
        first_text = Some(complete);
    }
    Ok((
        LoadTiming {
            first_text: first_text.expect("first paragraph should be available"),
            complete,
        },
        document,
    ))
}

fn contains_first_paragraph_text(document: &Document) -> bool {
    document.preorder().any(|id| {
        let node = document.node(id).unwrap();
        matches!(node.kind(), NodeKind::Text(text) if !text.is_empty())
            && node
                .parent()
                .and_then(|parent| document.node(parent))
                .and_then(|node| node.as_element())
                .is_some_and(|element| element.tag_name() == "p")
    })
}

fn parse_error(error: parser::HtmlError) -> io::Error {
    io::Error::other(format!("HTML parse failed: {error:?}"))
}

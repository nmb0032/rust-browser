# Frame benchmark captures

`benches/frame.rs` always runs on deterministic synthetic pages. It also
benchmarks every `<name>/index.html` here. Captures are third-party content,
so they are gitignored and kept for local measurement only:

```sh
python3 tools/capture_perf_fixture.py https://news.ycombinator.com/ hn
python3 tools/capture_perf_fixture.py 'https://en.wikipedia.org/wiki/Rust_(programming_language)' wikipedia-rust
python3 tools/capture_perf_fixture.py https://github.com/rust-lang/rust github-rust
cargo bench --bench frame            # all pages
cargo bench --bench frame -- wiki    # name filter
```

Run the browser with `RUST_BROWSER_FRAME_TRACE=1` to log per-frame stage
timings (cascade, box build, layout, display list, raster) on the UI thread.

# Native headless reftests (milestone 8.13)

Run from the repository root:

```sh
cargo run --quiet -- --reftests tests/reftests --artifacts target/reftests/manual
cargo test --all-features --test reftests
cargo run --quiet -- tests/reftests/white-space-shaping.html \
  --screenshot target/text.png --viewport 800x600 --device-scale 2
```

No display server, desktop screenshot permissions, browser window, WPT checkout,
or network connection is needed. Screenshot output parents must already exist.
The CLI returns nonzero on a failed assertion, invalid input, missing resource,
unsupported readiness mechanism, or render/decode error.

## Rendering contract

`screenshot::{load, load_url, render, capture}` expose the native test API.
`load`/`load_url` return a local-policy `Frame`; `render` requires such a frame,
and `capture` combines loading and rendering. This uses **the production HTML
byte parser, resource loader, CSS cascade, container-query convergence, font
selection/rustybuzz shaping, layout, display-list builder and fontdue/software
rasterizer**. There is no screenshot proxy, character-based renderer, alternative
layout engine, or UI capture path. Normal interactive loading remains unchanged.

- Default viewport: **800×600 CSS pixels**, device scale **1**, light color scheme,
  opaque white canvas; unavailable media capabilities retain their normal
  unknown values. `--viewport WIDTHxHEIGHT` and `--device-scale N` set layout/media
  dimensions and `resolution` separately from framebuffer dimensions.
- `--scroll X,Y` and repeatable `--panel-scroll ID:X:Y` use CSS-pixel offsets.
  Root and panel axes are clamped by production layout scroll ranges; fixed
  content remains in viewport coordinates. Missing, ambiguous or non-scrollable
  panel IDs are errors. Both pages receive the same options. Panel IDs must exist
  on both pages if used with reftests. Offsets are native harness configuration,
  **not WPT HTML metadata**; future wptrunner tests requiring scrolling will need
  equivalent script/testdriver setup.
- Device dimensions must be integral, positive and at most **4096×4096**; scales
  are finite **0.125–8**. Fractional device dimensions are rejected, not rounded.
  Scroll inputs must be finite, nonnegative and at most 1,000,000 CSS pixels;
  at most 128 panel requests are accepted.
- Output and comparison are **unpremultiplied RGBA8 sRGB**. Native rasterization
  composites onto white (alpha 255). PNG references are interpreted as sRGB
  channel values, decoded to RGBA8 and alpha-composited onto white with the same
  integer rounding as paint. No desktop/ICC color-management transform or
  perceptual/UI-color tolerance is applied. Use sRGB PNG fixtures.
- HTML parser recovery is retained, including DOM-decoded HTML entities and
  ordinary escaped/percent-encoded URL paths. Parse diagnostics may appear on
  stderr; they do not themselves invalidate recoverable HTML. Frame errors and
  **all CSS/resource/font/layout diagnostics** make a screenshot fail rather than
  silently accepting missing fonts/images or nonconverged layout.
- The current engine's document inset and native scrollbar paint are retained,
  not removed for tests. References use body-relative geometry where appropriate,
  so they do not encode a private viewport inset into their expected positions.

### Fonts and local resources

The harness always uses `FontContext::bundled()` (licensed Noto Sans bytes in
`assets/fonts/noto-sans/NotoSans.ttf`, default font instance). No system font
discovery occurs. Author `@font-face` URL sources still go through the production
loader and affect both geometry and rasterization. `local()` is explicitly
rejected; it cannot introduce platform-dependent fonts. Use static URL fonts:
unsupported variable-font settings and fallback diagnostics fail visibly.

The shaping fixture loads the existing **static Noto Sans Italic** WOFF2 subset
through an external stylesheet, CSS import and escaped CSS URL. Its independent
reference loads the matching TTF and uses preformatted text rather than collapsed
inline spans. A contract test proves `ffi` is a single real glyph from the
downloadable italic face, not bundled fallback. These fixtures are already
licensed under **SIL OFL 1.1**; see `assets/fonts/noto-sans/OFL.txt` and
`PROVENANCE.txt`. No new font downloads or binary fixtures were added.

`--resource-root` defaults to the current directory. Every document, reference,
CSS import, image and non-data font URL must resolve to an existing regular file
under its **canonical** root. Network URLs, query URLs, traversal and symlink
escapes are rejected before resource fetch; ordinary remote `rel=help` links are
metadata and are never fetched. Bounded data-font sources remain supported.
Existing production HTML/CSS/image/font byte, import and decode limits apply.

## Test format and enumeration

Tests use [WPT reftest HTML format](https://web-platform-tests.org/writing-tests/reftests.html):

```html
<link rel="help" href="https://www.w3.org/TR/CSS22/box.html#collapsing-margins">
<link rel="match" href="references/example-ref.html">
<link rel="mismatch" href="references/not-example-ref.html">
```

`reftest::discover` recursively scans `.html`/`.htm` files beneath the suite and
selects pages containing match/mismatch links in the **parsed DOM**, in sorted
path order. References without assertion links are not standalone tests.
HTML's first `<base href>` rule resolves reference links and reference-specific
fuzzy metadata, just as it resolves page resources. Reference fragments are
preserved (including `:target`); query URLs are unsupported in local mode.
References may be HTML pages or local PNGs (the latter are a native convenience,
not a promise that wptrunner accepts PNG links). No screenshot baselines are
committed by this initial suite.

If match references exist, **at least one must match (OR)**. **Every mismatch
reference must mismatch (AND)**. Reference pages can themselves contain
assertions, which must pass; cyclic graphs are errors. All alternatives are
evaluated, so a resource/render error cannot be hidden by a passing alternative.
A valid but differing match alternative may fail while the overall OR passes.
Size/decode errors are invalid references, not successful mismatch assertions.

### Exact and fuzzy comparisons

Default equality is exact: `maxDifference=0`, `totalPixels=0`. `maxDifference`
is the largest absolute RGB-channel difference; `totalPixels` counts pixels with
any nonzero RGB difference **once**, not individual channels. Dimensions must
match exactly. The diff image contains absolute RGB channel differences.

WPT `meta name=fuzzy` is supported, with named or positional parameters,
inclusive ranges and optional reference URL keys:

```html
<meta name="fuzzy" content="maxDifference=0-2;totalPixels=0-20">
<meta name="fuzzy" content="references/example-ref.html:1-2;10-20">
```

A scalar such as `2;20` means **exactly 2 and exactly 20**, not `0-2;0-20`.
Both ranges must contain their measured value; a reference-specific declaration
overrides the default. Duplicate/unknown/invalid parameters and dangling
reference keys are errors. Match means within both ranges; mismatch means not
within both. **The committed fixtures use no fuzzy allowances**. Boundary tests
exercise both parameters, their lower and upper limits, zero, alpha normalization
and actual runner application; no hidden threshold hides layout regressions.

### Readiness and future interoperability

Synchronous loading completes resources/fonts and native paint before comparison.
There is no JavaScript/event loop or WebDriver ready protocol yet. **Any script
(including testdriver), `on*` handler, `reftest-wait` or `test-wait` class is an
explicit unsupported-readiness failure**, not a silently premature screenshot
or a passing/skipped assertion. Those capabilities remain pending the
JavaScript/WebDriver milestones (9.8/10).

The same HTML match/mismatch links, help links and fuzzy metadata are suitable
for later wptrunner discovery; this runner does **not** implement a WPT manifest
server, print/HTTP/variant tests, dynamic testdriver actions, or a wpt.fyi gate.
Passing this finite suite is **not full CSS specification or WPT conformance**.

## Evidence, limits and CI

Use a dedicated directory under the canonical resource root's `target/`, or
outside that root, **never inside the suite or source directories**. Symlink
artifact directories/files are refused. The runner owns `manifest.txt`,
`test-NNN/actual.png` (when a renderable test later errors), and
`test-NNN/comparison-NNN.{actual,expected,diff}.png`. Existing unrelated files
are not deleted. The manifest records the suite/root, font/viewport/scale/scroll
contract, each assertion relation and URL, fuzzy limits, measured metrics, final
PASS/FAIL/error and evidence paths.

Every compared pair has actual/expected/diff evidence, even an unsuccessful OR
alternative. A size error has actual/expected PNGs but no valid pixel diff;
stale diffs with that owned filename are removed. Missing/unsupported references
cannot have an expected image; the test's actual is retained when renderable.
The **current manifest**, not leftover files from an earlier run, is authoritative.

Enumeration is limited to 2,048 entries and 16 directory levels (symlinks are
rejected); each graph has at most 128 pages, 256 comparisons, 128 MiB cached
RGBA, 256 MiB decoded evidence work, and 8,192 bytes per reference URL. PNGs
are limited to 16 MiB encoded, 4096 per dimension and 64 MiB decoded allocation.

CI runs the real CLI harness and uploads `target/reftests/` on failure.
`cargo test --test reftests` also runs the eight committed tests at device scales
1 and 2, CLI PNG byte-reproducibility without a display, actual root/panel/fixed
pixel checks, metadata/readiness/resource/bounds failures, and an intentionally
wrong reference. That wrong-reference test **must observe a nonzero CLI exit,
nonzero pixel metrics and actual/expected/diff/manifest artifacts**; its assertion
passes because the runner correctly reports the deliberate failure.

Initial coverage: anonymous blocks, adjoining/auto margins, inline baselines,
white-space collapse, shaped ligatures/static web fonts, container feedback,
relative/absolute/fixed positioning, z-order/clips, flex/grid, floats/clearance
and fixed table cells. Further CSS coverage is pending; **8.14 is untouched**.

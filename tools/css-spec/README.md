# Frozen CSS property data

`webref-css.json` is the **unaltered `package/css.json` member** of the public
[`@webref/css` 8.7.6 release](https://registry.npmjs.org/@webref/css/-/css-8.7.6.tgz).
It is not a locally authored specification table. The release consolidates
spec definitions/extensions and includes 821 unique property entries, 529
type productions, 161 functions, 55 at-rules and 158 selectors. The 0.8 MB
snapshot includes experimental, SVG and compatibility properties; it is not
a Baseline feature list or an assertion about shipping browser support.

`provenance.json` pins the package version, repository Git ref, immutable
download URLs, tarball SHA-256/SHA-512, extracted snapshot SHA-256 and license
SHA-256. Each property/production retains its spec definition URL. The package
declares **MIT**; the repository license at the package's Git ref is preserved
in `LICENSE` (the npm artifact does not bundle a license file).

## Generate and check (offline)

```sh
python3 tools/generate_css_properties.py
python3 tools/generate_css_properties.py --check
PYTHONDONTWRITEBYTECODE=1 python3 tools/test_generate_css_properties.py
cargo test --all-features metadata_tests
cargo test --all-features --test css_coverage
cargo run -- --dump=css-coverage
```

Generation uses Python's standard library and the existing Rust toolchain's
`rustfmt`; it needs no crates, npm installation, build hook or network.
`--check` compares both generated artifacts byte-for-byte without writing.
`--dump=css-coverage` prints `docs/css-coverage.md` without loading the supplied
or default page. Ordinary builds use the checked-in Rust and report artifacts.

Outputs are `src/style/generated_properties.rs` and `docs/css-coverage.md`.
`style::metadata` exposes full property syntax, initial values, inheritance,
computed-value rules, animation types, alias/longhand/reset relationships,
scoped type/function productions and `@property` descriptor metadata. Missing
source fields remain `None`, including metadata absent on legacy aliases.

## Executable boundary and engine overlay

`engine-support.json` is **engine implementation policy**, not spec data.
It records Rust field mappings, finite semantic parser routes, implemented
keyword-to-value mappings, sample values, UA-dependent choices and stage gaps.
Property existence, syntax metadata, inheritance, initial tokens, shorthand
expansion/reset relationships, computed rules and animation descriptors come
from Webref. The generator rejects unknown properties, aliased "longhands",
duplicate field IDs, missing supported metadata, ambiguous inheritance,
keyword mappings absent from reachable spec terminals, and inconsistent
shorthand ordering.
The explicit `aliases` overlay must exactly match Webref's `legacyAliasOf`
relationship and target a canonical executable handler. It does not invent
missing spec fields or turn unsupported targets into accepted aliases.
Currently only `-webkit-box-sizing` is registered; `-webkit-text-fill-color`
is a separate, non-aliased property from the Compat Standard.

Generated longhand enumeration, shorthand expansion, `all`, inherited flags
and initial tokens replace the handwritten property registry. Initial tokens
use the real declaration parser for `initial`/`unset` and shorthand resets;
the only initial override is the **spec's UA-dependent** font-family choice
of `sans-serif`. `medium` font-size resolves using the existing initial font
environment; CanvasText uses the existing fixed light system palette. Tests
check *every* supported initial/inheritance rule and shorthand mapping against
the full frozen snapshot and actual cascade/defaults, including content,
height, containers, font, background, positioning/inset/z-index, and all physical border sides.

Executable generated grammar is intentionally finite: keyword and color
dispatch calls the existing parsers. Reachable spec keywords validate mappings;
they do **not** imply arbitrary terminals are valid standalone values.
The exact spec computed rule `an absolute length` generates the font-size
absolute-length stage selection. Other context-dependent computation remains
in handwritten semantic handlers; arbitrary prose, full syntax metadata,
referenced productions and animation descriptors are **not executable**
grammar/computation/animation implementations. No unsupported metadata entry
enters a catch-all property parser or makes `@supports` succeed.

Coverage is 0 fully implemented, 225 partial canonical properties (176 longhands
and 49 shorthands), 532 unimplemented canonical properties and 64 legacy
aliases (1 accepted for a partial target, 63 unsupported). Partial requires
an executable engine handler, but every
partial row records parser, cascade, computed, used/layout/paint and animation
gaps. Shorthand missing expansion/reset targets are generated from the spec.
Aliases do not count as independent grammar conformance. Unregistered custom
properties and finite registrations are reported separately, outside the
821-entry denominator. No external WPT or Chrome/Firefox parity gate is claimed.
The effects family uses a dedicated typed dispatcher; the generator validates
that mapping. Its mask shorthand has eight executable layer longhands and
explicit missing mask-border resets. Projected finite 3D, device-space filter
kernels and finite raster masks do not imply full Transforms/Filters/SVG support.
Declaration-local quirks are selected from HTML document mode, not from the
tokenizer or Webref metadata. Eligible declarations and `@supports` use the
same context, including pending `var()` shorthands and imports; registered
typed values and standalone parsers remain standards-only. See architecture
7.15 for the exact supported compatibility slice and remaining conformance gaps.

## Refresh or advance the snapshot

```sh
python3 tools/generate_css_properties.py --refresh
```

`--refresh` is the **only** network-enabled operation. It re-downloads the
already pinned public artifact/license, verifies all hashes, then regenerates.
It does not resolve `latest`, extract arbitrary archive paths, or update pins.
It cannot be combined with `--check`.

To advance intentionally: select a published `@webref/css` version, verify its
registry `dist.integrity`/repository `gitHead`, independently obtain the exact
artifact/license hashes, and review/update `provenance.json`. Run `--refresh`;
review the raw spec diff, metadata omissions, new/removed/aliased properties,
keyword and shorthand changes; update only the engine policies actually
implemented; regenerate and run the commands above plus `cargo test
--all-features` and `cargo fmt --all -- --check`. Update pinned count assertions
and this document after auditing changes, not to suppress failures.
Compare the generated report's unimplemented/partial rows over time. Reducing
a gap requires semantic work and evidence, not a broader accepting AST.

# Native animations (milestone 8.18)

Declarative CSS motion runs in the production native renderer. This is a
bounded implementation of CSS Animations, Transitions, scroll/view timelines
and a Rust Web Animations timing model, **not complete specification/WPT
conformance**. There is no JavaScript engine, DOM animation binding, promise
implementation or JavaScript event dispatch. HTML rendering-update integration
in 10.11 remains future work; the native sample/update boundary is ready for it.

## Try it

```sh
cargo run --quiet -- tests/pages/animations.html
cargo run --quiet -- tests/reftests-animations/moving-box.html \
  --screenshot target/animated.png --animation-time 1s --device-scale 2
cargo run --quiet -- --reftests tests/reftests-animations \
  --animation-time 1000ms --artifacts target/reftests/animation-manual
cargo test --all-features --test animations
```

The native window uses an `Instant`-relative document clock and schedules
`WaitUntil` redraws approximately every 16 ms only while document-timed effects
are running with nonzero playback rate. Paused, finished and scroll-only
effects do not spin. Scroll, scrollbar, keyboard, viewport, interaction and
resource updates use the same cascade/layout/display-list path. Author
`prefers-reduced-motion` CSS uses the existing media environment; the engine
does not impose a new unconditional reduced-motion policy.

Headless `Options::animation_time` is seconds. CLI bare numbers are milliseconds;
`ms` and `s` suffixes are accepted. The default is a **frozen document time of
zero**, preserving ordinary reference pages. Local resources settle first,
effects are established at zero, then the requested time is sampled. No sleep,
wall clock, remote browser or screenshot proxy is involved. Both sides of a
reftest get the same time and scroll configuration. Readiness scripts,
`reftest-wait`, animation-event script handlers and testdriver still require
unsupported JavaScript and fail rather than being silently bypassed.

## Update and lifetime contract

`Frame` retains its collected author sheets, a separate `author_styles`
snapshot, document-identified scroll state and a cloneable animation `Engine`.
Each update resolves author candidates, samples typed animation/transition
values into the normal cascade, computes fonts/units/colors, lays out, and
builds the real display list. Animated font values are resolved before dependent
keyframe dimensions and shaping. Percentages retain their containing-box basis;
font, viewport and container units use existing computed-value contexts.
Sampled values never replace the retained author input.

Animations beat normal author declarations but lose to important declarations.
Active transitions occupy the highest synthetic cascade level, including above
important declarations. Keyframe declarations use the existing parser,
CSS-wide values, pending `var()` shorthands and font/color computation.
Keyframe `!important` is ignored. Duplicate offsets combine declarations in
source order; omitted endpoints use the underlying computed value.

Animation list matching proceeds right-to-left by name, retaining creation time
and IDs across identical restyles, resource updates and same-document snapshot
clones. Renaming/removing effects or hiding their ancestor cancels them.
Timing edits do not restart a matching effect. Initial style resolution does
not accidentally start transitions. Subsequent source changes transition from
the actual previous rendered value; interruptions preserve continuity,
reversals shorten duration and negative delay, and the last matching
`transition-property` list entry wins. Logical properties share physical
transition identity. Completed/removed/cancelled transitions are cleaned up.

Every progressive loader snapshot for one load has the same document identity.
Worker snapshots adopt the latest retained native engine; separate loads,
including a reload of the same URL, never borrow the old clock/effect IDs.
Clones retain timing data but not shared mutable animation state.

## Native Rust control

```rust
use rust_browser::{engine::Frame, style::animation::EffectId};

fn seek_effect(frame: &mut Frame, effect: EffectId) -> Result<(), &'static str> {
    let now = frame.animations().time();
    let animation = frame.animations_mut().animation_mut(effect)
        .ok_or("effect no longer exists")?;
    animation.pause(now)?;
    animation.seek(0.75, now)?;
    Ok(())
}
```

Use `Frame::set_animation_time(seconds)` and
`Frame::layout_with_container_queries(&font_context)` to produce the next
sampled layout/paint snapshot. `Engine::effect_ids(node, pseudo)` addresses
generated pseudos separately. `Animation::new(Timing, start_time)`,
`update_timing`, `pause`, `play`, `seek`, `set_playback_rate`, and `finish`
validate finite representability and return errors; `cancel`, `current_time`,
`start_time`, `playback_rate`, `play_state`, `timing`, and `computed_timing`
expose the native model. CSS timing changes supersede native timing overrides;
identical CSS restyles do not. Finishing infinite/zero-rate effects or starting
an unbounded reverse effect is rejected.

`Engine::take_events()` drains native `Run`, `Start`, `Iteration`, `Finish` and
`Cancel` observation records with target/pseudo/ID/name and active elapsed
seconds (delays excluded). These are **not dispatched DOM events**. Sampling
across many iteration boundaries coalesces iteration observations; a bounded
queue is not a replacement for the future HTML event/promise ordering model.
This implementation also lacks pending animation tasks, ready/finished
promises, iteration-start/effect-level easing controls and complete UA
finished hold-time/seek semantics. Its explicit clock and effect ownership
are callable Rust integration points, not a claim that DOM milestone 9 or
HTML/JavaScript milestone 10 has been implemented.

## Supported timing, interpolation and timelines

- Typed duration/start-delay/end-delay, count (including fractional/infinite),
  direction, fill, pause, easing, composition and property/name lists.
  Atomic shorthands retain existing variable/reset behavior and handle
  ambiguous timing/direction/fill keywords before names. The pinned Webref
  snapshot defines `animation-delay` over `animation-delay-start` and
  `animation-delay-end`; the end delay is nonnegative. The `animation`
  shorthand resets supported timeline/range values but cannot implement its
  unsupported `animation-trigger` reset.
- Negative start delays, zero duration/count, alternating/reversed fills,
  exact active-end boundaries, and positive/negative native playback rates.
  Zero-duration infinite iterations are finite and idle. Linear, standard
  easing keywords, bounded cubic Bézier solving, `steps()` start/end and
  jump-start/end/none/both have deterministic endpoint/before-flag behavior.
- Numeric lengths/percentages/structural computed math, numbers/integers/font
  weights, line-height, typography width/spacing, and premultiplied Oklab
  colors. Opacity and bounded filter amounts clamp at computed-value time,
  not by clamping all easing overshoot. Compatible translate/scale/same-axis
  rotation/skew lists interpolate with identity padding. Blur and matching
  basic numeric/hue filter lists interpolate. Layout, real glyph selection
  and painting consume these samples, not just metadata.
- Legitimately discrete metadata/keyword pairs (including italic versus normal
  and intrinsic/automatic versus numeric sizes) switch at half progress without
  an unsupported-feature diagnostic. Discrete default transitions do not start.
- Replace and a finite add subset: numbers, px/percentage lengths, opacity,
  and transform-list concatenation. `accumulate` explicitly diagnoses a
  replace fallback; it is not incorrectly treated as add. Unsupported
  animatable value pairs switch discretely at half progress with a diagnostic.
  Unsupported transition pairs diagnose omission: `transition-behavior`
  and discrete transitions are not implemented.
- `auto` uses document time; `none` disables timing. `scroll()` supports root,
  self and nearest **ancestor** sources and block/inline/x/y axes.
  `view()` uses the actual subject and nearest ancestor scrollport.
  Named scroll/view declarations resolve on the subject/ancestor chain, not
  arbitrary nodes. Logical axes use source writing mode/direction, including
  negative-origin vertical/RTL ranges. Missing/non-scrollable/zero-range
  sources are inactive, leaving underlying styles.
- View cover/contain/entry/exit/entry-crossing/exit-crossing ranges, percentage
  or computed-length range offsets, and one/two insets are geometry-driven.
  Insets use the defining element's computed units; since 8.19 `auto` uses
  the source scrollport's physical scroll-padding, with reverse logical axes
  mapped to the corresponding start/end side. Pausing retains sampled timeline time.
  A completed layout/scroll snapshot feeds one additional sampled layout,
  without an unbounded feedback loop. Scroll-only motion advances on input
  updates (including 8.19 smooth-scroll samples), not continuous idle
  document-clock redraws.

## Explicit finite boundaries

Lists are at most 64 entries; an engine has at most 2,048 targets/effects,
2,048 distinct diagnostic messages, and 256 queued event records. A keyframes
collection and each definition have at most 256 definitions/rules/offsets,
with 2,048 declarations per combined
offset. Names are bounded to 4,096 bytes. Times have magnitude at most
1,000,000,000 seconds; finite counts use the same numeric bound, steps use at
most 1,000,000 jumps and native playback rates have magnitude at most
1,000,000. Unrepresentable arithmetic is rejected. Existing CSS/resource,
math-depth/token, container-convergence, glyph/raster and effect-buffer budgets
remain in force. Repeated transition interruptions cannot grow unbounded math
trees or transform lists.

`linear()`, timeline-scope/sibling timeline lookup, scoped/style/container
keyframes definitions, custom-property keyframe animation, vendor animation
aliases, discrete display transitions, transform matrix
decomposition/perspective interpolation, unequal-axis rotations, drop-shadow
interpolation, gradient/mask/path/track-list interpolation and full additive/
iteration accumulation remain outside this step. These are rejected or
diagnosed finite fallbacks, not parsed-and-ignored execution claims.
Unbounded scroll timelines are inactive. Scroll/layout feedback is sampled
once, not a promise of arbitrary cyclic geometry convergence or compositor-
only animation. Native UI logs diagnostics; the headless harness rejects
diagnostics and resource failures instead of accepting partial pixels.

Six independent static reference pairs cover moving geometry, colors, group
opacity, real font/layout sizing, steps/negative delay and important cascade at
one second and device scales 1/2. Rust tests additionally exercise retained
snapshots, lifecycle/elapsed records, resource/container restyles, interruption/
reversal, variables/keyframe ordering, finite fallback and root/nested/
vertical/named/view scroll timing. Existing 29 reference pages remain exact at
time zero. See [reftests.md](reftests.md). Milestone 8 and its external WPT exit
gate remain incomplete; 8.19 and later are not part of this implementation.

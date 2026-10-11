# Native scrolling (milestone 8.19)

This is a bounded working renderer/input implementation, not a full CSS/WPT or
CSSOM conformance claim. Milestone 8.20 fragmentation/printing is not included.
`tests/pages/scrolling.html` demonstrates sticky headers, nested fragment reveal,
snap carousels, containment and styled scrollbars without JavaScript.

## Shared production boundary

`Frame` retains `ScrollState` across layout, viewport/font/resource restyles and
progressive snapshots of the same document. Real DOM/pseudo identities key panel
offsets, gesture state, smooth requests and selected snap targets; viewport state
is separate. Clones are independent snapshots; navigation gets fresh identity.
The existing root/overflow range calculation preserves negative RTL/vertical
origins. Root overflow can propagate from body, but root snap, behavior, padding,
overscroll and scrollbar properties use the root element, not body propagation.

Rust clients use `LayoutTree::request_scroll(Some(node), offset, behavior)` or
`None` for the viewport. `ScrollBehavior::{Auto,Instant,Smooth}` distinguishes CSS
behavior from an explicit override. Requests reject non-finite coordinates and
missing scrollports. The older `set_scroll_offset`/`set_viewport_scroll` are raw
instant geometry setters for frozen harness/input state; they intentionally do
not invoke CSS snap/behavior. Hidden overflow accepts programmatic scrolling;
visible/clip axes have zero range. `Frame::retain_scroll_state` adopts updates
only from a tree with the same document identity.

## Sticky positioning

Sticky boxes remain normal-flow allocations, preserve relative-style adjoining
margin collapse, establish positioned containing blocks and create stacking
contexts even with `z-index:auto`. Absolute/fixed descendants keep the existing
semantic containing-block and viewport-escape rules.

Each current offset derives a translation from normal geometry, the nearest
scrollport (including hidden overflow), physical/logical insets and containing
block margin-box limits. Scroll-container containing areas include their
scrollable content extent; intervening ordinary blocks still constrain the end.
Opposing insets reduce the writing-axis end inset if the sticky view rectangle
would be smaller than the box. Negative margins remain signed. Nested sticky
translations compose once in the same spatial ancestry used by rasterization,
link/text/image hits, wheel targeting and scrollbar paint/inverse drag, including
transformed clip planes. Layout rectangles are never mutated by scrolling.

## Snap selection and gesture policy

Typed `scroll-snap-type` supports x/y/block/inline/both and mandatory/proximity
(an axis without strictness defaults to proximity). One/two `scroll-snap-align`
values represent block/inline axes. `scroll-snap-stop` supports normal/always.
Physical and logical `scroll-padding` and `scroll-margin` longhands/shorthands
participate in the normal variable/wide/rollback cascade. Padding accepts auto
(UA 0), nonnegative length/percentage; margin accepts signed lengths, not
percentages. Percent padding resolves against the corresponding scrollport axis.

Targets belong to the nearest ancestor scrollport; nested scrollers do not
capture their parent's snap selection. Fully clipped targets, absent boxes and
viewport-fixed descendants are excluded. Geometry uses unscrolled transformed
border bounds expanded by margins, and the padded snapport. Coordinates are
clamped to reachable ranges. Stable document order breaks equal-distance ties.
Both-axis requests prefer an actual joint target by Euclidean distance rather
than synthesizing an unreachable cross-pair of two elements.

**Explicit UA policies:** closest eligible endpoint/direction selection;
proximity threshold **30% of the corresponding port dimension**; wheel settle
is instantaneous after `TouchPhase::Ended/Cancelled`, or **120 ms** without a
mouse-wheel delta. Deltas scroll normally until that boundary, rather than
snapping each wheel event. Reversal resets the directional origin. Always-stop
targets intercept the nearest crossed position in that direction. An oversized
target permits free scrolling within the interval where it covers the padded
port, allowing escape beyond that interval. Selected targets are retained by
DOM identity and re-snapped after content/viewport changes; smooth snapped
requests follow the target's new geometry without restarting their clock.

These are deterministic engine policies, not promises to match another UA's
gesture, velocity, proximity or tie-breaking heuristics. There is no touch-pan
recognizer or momentum simulator; winit wheel phases and mouse idle are the
implemented gesture boundaries. Advanced snap-event/DOM APIs are absent.

## Smooth scrolling, overscroll and scheduling

`scroll-behavior:auto/smooth` affects programmatic requests, fragment reveal and
native keyboard scrolling, **not wheel deltas**. Smooth interpolation is
`t*t*(3-2*t)` over **clamp(0.18 + distance/2000, 0.18, 0.50) seconds** (distance in
CSS pixels). Samples use the same explicit document clock as CSS motion, before
CSS scroll/view timeline sampling and shared sticky paint. `sample_scrolling`
supports deterministic Rust sampling without sleeps. Same-axis wheel input
interrupts motion; other axes can continue. Pointer presses cancel outstanding
motions; new requests replace prior requests at their current sampled offset.
Removed scrollports cancel motion. Completion removes animation work.

Native scheduling uses approximately 16 ms `WaitUntil` redraws for running CSS
or scroll animations and the actual next wheel-settle deadline otherwise.
Finished/paused/scroll-only idle pages do not spin. Logical/physical arrow keys,
PageUp/PageDown and Home/End use the same per-axis chain/request boundary.

Typed `overscroll-behavior` x/y values auto/contain/none independently control
unconsumed wheel deltas, including a zero-range or already-exhausted scrollport.
Contain/none stop ancestor chaining; auto chains. No bounce/affordance is
simulated, so contain and none share pixels here. No cross-window/OS viewport
overscroll control, pull-to-refresh or browser gesture integration is claimed.

## Scrollbars and fragments

`scrollbar-color:auto` or a color pair uses the existing color pipeline
(including inherited currentcolor); `scrollbar-width:auto/thin/none` is
non-inherited. Panel overlays use **6/3 CSS px** tracks, bounded to the port,
and a **12 CSS px minimum thumb** bounded by track length. None suppresses both
paint and hit/drag, not the scrolling range. Negative-origin thumb placement
uses `(offset-min)/(max-min)`. Track clicks and drags use current inverse
transforms and sticky translations, even after a rebuild.

The engine preserves its historical undecorated root default: explicit root
color styling or thin width opts into viewport overlays; root none suppresses
them. There are **no reserved scrollbar gutters**. `scrollbar-gutter`,
stable/both-edges and vendor pseudo-element styling remain unsupported and
diagnose through the ordinary property registry rather than claiming layout.

Fragment resolution percent-decodes strict UTF-8, chooses the first exact ID
before legacy anchor name, then recognizes empty/top. Malformed percent/UTF-8
input returns a Rust error, while no matching target is an ordinary no-op.
`:target` is resolved by the existing interaction cascade. Same-document native
navigation updates URL/target without reloading or replacing timeline identity.
Pending navigation reveal survives progressive/font/effect resource layout
until settled; user scroll/pointer input cancels automatic pending reveal.

`scroll_into_view` is an inside-out shared Rust API: reveal nested hidden/auto
ports and then the viewport using scroll margin/padding, behavior and snapping.
It operates on source boxes, not visible hits, so clipping/masks do not prevent
reveal. `display:contents` targets use descendant bounds and inline targets
start with the first visual inline fragment. Physical axes and native vertical
writing are supported. Arbitrary transformed quads are approximated by
axis-aligned bounds, not precise 3D minimum-scroll optimization. Frame fragment
reveal supplies the source style even for contents targets with no layout box;
the standalone tree-only API uses the first descendant's style in that case.

## Deterministic evidence

Headless offsets remain frozen, instant and unsnapped by default. `--snap-scroll`
(`Options::snap_scroll`) opts into instant programmatic snap selection.
`--fragment VALUE` (`Options::fragment`) performs real settled fragment
navigation with an instant override; it is not a script/readiness substitute.
These options apply equally to assertion/reference documents and are recorded
in the options manifest. Fragment input is bounded to 4096 bytes; existing
panel, resource, geometry and raster budgets remain in force.

Five independent static reference pairs in `tests/reftests-scrolling` cover
root and nested sticky, an explicitly snapped panel, exact colored/thin overlay
geometry and nested fragment margin/padding/target paint. Each is exact at
scales 1/2, with no fuzzy tolerances or scripts. Rust production tests additionally
cover containment ends, opposing insets, negative margins/origins, vertical
logical axes, stacking/transform hits, clones/restyles, gesture/end/reversal/
always-stop/proximity/oversized rules, joint snap targets, clipping, stable
re-snap, smooth interruption/idle and CSS timeline ordering, inverse scrollbar
drag/none/root styles, encoded/contents fragments and the native resize renderer.

```sh
cargo test --all-features --lib scrolling_tests
cargo test --all-features --test scrolling
cargo run --quiet -- tests/pages/scrolling.html --screenshot target/scrolling.png
cargo run --quiet -- tests/reftests-scrolling/snap/snap.html \
  --screenshot target/snap.png --viewport 300x200 \
  --snap-scroll --panel-scroll p:0:64
```

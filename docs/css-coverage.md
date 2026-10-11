# Generated CSS coverage

Source: [`@webref/css 8.7.6`](https://registry.npmjs.org/@webref/css/-/css-8.7.6.tgz), MIT.
Repository ref: `7fc4c84bdc356eab59dccb571643c8a1e1f703dd`.
Snapshot SHA-256: `4860d2a9ad90814a61888e8b2d879651fd523c0d3926eecfe4548b4dd60a20eb`.

Regenerate offline: `python3 tools/generate_css_properties.py`; verify: `python3 tools/generate_css_properties.py --check`.
Read without loading a page: `cargo run -- --dump=css-coverage`.

Snapshot entries: **821**; canonical properties: **757**; legacy aliases: **64**.
Canonical support: **0 implemented**, **51 partial**, **706 unimplemented**. Legacy aliases: **1 accepted (partial targets)**, **63 unimplemented** (not independent grammar conformance).
Engine surface: **37 longhands**, **14 shorthands**.

Partial means an executable finite engine handler exists, not that an AST accepts the name or that its spec grammar/computation/layout is complete. Implemented would require all stages and a verified applicable external conformance gate. No property currently meets that gate. All property animation remains absent. No WPT or Chrome/Firefox parity result is asserted.

Full syntax, initial, inheritance, computed-value and animation strings are generated as metadata (missing source fields remain None). Only the color/keyword subsets and dispatch to existing semantic handlers are executable; arbitrary prose and the full value-definition grammar are **not** executable implementations. The exact spec `an absolute length` rule selects the existing font-size absolute-length stage; other contextual computations remain manual. Type/function productions and @property descriptors are exposed separately. This is the complete bounded release snapshot, including experimental/SVG/compat properties, not a Baseline list or a claim about what browsers ship.

## Partial property stage gaps

| Property | Executable dispatch | Parser gap / finite scope | Cascade gap | Computed-value gap | Used/layout/paint gap | Animation gap |
| --- | --- | --- | --- | --- | --- | --- |
| -webkit-box-sizing | legacy name alias of box-sizing | content-box and border-box, including the exact WHATWG -webkit-box-sizing legacy alias. | Alias shares canonical identity, priority, var and rollback; local tests only. | Specified keyword; no animation or full CSSOM. | Horizontal block and replaced width/definite height; minimum content box is zero. No min/max sizing, definite percentage-height or advanced layout. | No animation engine (including discrete animation). |
| -webkit-text-fill-color | color subset | Existing finite Color 4/5 subset; no quirky hex colors, stroking or advanced color gaps resolved. | Inherited currentcolor and wide/var/origin/layer tests; no external conformance gate. | Currentcolor is retained for inheritance; authoritative resolved fill is independent of color/borders/background. | Text, generated text, list markers and image alt text fill; bundled font, sRGB framebuffer, no strokes. | No animation engine (including discrete animation). |
| all | semantic subset: Wide | Five existing CSS-wide keywords only; snapshot revert-rule absent. | Expands only implemented longhands; does not implement snapshot's unsupported properties. | Delegates supported initial/inherited values. | Only supported property's finite layout/paint subset. | No animation engine (including discrete animation). |
| background | semantic subset: Background | One color and/or none; layers, sizing, positioning, repeat, attachment and boxes absent. | Resets only supported background-color/image; other longhand and reset targets absent. Missing expansion targets: background-position-x, background-position-y, background-size, background-repeat, background-attachment, background-origin, background-clip. Missing reset targets: background-blend-mode. | Only color and None image stored. | Flat color background only. | No animation engine (including discrete animation). |
| background-color | color subset | Finite Color 4/5 subset; no device-cmyk(), light-dark(), contrast-color(), custom profiles or exceptional numbers. | Local origin/layer/var/currentcolor tests; no external conformance gate. | System colors use a fixed light palette; dependency/serialization and exceptional-value gates remain. | sRGB byte framebuffer; no HDR or CSS perceptual gamut mapping. | No animation engine (including discrete animation). |
| background-image | generated keywords: none | Only none; image functions, URLs and multiple layers absent. | Single stored value; no layered image model. | Only BackgroundImage::None. | No CSS background images painted. | No animation engine (including discrete animation). |
| border | semantic subset: Border | Finite width/style/color subset; hairline, groove/ridge/inset/outset and additional colors absent. | Supported physical edges only; border-image reset targets absent. Missing reset targets: border-image-source, border-image-slice, border-image-width, border-image-outset, border-image-repeat. | Delegates finite edge handlers. | Basic box edges only; no full border paint conformance. | No animation engine (including discrete animation). |
| border-bottom | semantic subset: Border | Finite width/style/color subset; hairline, groove/ridge/inset/outset and additional colors absent. | Supported physical edges only; border-image reset targets absent. | Delegates finite edge handlers. | Basic box edges only; no full border paint conformance. | No animation engine (including discrete animation). |
| border-bottom-color | color subset | Finite Color 4/5 subset; border image-1D alternative, advanced Color 5 and exceptional numbers absent. | Local edge/origin/layer/var/currentcolor tests; no external conformance gate. | Color only, no one-dimensional image function; fixed light system palette and incomplete CSSOM. | sRGB byte framebuffer and basic box edges; no HDR, perceptual gamut mapping or border gradients. | No animation engine (including discrete animation). |
| border-bottom-style | generated keywords: dashed, dotted, double, hidden, none, solid | none/hidden/solid/dashed/dotted/double; groove/ridge/inset/outset absent. | Local side/four-value/wide tests; no external conformance gate. | Supported keyword enum only. | Basic edge paint; no full border-style conformance gate. | No animation engine (including discrete animation). |
| border-bottom-width | semantic subset: Longhand | Finite nonnegative lengths and thin/medium/thick; hairline and additional Values forms absent. | Local side/four-value/wide tests; no external conformance gate. | UA widths 1/3/5px; none/hidden zeroing, no spec snapping gate. | Basic box edges; no advanced border geometry. | No animation engine (including discrete animation). |
| border-color | semantic subset: Four | Finite Color 4/5 subset; border image-1D alternative, advanced Color 5 and exceptional numbers absent. | Local edge/origin/layer/var/currentcolor tests; no external conformance gate. | Color only, no one-dimensional image function; fixed light system palette and incomplete CSSOM. | sRGB byte framebuffer and basic box edges; no HDR, perceptual gamut mapping or border gradients. | No animation engine (including discrete animation). |
| border-left | semantic subset: Border | Finite width/style/color subset; hairline, groove/ridge/inset/outset and additional colors absent. | Supported physical edges only; border-image reset targets absent. | Delegates finite edge handlers. | Basic box edges only; no full border paint conformance. | No animation engine (including discrete animation). |
| border-left-color | color subset | Finite Color 4/5 subset; border image-1D alternative, advanced Color 5 and exceptional numbers absent. | Local edge/origin/layer/var/currentcolor tests; no external conformance gate. | Color only, no one-dimensional image function; fixed light system palette and incomplete CSSOM. | sRGB byte framebuffer and basic box edges; no HDR, perceptual gamut mapping or border gradients. | No animation engine (including discrete animation). |
| border-left-style | generated keywords: dashed, dotted, double, hidden, none, solid | none/hidden/solid/dashed/dotted/double; groove/ridge/inset/outset absent. | Local side/four-value/wide tests; no external conformance gate. | Supported keyword enum only. | Basic edge paint; no full border-style conformance gate. | No animation engine (including discrete animation). |
| border-left-width | semantic subset: Longhand | Finite nonnegative lengths and thin/medium/thick; hairline and additional Values forms absent. | Local side/four-value/wide tests; no external conformance gate. | UA widths 1/3/5px; none/hidden zeroing, no spec snapping gate. | Basic box edges; no advanced border geometry. | No animation engine (including discrete animation). |
| border-right | semantic subset: Border | Finite width/style/color subset; hairline, groove/ridge/inset/outset and additional colors absent. | Supported physical edges only; border-image reset targets absent. | Delegates finite edge handlers. | Basic box edges only; no full border paint conformance. | No animation engine (including discrete animation). |
| border-right-color | color subset | Finite Color 4/5 subset; border image-1D alternative, advanced Color 5 and exceptional numbers absent. | Local edge/origin/layer/var/currentcolor tests; no external conformance gate. | Color only, no one-dimensional image function; fixed light system palette and incomplete CSSOM. | sRGB byte framebuffer and basic box edges; no HDR, perceptual gamut mapping or border gradients. | No animation engine (including discrete animation). |
| border-right-style | generated keywords: dashed, dotted, double, hidden, none, solid | none/hidden/solid/dashed/dotted/double; groove/ridge/inset/outset absent. | Local side/four-value/wide tests; no external conformance gate. | Supported keyword enum only. | Basic edge paint; no full border-style conformance gate. | No animation engine (including discrete animation). |
| border-right-width | semantic subset: Longhand | Finite nonnegative lengths and thin/medium/thick; hairline and additional Values forms absent. | Local side/four-value/wide tests; no external conformance gate. | UA widths 1/3/5px; none/hidden zeroing, no spec snapping gate. | Basic box edges; no advanced border geometry. | No animation engine (including discrete animation). |
| border-style | semantic subset: Four | none/hidden/solid/dashed/dotted/double; groove/ridge/inset/outset absent. | Local side/four-value/wide tests; no external conformance gate. | Supported keyword enum only. | Basic edge paint; no full border-style conformance gate. | No animation engine (including discrete animation). |
| border-top | semantic subset: Border | Finite width/style/color subset; hairline, groove/ridge/inset/outset and additional colors absent. | Supported physical edges only; border-image reset targets absent. | Delegates finite edge handlers. | Basic box edges only; no full border paint conformance. | No animation engine (including discrete animation). |
| border-top-color | color subset | Finite Color 4/5 subset; border image-1D alternative, advanced Color 5 and exceptional numbers absent. | Local edge/origin/layer/var/currentcolor tests; no external conformance gate. | Color only, no one-dimensional image function; fixed light system palette and incomplete CSSOM. | sRGB byte framebuffer and basic box edges; no HDR, perceptual gamut mapping or border gradients. | No animation engine (including discrete animation). |
| border-top-style | generated keywords: dashed, dotted, double, hidden, none, solid | none/hidden/solid/dashed/dotted/double; groove/ridge/inset/outset absent. | Local side/four-value/wide tests; no external conformance gate. | Supported keyword enum only. | Basic edge paint; no full border-style conformance gate. | No animation engine (including discrete animation). |
| border-top-width | semantic subset: Longhand | Finite nonnegative lengths and thin/medium/thick; hairline and additional Values forms absent. | Local side/four-value/wide tests; no external conformance gate. | UA widths 1/3/5px; none/hidden zeroing, no spec snapping gate. | Basic box edges; no advanced border geometry. | No animation engine (including discrete animation). |
| border-width | semantic subset: Four | Finite nonnegative lengths and thin/medium/thick; hairline and additional Values forms absent. | Local side/four-value/wide tests; no external conformance gate. | UA widths 1/3/5px; none/hidden zeroing, no spec snapping gate. | Basic box edges; no advanced border geometry. | No animation engine (including discrete animation). |
| box-sizing | generated keywords: border-box, content-box | content-box and border-box, including the exact WHATWG -webkit-box-sizing legacy alias. | Alias shares canonical identity, priority, var and rollback; local tests only. | Specified keyword; no animation or full CSSOM. | Horizontal block and replaced width/definite height; minimum content box is zero. No min/max sizing, definite percentage-height or advanced layout. | No animation engine (including discrete animation). |
| color | color subset | Finite Color 4/5 subset; no device-cmyk(), light-dark(), contrast-color(), custom profiles or exceptional numbers. | Local origin/layer/var/currentcolor tests; no external conformance gate. | System colors use a fixed light palette; dependency/serialization and exceptional-value gates remain. | sRGB byte framebuffer; no HDR or CSS perceptual gamut mapping. | No animation engine (including discrete animation). |
| container | semantic subset: Container | Names with optional size/inline-size/normal type; scroll-state absent. | Resets both supported fields; local tests only. | Delegates name/type handlers. | Bounded horizontal block-size feedback only. | No animation engine (including discrete animation). |
| container-name | semantic subset: Longhand | none or bounded custom-ident list (256 tokens); no external grammar gate. | Local name/inheritance/query tests; no external conformance gate. | Ordered identifier list; style/scroll-state queries absent. | Named horizontal size containers only. | No animation engine (including discrete animation). |
| container-type | generated keywords: inline-size, normal, size | Only normal, size, inline-size; scroll-state and combinations absent. | Local named/anonymous query and var tests; no external conformance gate. | Keyword enum; style/scroll-state query semantics absent. | Bounded horizontal block-size feedback; no advanced formatting-context containment. | No animation engine (including discrete animation). |
| content | semantic subset: Longhand | Only normal, none, strings and untyped attr(name); no images, counters, quotes, alt text or element(). | Only before/after text pseudo boxes; no page-margin/replacement model. | Bounded text list; typed attr(), counters and full content computation absent. | Horizontal text only; generated images/markers/page margins absent. | No animation engine (including discrete animation). |
| direction | generated keywords: ltr, rtl | ltr and rtl keywords only. | Inherited, CSS-wide/var/origin/layer handling; excluded from all as required by CSS Cascade. | Inherited direction enum; unicode-bidi and logical properties absent. | Containing block direction selects the dropped margin in the normal-flow block width equation only; no bidi text, RTL inline flow, or writing modes. | No animation engine (including discrete animation). |
| display | generated keywords: block, inline, list-item, none | Only block, inline, list-item and none; no multi-keyword/flex/grid/table/ruby/math. | Local cascade only; no full display fixup model. | Single enum; no inner/outer pair, blockification/inlinification model. | Basic block/inline/list-item layout only. | No animation engine (including discrete animation). |
| font | semantic subset: Font | Finite style/weight/size/line-height/family subset; variants, stretch and system-font shorthand absent. | Expands/resets only five supported fields; other spec longhand/reset targets absent. Missing expansion targets: font-variant-ligatures, font-variant-caps, font-variant-alternates, font-variant-numeric, font-variant-east-asian, font-variant-position, font-variant-emoji, font-width. Missing reset targets: font-feature-settings, font-kerning, font-language-override, font-optical-sizing, font-size-adjust, font-variation-settings. | Delegates finite font handlers. | Bundled font and synthetic flags only. | No animation engine (including discrete animation). |
| font-family | semantic subset: Longhand | Family lists/generic names; no complete CSSOM serialization gate. | Local inherited list tests; no external conformance gate. | UA initial is sans-serif; typed list retained. | Family selection absent; bundled face is used. | No animation engine (including discrete animation). |
| font-size | semantic subset: Longhand | Finite lengths/math/size keywords; math keyword and additional Values 4/5 forms absent. | Local inheritance/var/origin tests; no external conformance gate. | UA medium=16px; rasterization clamps at 512px; keyword/metric approximations. | Bundled face, no font selection/shaping parity. | No animation engine (including discrete animation). |
| font-style | generated keywords: italic, normal, oblique | Only normal, italic, oblique; no oblique angle or left/right. | Local inherited tests; no external conformance gate. | Keyword enum only; no angle. | Synthetic italic, not font face selection. | No animation engine (including discrete animation). |
| font-weight | semantic subset: Longhand | Numbers 1..1000, normal/bold/bolder/lighter and finite number math. | Local relative/inherited tests; no external conformance gate. | Local relative-weight rules; no exhaustive external tests. | Synthetic bold, not variable-font face selection. | No animation engine (including discrete animation). |
| height | semantic subset: Longhand | Only auto and finite nonnegative lengths/percentages/math; intrinsic sizing/anchor-size absent. | Local wide/var tests; no external conformance gate. | Typed lengths; no full intrinsic sizing or percentage stage model. | CSS2 horizontal normal-flow block widths and supported replaced images; min/max/intrinsic sizing, height percentage/containment and advanced layouts incomplete. | No animation engine (including discrete animation). |
| line-height | semantic subset: Longhand | Finite nonnegative number/length/percentage/math and normal subset. | Local inherited tests; no external conformance gate. | normal approximated as 1.25em; metric/exceptional-value gaps. | Basic horizontal line boxes; no full inline formatting/shaping. | No animation engine (including discrete animation). |
| margin | semantic subset: Four | Auto and finite lengths/percentages/math; anchor-size() absent. | Atomic side/four-value/wide/var/origin/layer handling; local tests only. | Typed length-or-auto values retain auto and unresolved percentages, separately from used margins. | CSS2 horizontal normal-flow block equations, including supported replaced images, centering and direction-dependent overconstraint; inline/vertical auto margins are zero. Collapsing, positioned/flex/grid/logical layouts absent. | No animation engine (including discrete animation). |
| margin-bottom | semantic subset: Longhand | Auto and finite lengths/percentages/math; anchor-size() absent. | Atomic side/four-value/wide/var/origin/layer handling; local tests only. | Typed length-or-auto values retain auto and unresolved percentages, separately from used margins. | CSS2 horizontal normal-flow block equations, including supported replaced images, centering and direction-dependent overconstraint; inline/vertical auto margins are zero. Collapsing, positioned/flex/grid/logical layouts absent. | No animation engine (including discrete animation). |
| margin-left | semantic subset: Longhand | Auto and finite lengths/percentages/math; anchor-size() absent. | Atomic side/four-value/wide/var/origin/layer handling; local tests only. | Typed length-or-auto values retain auto and unresolved percentages, separately from used margins. | CSS2 horizontal normal-flow block equations, including supported replaced images, centering and direction-dependent overconstraint; inline/vertical auto margins are zero. Collapsing, positioned/flex/grid/logical layouts absent. | No animation engine (including discrete animation). |
| margin-right | semantic subset: Longhand | Auto and finite lengths/percentages/math; anchor-size() absent. | Atomic side/four-value/wide/var/origin/layer handling; local tests only. | Typed length-or-auto values retain auto and unresolved percentages, separately from used margins. | CSS2 horizontal normal-flow block equations, including supported replaced images, centering and direction-dependent overconstraint; inline/vertical auto margins are zero. Collapsing, positioned/flex/grid/logical layouts absent. | No animation engine (including discrete animation). |
| margin-top | semantic subset: Longhand | Auto and finite lengths/percentages/math; anchor-size() absent. | Atomic side/four-value/wide/var/origin/layer handling; local tests only. | Typed length-or-auto values retain auto and unresolved percentages, separately from used margins. | CSS2 horizontal normal-flow block equations, including supported replaced images, centering and direction-dependent overconstraint; inline/vertical auto margins are zero. Collapsing, positioned/flex/grid/logical layouts absent. | No animation engine (including discrete animation). |
| padding | semantic subset: Four | Finite nonnegative lengths/percentages/math subset; remaining Values forms absent. | Local side/four-value/wide tests; no external conformance gate. | Typed lengths; exceptional/percentage conformance gaps. | Basic horizontal boxes; logical/vertical/advanced layout absent. | No animation engine (including discrete animation). |
| padding-bottom | semantic subset: Longhand | Finite nonnegative lengths/percentages/math subset; remaining Values forms absent. | Local side/four-value/wide tests; no external conformance gate. | Typed lengths; exceptional/percentage conformance gaps. | Basic horizontal boxes; logical/vertical/advanced layout absent. | No animation engine (including discrete animation). |
| padding-left | semantic subset: Longhand | Finite nonnegative lengths/percentages/math subset; remaining Values forms absent. | Local side/four-value/wide tests; no external conformance gate. | Typed lengths; exceptional/percentage conformance gaps. | Basic horizontal boxes; logical/vertical/advanced layout absent. | No animation engine (including discrete animation). |
| padding-right | semantic subset: Longhand | Finite nonnegative lengths/percentages/math subset; remaining Values forms absent. | Local side/four-value/wide tests; no external conformance gate. | Typed lengths; exceptional/percentage conformance gaps. | Basic horizontal boxes; logical/vertical/advanced layout absent. | No animation engine (including discrete animation). |
| padding-top | semantic subset: Longhand | Finite nonnegative lengths/percentages/math subset; remaining Values forms absent. | Local side/four-value/wide tests; no external conformance gate. | Typed lengths; exceptional/percentage conformance gaps. | Basic horizontal boxes; logical/vertical/advanced layout absent. | No animation engine (including discrete animation). |
| width | semantic subset: Longhand | Only auto and finite nonnegative lengths/percentages/math; intrinsic sizing/anchor-size absent. | Local wide/var tests; no external conformance gate. | Typed lengths; no full intrinsic sizing or percentage stage model. | CSS2 horizontal normal-flow block widths and supported replaced images; min/max/intrinsic sizing, height percentage/containment and advanced layouts incomplete. | No animation engine (including discrete animation). |

## Custom properties and registration (outside the property denominator)

Webref does not enumerate arbitrary `--name` properties. Unregistered case-sensitive custom properties inherit bounded computed token streams with a guaranteed-invalid initial value. The runtime @property registration's inherits/initial-value descriptors override those rules. The generated descriptor syntax/default metadata does not supply missing required descriptors: the existing registration validator still requires syntax and inherits, and typed syntax requires a computationally independent initial-value.
Only `*`, `<length>`, `<number>`, `<integer>`, `<percentage>`, `<color>` registration syntax is executable; no alternatives/lists, relative registered lengths/currentcolor dependency model, full CSSOM or animation. This is **partial**, not another implemented snapshot property.

## All snapshot properties

Unsupported metadata does not grant parser acceptance. Only explicitly registered aliases with supported canonical targets are accepted. Their source metadata (including missing initial/inheritance fields) is preserved, not inferred from targets.

| Property | Kind | Support | Spec syntax | Initial | Inheritance | Computed value | Spec animation type |
| --- | --- | --- | --- | --- | --- | --- | --- |
| -webkit-align-content | alias of align-content | alias unimplemented | normal \| <baseline-position> \| <content-distribution> \| <overflow-position>? <content-position> | (missing in snapshot) | (missing in snapshot) | (missing in snapshot) | (missing in snapshot) |
| -webkit-align-items | alias of align-items | alias unimplemented | normal \| stretch \| <baseline-position> \| <overflow-position>? <self-position> | (missing in snapshot) | (missing in snapshot) | (missing in snapshot) | (missing in snapshot) |
| -webkit-align-self | alias of align-self | alias unimplemented | auto \| <overflow-position>? [ normal \| <self-position> ]\| stretch \| <baseline-position> | (missing in snapshot) | (missing in snapshot) | (missing in snapshot) | (missing in snapshot) |
| -webkit-animation | alias of animation | alias unimplemented | <single-animation># | (missing in snapshot) | (missing in snapshot) | (missing in snapshot) | (missing in snapshot) |
| -webkit-animation-delay | alias of animation-delay | alias unimplemented | <time># | (missing in snapshot) | (missing in snapshot) | (missing in snapshot) | (missing in snapshot) |
| -webkit-animation-direction | alias of animation-direction | alias unimplemented | <single-animation-direction># | (missing in snapshot) | (missing in snapshot) | (missing in snapshot) | (missing in snapshot) |
| -webkit-animation-duration | alias of animation-duration | alias unimplemented | <time [0s,∞]># | (missing in snapshot) | (missing in snapshot) | (missing in snapshot) | (missing in snapshot) |
| -webkit-animation-fill-mode | alias of animation-fill-mode | alias unimplemented | <single-animation-fill-mode># | (missing in snapshot) | (missing in snapshot) | (missing in snapshot) | (missing in snapshot) |
| -webkit-animation-iteration-count | alias of animation-iteration-count | alias unimplemented | <single-animation-iteration-count># | (missing in snapshot) | (missing in snapshot) | (missing in snapshot) | (missing in snapshot) |
| -webkit-animation-name | alias of animation-name | alias unimplemented | [ none \| <keyframes-name> ]# | (missing in snapshot) | (missing in snapshot) | (missing in snapshot) | (missing in snapshot) |
| -webkit-animation-play-state | alias of animation-play-state | alias unimplemented | <single-animation-play-state># | (missing in snapshot) | (missing in snapshot) | (missing in snapshot) | (missing in snapshot) |
| -webkit-animation-timing-function | alias of animation-timing-function | alias unimplemented | <easing-function># | (missing in snapshot) | (missing in snapshot) | (missing in snapshot) | (missing in snapshot) |
| -webkit-appearance | alias of appearance | alias unimplemented | none \| auto \| base \| base-select \| <compat-auto> \| <compat-special> | (missing in snapshot) | (missing in snapshot) | (missing in snapshot) | (missing in snapshot) |
| -webkit-backface-visibility | alias of backface-visibility | alias unimplemented | visible \| hidden | (missing in snapshot) | (missing in snapshot) | (missing in snapshot) | (missing in snapshot) |
| -webkit-background-clip | alias of background-clip | alias unimplemented | <visual-box># | (missing in snapshot) | (missing in snapshot) | (missing in snapshot) | (missing in snapshot) |
| -webkit-background-origin | alias of background-origin | alias unimplemented | <visual-box># | (missing in snapshot) | (missing in snapshot) | (missing in snapshot) | (missing in snapshot) |
| -webkit-background-size | alias of background-size | alias unimplemented | <bg-size># | (missing in snapshot) | (missing in snapshot) | (missing in snapshot) | (missing in snapshot) |
| -webkit-border-bottom-left-radius | alias of border-bottom-left-radius | alias unimplemented | <border-radius> | (missing in snapshot) | (missing in snapshot) | (missing in snapshot) | (missing in snapshot) |
| -webkit-border-bottom-right-radius | alias of border-bottom-right-radius | alias unimplemented | <border-radius> | (missing in snapshot) | (missing in snapshot) | (missing in snapshot) | (missing in snapshot) |
| -webkit-border-radius | alias of border-radius | alias unimplemented | <length-percentage [0,∞]>{1,4} [ / <length-percentage [0,∞]>{1,4} ]? | (missing in snapshot) | (missing in snapshot) | (missing in snapshot) | (missing in snapshot) |
| -webkit-border-top-left-radius | alias of border-top-left-radius | alias unimplemented | <border-radius> | (missing in snapshot) | (missing in snapshot) | (missing in snapshot) | (missing in snapshot) |
| -webkit-border-top-right-radius | alias of border-top-right-radius | alias unimplemented | <border-radius> | (missing in snapshot) | (missing in snapshot) | (missing in snapshot) | (missing in snapshot) |
| -webkit-box-align | longhand | unimplemented | (missing in snapshot) | (missing in snapshot) | (missing in snapshot) | (missing in snapshot) | (missing in snapshot) |
| -webkit-box-flex | longhand | unimplemented | (missing in snapshot) | (missing in snapshot) | (missing in snapshot) | (missing in snapshot) | (missing in snapshot) |
| -webkit-box-ordinal-group | longhand | unimplemented | (missing in snapshot) | (missing in snapshot) | (missing in snapshot) | (missing in snapshot) | (missing in snapshot) |
| -webkit-box-orient | longhand | unimplemented | (missing in snapshot) | (missing in snapshot) | (missing in snapshot) | (missing in snapshot) | (missing in snapshot) |
| -webkit-box-pack | longhand | unimplemented | (missing in snapshot) | (missing in snapshot) | (missing in snapshot) | (missing in snapshot) | (missing in snapshot) |
| -webkit-box-shadow | alias of box-shadow | alias unimplemented | <spread-shadow># | (missing in snapshot) | (missing in snapshot) | (missing in snapshot) | (missing in snapshot) |
| -webkit-box-sizing | alias of box-sizing | partial | content-box \| border-box | (missing in snapshot) | (missing in snapshot) | (missing in snapshot) | (missing in snapshot) |
| -webkit-filter | alias of filter | alias unimplemented | none \| <filter-value-list> | (missing in snapshot) | (missing in snapshot) | (missing in snapshot) | (missing in snapshot) |
| -webkit-flex | alias of flex | alias unimplemented | none \| [ <'flex-grow'> <'flex-shrink'>? \|\| <'flex-basis'> ] | (missing in snapshot) | (missing in snapshot) | (missing in snapshot) | (missing in snapshot) |
| -webkit-flex-basis | alias of flex-basis | alias unimplemented | content \| <'width'> | (missing in snapshot) | (missing in snapshot) | (missing in snapshot) | (missing in snapshot) |
| -webkit-flex-direction | alias of flex-direction | alias unimplemented | row \| row-reverse \| column \| column-reverse | (missing in snapshot) | (missing in snapshot) | (missing in snapshot) | (missing in snapshot) |
| -webkit-flex-flow | alias of flex-flow | alias unimplemented | <'flex-direction'> \|\| <'flex-wrap'> | (missing in snapshot) | (missing in snapshot) | (missing in snapshot) | (missing in snapshot) |
| -webkit-flex-grow | alias of flex-grow | alias unimplemented | <number [0,∞]> | (missing in snapshot) | (missing in snapshot) | (missing in snapshot) | (missing in snapshot) |
| -webkit-flex-shrink | alias of flex-shrink | alias unimplemented | <number [0,∞]> | (missing in snapshot) | (missing in snapshot) | (missing in snapshot) | (missing in snapshot) |
| -webkit-flex-wrap | alias of flex-wrap | alias unimplemented | nowrap \| [ wrap \| wrap-reverse ] \|\| balance | (missing in snapshot) | (missing in snapshot) | (missing in snapshot) | (missing in snapshot) |
| -webkit-justify-content | alias of justify-content | alias unimplemented | normal \| <content-distribution> \| <overflow-position>? [ <content-position> \| left \| right ] | (missing in snapshot) | (missing in snapshot) | (missing in snapshot) | (missing in snapshot) |
| -webkit-line-clamp | shorthand | unimplemented | none \| <integer [1,∞]> | none | see individual properties | see individual properties | see individual properties |
| -webkit-mask | alias of mask | alias unimplemented | <mask-layer># | (missing in snapshot) | (missing in snapshot) | (missing in snapshot) | (missing in snapshot) |
| -webkit-mask-box-image | alias of mask-border | alias unimplemented | <'mask-border-source'> \|\| <'mask-border-slice'> [ / <'mask-border-width'>? [ / <'mask-border-outset'> ]? ]? \|\| <'mask-border-repeat'> \|\| <'mask-border-mode'> | (missing in snapshot) | (missing in snapshot) | (missing in snapshot) | (missing in snapshot) |
| -webkit-mask-box-image-outset | alias of mask-border-outset | alias unimplemented | <'border-image-outset'> | (missing in snapshot) | (missing in snapshot) | (missing in snapshot) | (missing in snapshot) |
| -webkit-mask-box-image-repeat | alias of mask-border-repeat | alias unimplemented | <'border-image-repeat'> | (missing in snapshot) | (missing in snapshot) | (missing in snapshot) | (missing in snapshot) |
| -webkit-mask-box-image-slice | alias of mask-border-slice | alias unimplemented | <'border-image-slice'> | (missing in snapshot) | (missing in snapshot) | (missing in snapshot) | (missing in snapshot) |
| -webkit-mask-box-image-source | alias of mask-border-source | alias unimplemented | <'border-image-source'> | (missing in snapshot) | (missing in snapshot) | (missing in snapshot) | (missing in snapshot) |
| -webkit-mask-box-image-width | alias of mask-border-width | alias unimplemented | <'border-image-width'> | (missing in snapshot) | (missing in snapshot) | (missing in snapshot) | (missing in snapshot) |
| -webkit-mask-clip | alias of mask-clip | alias unimplemented | [ <coord-box> \| no-clip ]# | (missing in snapshot) | (missing in snapshot) | (missing in snapshot) | (missing in snapshot) |
| -webkit-mask-composite | alias of mask-composite | alias unimplemented | <compositing-operator># | (missing in snapshot) | (missing in snapshot) | (missing in snapshot) | (missing in snapshot) |
| -webkit-mask-image | alias of mask-image | alias unimplemented | <mask-reference># | (missing in snapshot) | (missing in snapshot) | (missing in snapshot) | (missing in snapshot) |
| -webkit-mask-origin | alias of mask-origin | alias unimplemented | <coord-box># | (missing in snapshot) | (missing in snapshot) | (missing in snapshot) | (missing in snapshot) |
| -webkit-mask-position | alias of mask-position | alias unimplemented | <position># | (missing in snapshot) | (missing in snapshot) | (missing in snapshot) | (missing in snapshot) |
| -webkit-mask-repeat | alias of mask-repeat | alias unimplemented | <repeat-style># | (missing in snapshot) | (missing in snapshot) | (missing in snapshot) | (missing in snapshot) |
| -webkit-mask-size | alias of mask-size | alias unimplemented | <bg-size># | (missing in snapshot) | (missing in snapshot) | (missing in snapshot) | (missing in snapshot) |
| -webkit-order | alias of order | alias unimplemented | <integer> | (missing in snapshot) | (missing in snapshot) | (missing in snapshot) | (missing in snapshot) |
| -webkit-perspective | alias of perspective | alias unimplemented | none \| <length [0,∞]> | (missing in snapshot) | (missing in snapshot) | (missing in snapshot) | (missing in snapshot) |
| -webkit-perspective-origin | alias of perspective-origin | alias unimplemented | <position> | (missing in snapshot) | (missing in snapshot) | (missing in snapshot) | (missing in snapshot) |
| -webkit-text-fill-color | longhand | partial | <color> | currentcolor | yes | an RGBA color | by computed value type |
| -webkit-text-size-adjust | alias of text-size-adjust | alias unimplemented | auto \| none \| <percentage [0,∞]> | (missing in snapshot) | (missing in snapshot) | (missing in snapshot) | (missing in snapshot) |
| -webkit-text-stroke | shorthand | unimplemented | <line-width> \|\| <color> | See individual properties | yes | See individual properties | See individual properties |
| -webkit-text-stroke-color | longhand | unimplemented | <color> | currentcolor | yes | an RGBA color | by computed value type |
| -webkit-text-stroke-width | longhand | unimplemented | <line-width> | 0 | yes | absolute length | discrete |
| -webkit-transform | alias of transform | alias unimplemented | none \| <transform-list> | (missing in snapshot) | (missing in snapshot) | (missing in snapshot) | (missing in snapshot) |
| -webkit-transform-origin | alias of transform-origin | alias unimplemented | [ left \| center \| right \| top \| bottom \| <length-percentage> ] \| [ left \| center \| right \| <length-percentage> ] [ top \| center \| bottom \| <length-percentage> ] <length>? \| [ [ center \| left \| right ] && [ center \| top \| bottom ] ] <length>? | (missing in snapshot) | (missing in snapshot) | (missing in snapshot) | (missing in snapshot) |
| -webkit-transform-style | alias of transform-style | alias unimplemented | flat \| preserve-3d | (missing in snapshot) | (missing in snapshot) | (missing in snapshot) | (missing in snapshot) |
| -webkit-transition | alias of transition | alias unimplemented | <single-transition># | (missing in snapshot) | (missing in snapshot) | (missing in snapshot) | (missing in snapshot) |
| -webkit-transition-delay | alias of transition-delay | alias unimplemented | <time># | (missing in snapshot) | (missing in snapshot) | (missing in snapshot) | (missing in snapshot) |
| -webkit-transition-duration | alias of transition-duration | alias unimplemented | <time [0s,∞]># | (missing in snapshot) | (missing in snapshot) | (missing in snapshot) | (missing in snapshot) |
| -webkit-transition-property | alias of transition-property | alias unimplemented | none \| <single-transition-property># | (missing in snapshot) | (missing in snapshot) | (missing in snapshot) | (missing in snapshot) |
| -webkit-transition-timing-function | alias of transition-timing-function | alias unimplemented | <easing-function># | (missing in snapshot) | (missing in snapshot) | (missing in snapshot) | (missing in snapshot) |
| -webkit-user-select | longhand | unimplemented | auto \| text \| none \| contain \| all | (missing in snapshot) | (missing in snapshot) | (missing in snapshot) | (missing in snapshot) |
| accent-color | longhand | unimplemented | auto \| <color> | auto | yes | the keyword auto or a computed color | by computed value type |
| align-content | longhand | unimplemented | normal \| <baseline-position> \| <content-distribution> \| <overflow-position>? <content-position> | normal | no | specified keyword(s) | discrete |
| align-items | longhand | unimplemented | normal \| stretch \| <baseline-position> \| <overflow-position>? <self-position> | normal | no | specified keyword(s) | discrete |
| align-self | longhand | unimplemented | auto \| <overflow-position>? [ normal \| <self-position> ]\| stretch \| <baseline-position> \| anchor-center | auto | no | specified keyword(s) | discrete |
| alignment-baseline | longhand | unimplemented | baseline \| <baseline-metric> | baseline | no | specified keyword | discrete |
| all | shorthand | partial | initial \| inherit \| unset \| revert \| revert-layer \| revert-rule | see individual properties | see individual properties | see individual properties | see individual properties |
| anchor-name | longhand | unimplemented | none \| <anchor-name># | none | no | as specified | discrete |
| anchor-scope | longhand | unimplemented | none \| all \| <anchor-name># | none | no | as specified | discrete |
| animation | shorthand | unimplemented | <single-animation># | see individual properties | no | see individual properties | not animatable |
| animation-composition | longhand | unimplemented | <single-animation-composition># | replace | no | list, each item a keyword as specified | not animatable |
| animation-delay | shorthand | unimplemented | [ <'animation-delay-start'> <'animation-delay-end'>? ]# | see individual properties | see individual properties | see individual properties | see individual properties |
| animation-delay-end | longhand | unimplemented | <time [0s,∞]># | 0s | no | list, each item a duration | not animatable |
| animation-delay-start | longhand | unimplemented | <time># | 0s | no | list, each item a duration | not animatable |
| animation-direction | longhand | unimplemented | <single-animation-direction># | normal | no | list, each item a keyword as specified | not animatable |
| animation-duration | longhand | unimplemented | [ auto \| <time [0s,∞]> ]# | auto | no | list, each item either a time or the keyword auto | not animatable |
| animation-fill-mode | longhand | unimplemented | <single-animation-fill-mode># | none | no | list, each item a keyword as specified | not animatable |
| animation-iteration-count | longhand | unimplemented | <single-animation-iteration-count># | 1 | no | list, each item either a number or the keyword infinite | not animatable |
| animation-name | longhand | unimplemented | [ none \| <keyframes-name> ]# | none | no | list, each item either a case-sensitive css identifier or the keyword none | not animatable |
| animation-play-state | longhand | unimplemented | <single-animation-play-state># | running | no | list, each item a keyword as specified | not animatable |
| animation-range | shorthand | unimplemented | [ <'animation-range-start'> <'animation-range-end'>? ]# | see individual properties | see individual properties | see individual properties | see individual properties |
| animation-range-center | longhand | unimplemented | [ normal \| [ <length-percentage> \| <timeline-range-center-subject> <length-percentage>? ] ]# | normal | no | list, each item either the keyword normal or a timeline range center subject and a length-percentage value | not animatable |
| animation-range-end | longhand | unimplemented | [ normal \| <length-percentage> \| <timeline-range-name> <length-percentage>? ]# | normal | no | list, each item either the keyword normal or a timeline range and progress percentage | not animatable |
| animation-range-start | longhand | unimplemented | [ normal \| <length-percentage> \| <timeline-range-name> <length-percentage>? ]# | normal | no | list, each item either the keyword normal or a timeline range and progress percentage | not animatable |
| animation-timeline | longhand | unimplemented | <single-animation-timeline># | auto | no | list, each item either the keyword none, the keyword auto, a case-sensitive css identifier, a computed scroll() function, or a computed view() function | not animatable |
| animation-timing-function | longhand | unimplemented | <easing-function># | ease | no | list, each item a computed <easing-function> | not animatable |
| animation-trigger | longhand | unimplemented | [ none \| [ <dashed-ident> <animation-action>+ ]+ ]# | none | no | as specified | not animatable |
| appearance | longhand | unimplemented | none \| auto \| base \| base-select \| <compat-auto> \| <compat-special> | none | no | specified keyword | discrete |
| aspect-ratio | longhand | unimplemented | auto \|\| <ratio> | auto | no | specified keyword or a pair of numbers | by computed value |
| backdrop-filter | longhand | unimplemented | none \| <filter-value-list> | none | no | as specified | see prose in Filter Effects 1 § 14. Animation of Filters. |
| backface-visibility | longhand | unimplemented | visible \| hidden | visible | no | specified keyword | discrete |
| background | shorthand | partial | <bg-layer>#? , <final-bg-layer> | see individual properties | no | see individual properties | see individual properties |
| background-attachment | longhand | unimplemented | <attachment># | scroll | no | list, each item the keyword as specified | discrete |
| background-blend-mode | longhand | unimplemented | <'mix-blend-mode'># | normal | no | as specified | discrete |
| background-clip | longhand | unimplemented | <bg-clip># | border-box | no | as specified | repeatable list |
| background-color | longhand | partial | <color> | transparent | no | computed color | by computed value |
| background-image | longhand | partial | <bg-image># | none | no | list, each item either an <image> or the keyword none | discrete |
| background-origin | longhand | unimplemented | <visual-box># | padding-box | no | list, each item a keyword as specified | repeatable list |
| background-position | shorthand | unimplemented | <bg-position># | 0% 0% | no | a list, each item a pair of offsets (horizontal and vertical) from the top left origin, each offset given as a computed <length-percentage> value | repeatable list |
| background-position-block | longhand | unimplemented | [ center \| [ [ start \| end ]? <length-percentage>? ]! ]# | 0% | no | A list, each item consisting of: an offset given as a computed <length-percentage> value, plus an origin keyword | repeatable list |
| background-position-inline | longhand | unimplemented | [ center \| [ [ start \| end ]? <length-percentage>? ]! ]# | 0% | no | A list, each item consisting of: an offset given as a computed <length-percentage> value, plus an origin keyword | repeatable list |
| background-position-x | longhand | unimplemented | [ center \| [ [ left \| right \| x-start \| x-end ]? <length-percentage>? ]! ]# | 0% | no | A list, each item consisting of: an offset given as a computed <length-percentage> value, plus an origin keyword | repeatable list |
| background-position-y | longhand | unimplemented | [ center \| [ [ top \| bottom \| y-start \| y-end ]? <length-percentage>? ]! ]# | 0% | no | A list, each item consisting of: an offset given as a computed <length-percentage> value, plus an origin keyword | repeatable list |
| background-repeat | longhand | unimplemented | <repeat-style># | repeat | no | list, each item a pair of keywords, one per dimension | discrete |
| background-repeat-block | longhand | unimplemented | <repetition># | repeat | no | as specified | discrete |
| background-repeat-inline | longhand | unimplemented | <repetition># | repeat | no | as specified | discrete |
| background-repeat-x | longhand | unimplemented | <repetition># | repeat | no | as specified | discrete |
| background-repeat-y | longhand | unimplemented | <repetition># | repeat | no | as specified | discrete |
| background-size | longhand | unimplemented | <bg-size># | auto | no | list, each item a pair of sizes (one per axis) each represented as either a keyword or a computed <length-percentage> value | repeatable list |
| background-tbd | longhand | unimplemented | <bg-layer># | see individual properties | no | see individual properties | see individual properties |
| baseline-shift | longhand | unimplemented | <length-percentage> \| sub \| super \| top \| center \| bottom | 0 | no | the specified keyword or a computed <length-percentage> value | by computed value type |
| baseline-source | longhand | unimplemented | auto \| first \| last | auto | no | specified keyword | discrete |
| block-ellipsis | longhand | unimplemented | no-ellipsis \| auto \| <string> | no-ellipsis | yes | specified value | discrete |
| block-size | longhand | unimplemented | auto \| <box-size> \| <anchor-size()> | auto | no | as specified, with <length-percentage> values computed | by computed value type, recursing into fit-content() |
| block-step | shorthand | unimplemented | <'block-step-size'> \|\| <'block-step-insert'> \|\| <'block-step-align'> \|\| <'block-step-round'> | see individual properties | no | see individual properties | see individual properties |
| block-step-align | longhand | unimplemented | auto \| center \| start \| end | auto | no | specified keyword | discrete |
| block-step-insert | longhand | unimplemented | margin-box \| padding-box \| content-box | margin-box | no | specified keyword | discrete |
| block-step-round | longhand | unimplemented | up \| down \| nearest | up | no | specified keyword | discrete |
| block-step-size | longhand | unimplemented | none \| <length [0,∞]> | none | no | specified keyword or absolute length | by computed value type |
| bookmark-label | longhand | unimplemented | <content-list> | content(text) | no | specified value | discrete |
| bookmark-level | longhand | unimplemented | none \| <integer [1,∞]> | none | no | the keyword none or the specified integer | by computed value type |
| bookmark-state | longhand | unimplemented | open \| closed | open | no | specified keyword | discrete |
| border | shorthand | partial | <line-width> \|\| <line-style> \|\| <color> | see individual properties | see individual properties | see individual properties | see individual properties |
| border-block | shorthand | unimplemented | <'border-block-start'> | see individual properties | see individual properties | see individual properties | see individual properties |
| border-block-clip | shorthand | unimplemented | <'border-top-clip'> | see individual properties | see individual properties | see individual properties | see individual properties |
| border-block-color | shorthand | unimplemented | <'border-top-color'>{1,2} | see individual properties | see individual properties | see individual properties | see individual properties |
| border-block-end | shorthand | unimplemented | <line-width> \|\| <line-style> \|\| <color> | See individual properties | no | see individual properties | see individual properties |
| border-block-end-clip | longhand | unimplemented | none \| [ <length-percentage [0,∞]> \| <flex> ]+ | none | no | none, or a list consisting of absolute lengths, or percentages as specified | by computed value |
| border-block-end-color | longhand | unimplemented | <color> \| <image-1D> | currentcolor | no | the computed color and/or a one-dimensional image function | see prose |
| border-block-end-radius | shorthand | unimplemented | <length-percentage [0,∞]>{1,2} [ / <length-percentage [0,∞]>{1,2} ]? | 0 | no | see individual properties | see individual properties |
| border-block-end-style | longhand | unimplemented | <line-style> | none | no | specified keyword | discrete |
| border-block-end-width | longhand | unimplemented | <line-width> | medium | no | absolute length, snapped as a border width | by computed value |
| border-block-start | shorthand | unimplemented | <line-width> \|\| <line-style> \|\| <color> | See individual properties | no | see individual properties | see individual properties |
| border-block-start-clip | longhand | unimplemented | none \| [ <length-percentage [0,∞]> \| <flex> ]+ | none | no | none, or a list consisting of absolute lengths, or percentages as specified | by computed value |
| border-block-start-color | longhand | unimplemented | <color> \| <image-1D> | currentcolor | no | the computed color and/or a one-dimensional image function | see prose |
| border-block-start-radius | shorthand | unimplemented | <length-percentage [0,∞]>{1,2} [ / <length-percentage [0,∞]>{1,2} ]? | 0 | no | see individual properties | see individual properties |
| border-block-start-style | longhand | unimplemented | <line-style> | none | no | specified keyword | discrete |
| border-block-start-width | longhand | unimplemented | <line-width> | medium | no | absolute length, snapped as a border width | by computed value |
| border-block-style | shorthand | unimplemented | <'border-top-style'>{1,2} | see individual properties | see individual properties | see individual properties | see individual properties |
| border-block-width | shorthand | unimplemented | <'border-top-width'>{1,2} | see individual properties | see individual properties | see individual properties | see individual properties |
| border-bottom | shorthand | partial | <line-width> \|\| <line-style> \|\| <color> | See individual properties | no | see individual properties | see individual properties |
| border-bottom-clip | longhand | unimplemented | none \| [ <length-percentage [0,∞]> \| <flex> ]+ | none | no | none, or a list consisting of absolute lengths, or percentages as specified | by computed value |
| border-bottom-color | longhand | partial | <color> \| <image-1D> | currentcolor | no | the computed color and/or a one-dimensional image function | see prose |
| border-bottom-left-radius | longhand | unimplemented | <border-radius> | 0 | no | pair of computed <length-percentage> values | by computed value |
| border-bottom-radius | shorthand | unimplemented | <length-percentage [0,∞]>{1,2} [ / <length-percentage [0,∞]>{1,2} ]? | 0 | no | see individual properties | see individual properties |
| border-bottom-right-radius | longhand | unimplemented | <border-radius> | 0 | no | pair of computed <length-percentage> values | by computed value |
| border-bottom-style | longhand | partial | <line-style> | none | no | specified keyword | discrete |
| border-bottom-width | longhand | partial | <line-width> | medium | no | absolute length, snapped as a border width | by computed value |
| border-boundary | longhand | unimplemented | none \| parent \| display | none | yes | specified keyword | discrete |
| border-clip | shorthand | unimplemented | <'border-top-clip'> | see individual properties | see individual properties | see individual properties | see individual properties |
| border-collapse | longhand | unimplemented | separate \| collapse | separate | yes | specified keyword | discrete |
| border-color | shorthand | partial | [ <color> \| <image-1D> ]{1,4} | see individual properties | see individual properties | see individual properties | see individual properties |
| border-end-end-radius | longhand | unimplemented | <border-radius> | 0 | no | pair of computed <length-percentage> values | by computed value |
| border-end-start-radius | longhand | unimplemented | <border-radius> | 0 | no | pair of computed <length-percentage> values | by computed value |
| border-image | shorthand | unimplemented | <'border-image-source'> \|\| <'border-image-slice'> [ / <'border-image-width'> \| / <'border-image-width'>? / <'border-image-outset'> ]? \|\| <'border-image-repeat'> | See individual properties | no | See individual properties | See individual properties |
| border-image-outset | longhand | unimplemented | [ <length [0,∞]> \| <number [0,∞]> ]{1,4} | 0 | no | four values, each a number or absolute length | by computed value |
| border-image-repeat | longhand | unimplemented | [ stretch \| repeat \| round \| space ]{1,2} | stretch | no | two keywords, one per axis | discrete |
| border-image-slice | longhand | unimplemented | [<number [0,∞]> \| <percentage [0,∞]>]{1,4} && fill? | 100% | no | four values, each either a number or percentage; plus a fill keyword if specified | by computed value |
| border-image-source | longhand | unimplemented | none \| <image> | none | no | the keyword none or the computed <image> | discrete |
| border-image-width | longhand | unimplemented | [ <length-percentage [0,∞]> \| <number [0,∞]> \| auto ]{1,4} | 1 | no | four values, each either a number, the keyword auto, or a computed <length-percentage> value | by computed value |
| border-inline | shorthand | unimplemented | <'border-block-start'> | see individual properties | see individual properties | see individual properties | see individual properties |
| border-inline-clip | shorthand | unimplemented | <'border-top-clip'> | see individual properties | see individual properties | see individual properties | see individual properties |
| border-inline-color | shorthand | unimplemented | <'border-top-color'>{1,2} | see individual properties | see individual properties | see individual properties | see individual properties |
| border-inline-end | shorthand | unimplemented | <line-width> \|\| <line-style> \|\| <color> | See individual properties | no | see individual properties | see individual properties |
| border-inline-end-clip | longhand | unimplemented | none \| [ <length-percentage [0,∞]> \| <flex> ]+ | none | no | none, or a list consisting of absolute lengths, or percentages as specified | by computed value |
| border-inline-end-color | longhand | unimplemented | <color> \| <image-1D> | currentcolor | no | the computed color and/or a one-dimensional image function | see prose |
| border-inline-end-radius | shorthand | unimplemented | <length-percentage [0,∞]>{1,2} [ / <length-percentage [0,∞]>{1,2} ]? | 0 | no | see individual properties | see individual properties |
| border-inline-end-style | longhand | unimplemented | <line-style> | none | no | specified keyword | discrete |
| border-inline-end-width | longhand | unimplemented | <line-width> | medium | no | absolute length, snapped as a border width | by computed value |
| border-inline-start | shorthand | unimplemented | <line-width> \|\| <line-style> \|\| <color> | See individual properties | no | see individual properties | see individual properties |
| border-inline-start-clip | longhand | unimplemented | none \| [ <length-percentage [0,∞]> \| <flex> ]+ | none | no | none, or a list consisting of absolute lengths, or percentages as specified | by computed value |
| border-inline-start-color | longhand | unimplemented | <color> \| <image-1D> | currentcolor | no | the computed color and/or a one-dimensional image function | see prose |
| border-inline-start-radius | shorthand | unimplemented | <length-percentage [0,∞]>{1,2} [ / <length-percentage [0,∞]>{1,2} ]? | 0 | no | see individual properties | see individual properties |
| border-inline-start-style | longhand | unimplemented | <line-style> | none | no | specified keyword | discrete |
| border-inline-start-width | longhand | unimplemented | <line-width> | medium | no | absolute length, snapped as a border width | by computed value |
| border-inline-style | shorthand | unimplemented | <'border-top-style'>{1,2} | see individual properties | see individual properties | see individual properties | see individual properties |
| border-inline-width | shorthand | unimplemented | <'border-top-width'>{1,2} | see individual properties | see individual properties | see individual properties | see individual properties |
| border-left | shorthand | partial | <line-width> \|\| <line-style> \|\| <color> | See individual properties | no | see individual properties | see individual properties |
| border-left-clip | longhand | unimplemented | none \| [ <length-percentage [0,∞]> \| <flex> ]+ | none | no | none, or a list consisting of absolute lengths, or percentages as specified | by computed value |
| border-left-color | longhand | partial | <color> \| <image-1D> | currentcolor | no | the computed color and/or a one-dimensional image function | see prose |
| border-left-radius | shorthand | unimplemented | <length-percentage [0,∞]>{1,2} [ / <length-percentage [0,∞]>{1,2} ]? | 0 | no | see individual properties | see individual properties |
| border-left-style | longhand | partial | <line-style> | none | no | specified keyword | discrete |
| border-left-width | longhand | partial | <line-width> | medium | no | absolute length, snapped as a border width | by computed value |
| border-limit | longhand | unimplemented | all \| [ sides \| corners ] <length-percentage [0,∞]>? \| [ top \| right \| bottom \| left ] <length-percentage [0,∞]> | all | no | as specified | discrete |
| border-radius | shorthand | unimplemented | <length-percentage [0,∞]>{1,4} [ / <length-percentage [0,∞]>{1,4} ]? | see individual properties | see individual properties | see individual properties | see individual properties |
| border-right | shorthand | partial | <line-width> \|\| <line-style> \|\| <color> | See individual properties | no | see individual properties | see individual properties |
| border-right-clip | longhand | unimplemented | none \| [ <length-percentage [0,∞]> \| <flex> ]+ | none | no | none, or a list consisting of absolute lengths, or percentages as specified | by computed value |
| border-right-color | longhand | partial | <color> \| <image-1D> | currentcolor | no | the computed color and/or a one-dimensional image function | see prose |
| border-right-radius | shorthand | unimplemented | <length-percentage [0,∞]>{1,2} [ / <length-percentage [0,∞]>{1,2} ]? | 0 | no | see individual properties | see individual properties |
| border-right-style | longhand | partial | <line-style> | none | no | specified keyword | discrete |
| border-right-width | longhand | partial | <line-width> | medium | no | absolute length, snapped as a border width | by computed value |
| border-shape | longhand | unimplemented | none \| [ <basic-shape> <geometry-box>?]{1,2} | none | no | list, each item a computed color | by computed value |
| border-spacing | longhand | unimplemented | <length [0,∞]>{1,2} | 0px 0px | yes | two absolute lengths | by computed value |
| border-start-end-radius | longhand | unimplemented | <border-radius> | 0 | no | pair of computed <length-percentage> values | by computed value |
| border-start-start-radius | longhand | unimplemented | <border-radius> | 0 | no | pair of computed <length-percentage> values | by computed value |
| border-style | shorthand | partial | <'border-top-style'>{1,4} | see individual properties | see individual properties | see individual properties | see individual properties |
| border-top | shorthand | partial | <line-width> \|\| <line-style> \|\| <color> | See individual properties | no | see individual properties | see individual properties |
| border-top-clip | longhand | unimplemented | none \| [ <length-percentage [0,∞]> \| <flex> ]+ | none | no | none, or a list consisting of absolute lengths, or percentages as specified | by computed value |
| border-top-color | longhand | partial | <color> \| <image-1D> | currentcolor | no | the computed color and/or a one-dimensional image function | see prose |
| border-top-left-radius | longhand | unimplemented | <border-radius> | 0 | no | pair of computed <length-percentage> values | by computed value |
| border-top-radius | shorthand | unimplemented | <length-percentage [0,∞]>{1,2} [ / <length-percentage [0,∞]>{1,2} ]? | 0 | no | see individual properties | see individual properties |
| border-top-right-radius | longhand | unimplemented | <border-radius> | 0 | no | pair of computed <length-percentage> values | by computed value |
| border-top-style | longhand | partial | <line-style> | none | no | specified keyword | discrete |
| border-top-width | longhand | partial | <line-width> | medium | no | absolute length, snapped as a border width | by computed value |
| border-width | shorthand | partial | <'border-top-width'>{1,4} | see individual properties | see individual properties | see individual properties | see individual properties |
| bottom | longhand | unimplemented | auto \| <length-percentage> \| <anchor()> \| <anchor-size()> | auto | no | the keyword auto or a computed <length-percentage> value | by computed value type |
| box-decoration-break | longhand | unimplemented | slice \| clone | slice | no | specified keyword | discrete |
| box-shadow | shorthand | unimplemented | <spread-shadow># | none | no | see individual properties | see individual properties |
| box-shadow-blur | longhand | unimplemented | <length [0,∞]># | 0 | no | list, each item a <length> | by computed value |
| box-shadow-color | longhand | unimplemented | <color># | currentcolor | no | list, each item a computed color | by computed value |
| box-shadow-offset | longhand | unimplemented | [ none \| <length>{1,2} ]# | none | no | list, each item either none or a pair of offsets (horizontal and vertical) from the element‘s box | by computed value, treating none as 0 0 when interpolated with non-none values. |
| box-shadow-position | longhand | unimplemented | [ outset \| inset ]# | outset | no | list, each item one of the keywords | by computed value |
| box-shadow-spread | longhand | unimplemented | <length># | 0 | no | list, each item a <length> | by computed value |
| box-sizing | longhand | partial | content-box \| border-box | content-box | no | specified keyword | discrete |
| box-snap | longhand | unimplemented | none \| block-start \| block-end \| center \| baseline \| last-baseline | none | yes | as specified | discrete |
| break-after | longhand | unimplemented | auto \| avoid \| always \| all \| avoid-page \| page \| left \| right \| recto \| verso \| avoid-column \| column \| avoid-region \| region | auto | no | specified keyword | discrete |
| break-before | longhand | unimplemented | auto \| avoid \| always \| all \| avoid-page \| page \| left \| right \| recto \| verso \| avoid-column \| column \| avoid-region \| region | auto | no | specified keyword | discrete |
| break-inside | longhand | unimplemented | auto \| avoid \| avoid-page \| avoid-column \| avoid-region | auto | no | specified keyword | discrete |
| caption-side | longhand | unimplemented | top \| bottom | top | yes | specified keyword | discrete |
| caret | shorthand | unimplemented | <'caret-color'> \|\| <'caret-animation'> \|\| <'caret-shape'> | auto | yes | see individual properties | see individual properties |
| caret-animation | longhand | unimplemented | auto \| manual | auto | yes | specified keyword | discrete |
| caret-color | longhand | unimplemented | auto \| <color> [auto \| <color>]? | auto | yes | The computed value for auto is auto. For <color> values, see CSS Color 4 § 15. Resolving <color> Values. | by computed value |
| caret-shape | longhand | unimplemented | auto \| bar \| block \| underscore | auto | yes | specified keyword | by computed value |
| clear | longhand | unimplemented | inline-start \| inline-end \| block-start \| block-end \| left \| right \| top \| bottom \| both-inline \| both-block \| both \| none | none | no | specified keyword | discrete |
| clip | longhand | unimplemented | <rect()> \| auto | auto | no | as specified | by computed value |
| clip-path | longhand | unimplemented | <clip-source> \| [ <basic-shape> \|\| <geometry-box> ] \| none | none | no | as specified, but with <url> values made absolute | by computed value |
| clip-rule | longhand | unimplemented | nonzero \| evenodd | nonzero | yes | as specified | discrete |
| color | longhand | partial | <color> | CanvasText | yes | computed color, see resolving color values | by computed value type |
| color-adjust | shorthand | unimplemented | <'print-color-adjust'> | see individual properties | see individual properties | see individual properties | see individual properties |
| color-interpolation | longhand | unimplemented | auto \| sRGB \| linearRGB | sRGB | yes | as specified | discrete |
| color-interpolation-filters | longhand | unimplemented | auto \| sRGB \| linearRGB | linearRGB | yes | as specified | discrete |
| color-scheme | longhand | unimplemented | normal \| [ light \| dark \| <custom-ident> ]+ && only? | normal | yes | the keyword normal, or a color scheme support | discrete |
| column-count | longhand | unimplemented | auto \| <integer [1,∞]> | auto | no | specified value | by computed value |
| column-fill | longhand | unimplemented | auto \| balance \| balance-all | balance | no | specified keyword | discrete |
| column-gap | longhand | unimplemented | normal \| <length-percentage [0,∞]> \| <line-width> | normal | no | specified keyword, else a computed <length-percentage> value | by computed value type |
| column-height | longhand | unimplemented | auto \| <length [0,∞]> | auto | no | the keyword auto or an absolute length | by computed value type |
| column-rule | shorthand | unimplemented | <gap-rule-list> \| <gap-auto-rule-list> | see individual properties | see individual properties | see individual properties | see individual properties |
| column-rule-break | longhand | unimplemented | none \| normal \| intersection | normal | no | as specified | discrete |
| column-rule-color | longhand | unimplemented | <line-color-list> \| <auto-line-color-list> | currentcolor | no | as specified | repeatable list, see § 4.7 Interpolation of list values. |
| column-rule-inset | shorthand | unimplemented | <'column-rule-inset-cap'> [ / <'column-rule-inset-junction'> ]? | see individual properties | see individual properties | see individual properties | see individual properties |
| column-rule-inset-cap | shorthand | unimplemented | <inset-value> <inset-value>? | see individual properties | see individual properties | see individual properties | see individual properties |
| column-rule-inset-cap-end | longhand | unimplemented | <inset-value> | 0 | no | as specified | by computed value type |
| column-rule-inset-cap-start | longhand | unimplemented | <inset-value> | 0 | no | as specified | by computed value type |
| column-rule-inset-end | shorthand | unimplemented | <inset-value> | see individual properties | see individual properties | see individual properties | see individual properties |
| column-rule-inset-junction | shorthand | unimplemented | <inset-value> <inset-value>? | see individual properties | see individual properties | see individual properties | see individual properties |
| column-rule-inset-junction-end | longhand | unimplemented | <inset-value> | 0 | no | as specified | by computed value type |
| column-rule-inset-junction-start | longhand | unimplemented | <inset-value> | 0 | no | as specified | by computed value type |
| column-rule-inset-start | shorthand | unimplemented | <inset-value> | see individual properties | see individual properties | see individual properties | see individual properties |
| column-rule-style | longhand | unimplemented | <line-style-list> \| <auto-line-style-list> | none | no | as specified | discrete |
| column-rule-visibility-items | longhand | unimplemented | all \| around \| between \| normal | normal | no | as specified | discrete |
| column-rule-width | longhand | unimplemented | <line-width-list> \| <auto-line-width-list> | medium | no | list of absolute lengths, snapped as a border width | repeatable list, see § 4.7 Interpolation of list values. |
| column-span | longhand | unimplemented | none \| <integer [1,∞]> \| all \| auto | none | no | specified value | discrete |
| column-width | longhand | unimplemented | auto \| <box-size> | (missing in snapshot) | (missing in snapshot) | as specified, with <length-percentage> values computed | by computed value type |
| column-wrap | longhand | unimplemented | auto \| nowrap \| wrap | auto | no | specified keyword | discrete |
| columns | shorthand | unimplemented | [ <'column-width'> \|\| <'column-count'> ] [ / <'column-height'> ]? | see individual properties | see individual properties | see individual properties | see individual properties |
| contain | longhand | unimplemented | none \| strict \| content \| [ [size \| inline-size] \|\| layout \|\| style \|\| paint ] | none | no | the keyword none or one or more of size, layout, style, paint | not animatable |
| contain-intrinsic-block-size | longhand | unimplemented | auto? [ none \| <length [0,∞]> ] | none | no | as specified, with <length> values computed | by computed value type |
| contain-intrinsic-height | longhand | unimplemented | auto? [ none \| <length [0,∞]> ] | none | no | as specified, with <length> values computed | by computed value type |
| contain-intrinsic-inline-size | longhand | unimplemented | auto? [ none \| <length [0,∞]> ] | none | no | as specified, with <length> values computed | by computed value type |
| contain-intrinsic-size | shorthand | unimplemented | [ auto? [ none \| <length [0,∞]> ] ]{1,2} | see individual properties | see individual properties | see individual properties | see individual properties |
| contain-intrinsic-width | longhand | unimplemented | auto? [ none \| <length [0,∞]> ] | none | no | as specified, with <length> values computed | by computed value type |
| container | shorthand | partial | <'container-name'> [ / <'container-type'> ]? | see individual properties | see individual properties | see individual properties | see individual properties |
| container-name | longhand | partial | none \| <custom-ident>+ | none | no | the keyword none, or an ordered list of identifiers | not animatable |
| container-type | longhand | partial | normal \| [ [ size \| inline-size ] \|\| scroll-state ] | normal | no | specified keyword | not animatable |
| content | longhand | partial | normal \| none \| [ <content-replacement> \| <content-list> ] [/ [ <string> \| <counter> \| <attr()> ]+ ]? \| <element()> | normal | no | See prose below | discrete |
| content-visibility | longhand | unimplemented | visible \| auto \| hidden | visible | no | as specified | see § 4.1 Animating and Interpolating content-visibility |
| continue | longhand | unimplemented | auto \| discard \| collapse \| -webkit-legacy \| overflow \| paginate \| fragments | auto | no | specified keyword | discrete |
| copy-into | longhand | unimplemented | none \| [ [ <custom-ident> <content-level>] [, <custom-ident> <content-level>]* ]? | none | no | as specified | discrete |
| corner | shorthand | unimplemented | <'border-radius'> \|\| <'corner-shape'> | 0 | no | see individual properties | see individual properties |
| corner-block-end | shorthand | unimplemented | <'border-top-radius'> \|\| <'corner-top-shape'> | 0 | no | see individual properties | see individual properties |
| corner-block-end-shape | shorthand | unimplemented | <'corner-top-left-shape'>{1,2} | see individual properties | see individual properties | see individual properties | see individual properties |
| corner-block-start | shorthand | unimplemented | <'border-top-radius'> \|\| <'corner-top-shape'> | 0 | no | see individual properties | see individual properties |
| corner-block-start-shape | shorthand | unimplemented | <'corner-top-left-shape'>{1,2} | see individual properties | see individual properties | see individual properties | see individual properties |
| corner-bottom | shorthand | unimplemented | <'border-top-radius'> \|\| <'corner-top-shape'> | 0 | no | see individual properties | see individual properties |
| corner-bottom-left | shorthand | unimplemented | <'border-top-left-radius'> \|\| <'corner-top-left-shape'> | 0 | no | see individual properties | see individual properties |
| corner-bottom-left-shape | longhand | unimplemented | <corner-shape-value> | round | no | the corresponding superellipse() value | see superellipse interpolation |
| corner-bottom-right | shorthand | unimplemented | <'border-top-left-radius'> \|\| <'corner-top-left-shape'> | 0 | no | see individual properties | see individual properties |
| corner-bottom-right-shape | longhand | unimplemented | <corner-shape-value> | round | no | the corresponding superellipse() value | see superellipse interpolation |
| corner-bottom-shape | shorthand | unimplemented | <'corner-top-left-shape'>{1,2} | see individual properties | see individual properties | see individual properties | see individual properties |
| corner-end-end | shorthand | unimplemented | <'border-top-left-radius'> \|\| <'corner-top-left-shape'> | 0 | no | see individual properties | see individual properties |
| corner-end-end-shape | longhand | unimplemented | <corner-shape-value> | round | no | the corresponding superellipse() value | see superellipse interpolation |
| corner-end-start | shorthand | unimplemented | <'border-top-left-radius'> \|\| <'corner-top-left-shape'> | 0 | no | see individual properties | see individual properties |
| corner-end-start-shape | longhand | unimplemented | <corner-shape-value> | round | no | the corresponding superellipse() value | see superellipse interpolation |
| corner-inline-end | shorthand | unimplemented | <'border-top-radius'> \|\| <'corner-top-shape'> | 0 | no | see individual properties | see individual properties |
| corner-inline-end-shape | shorthand | unimplemented | <'corner-top-left-shape'>{1,2} | see individual properties | see individual properties | see individual properties | see individual properties |
| corner-inline-start | shorthand | unimplemented | <'border-top-radius'> \|\| <'corner-top-shape'> | 0 | no | see individual properties | see individual properties |
| corner-inline-start-shape | shorthand | unimplemented | <'corner-top-left-shape'>{1,2} | see individual properties | see individual properties | see individual properties | see individual properties |
| corner-left | shorthand | unimplemented | <'border-top-radius'> \|\| <'corner-top-shape'> | 0 | no | see individual properties | see individual properties |
| corner-left-shape | shorthand | unimplemented | <'corner-top-left-shape'>{1,2} | see individual properties | see individual properties | see individual properties | see individual properties |
| corner-right | shorthand | unimplemented | <'border-top-radius'> \|\| <'corner-top-shape'> | 0 | no | see individual properties | see individual properties |
| corner-right-shape | shorthand | unimplemented | <'corner-top-left-shape'>{1,2} | see individual properties | see individual properties | see individual properties | see individual properties |
| corner-shape | shorthand | unimplemented | <'corner-top-left-shape'>{1,4} | round | no | see individual properties | see individual properties |
| corner-start-end | shorthand | unimplemented | <'border-top-left-radius'> \|\| <'corner-top-left-shape'> | 0 | no | see individual properties | see individual properties |
| corner-start-end-shape | longhand | unimplemented | <corner-shape-value> | round | no | the corresponding superellipse() value | see superellipse interpolation |
| corner-start-start | shorthand | unimplemented | <'border-top-left-radius'> \|\| <'corner-top-left-shape'> | 0 | no | see individual properties | see individual properties |
| corner-start-start-shape | longhand | unimplemented | <corner-shape-value> | round | no | the corresponding superellipse() value | see superellipse interpolation |
| corner-top | shorthand | unimplemented | <'border-top-radius'> \|\| <'corner-top-shape'> | 0 | no | see individual properties | see individual properties |
| corner-top-left | shorthand | unimplemented | <'border-top-left-radius'> \|\| <'corner-top-left-shape'> | 0 | no | see individual properties | see individual properties |
| corner-top-left-shape | longhand | unimplemented | <corner-shape-value> | round | no | the corresponding superellipse() value | see superellipse interpolation |
| corner-top-right | shorthand | unimplemented | <'border-top-left-radius'> \|\| <'corner-top-left-shape'> | 0 | no | see individual properties | see individual properties |
| corner-top-right-shape | longhand | unimplemented | <corner-shape-value> | round | no | the corresponding superellipse() value | see superellipse interpolation |
| corner-top-shape | shorthand | unimplemented | <'corner-top-left-shape'>{1,2} | see individual properties | see individual properties | see individual properties | see individual properties |
| counter-increment | longhand | unimplemented | [ <counter-name> <integer>? ]+ \| none | none | no | the keyword none or a list, each item an identifier paired with an integer | by computed value type |
| counter-reset | longhand | unimplemented | [ <counter-name> <integer>? \| <reversed-counter-name> <integer>? ]+ \| none | none | no | the keyword none or a list, each item an identifier or a reversed() function paired with an integer | by computed value type |
| counter-set | longhand | unimplemented | [ <counter-name> <integer>? ]+ \| none | none | no | the keyword none or a list, each item an identifier paired with an integer | by computed value type |
| cue | shorthand | unimplemented | <'cue-before'> <'cue-after'>? | see individual properties | no | see individual properties | see individual properties |
| cue-after | longhand | unimplemented | <url> <decibel>? \| none | none | no | specified value | by computed value type |
| cue-before | longhand | unimplemented | <url> <decibel>? \| none | none | no | specified value | by computed value type |
| cursor | longhand | unimplemented | [<cursor-image>,]* <cursor-predefined> | auto | yes | as specified, except with any relative URLs converted to absolute | discrete |
| cx | longhand | unimplemented | <length-percentage> | 0 | no | an absolute length or percentage | by computed value |
| cy | longhand | unimplemented | <length-percentage> | 0 | no | an absolute length or percentage | by computed value |
| d | longhand | unimplemented | none \| <string> | none | no | as specified | See prose |
| direction | longhand | partial | ltr \| rtl | ltr | yes | specified value | not animatable |
| display | longhand | partial | [ <display-outside> \|\| <display-inside> ] \| <display-listitem> \| <display-internal> \| <display-box> \| <display-legacy> \| grid-lanes \| inline-grid-lanes \| <display-outside> \|\| [ <display-inside> \| math ] | inline | no | a pair of keywords representing the inner and outer display types plus optional list-item flag, or a <display-internal> or <display-box> keyword; see prose in a variety of specs for computation rules | see § 2.9 Animating and Interpolating display |
| dominant-baseline | longhand | unimplemented | auto \| <baseline-metric> | auto | yes | specified keyword | discrete |
| dynamic-range-limit | longhand | unimplemented | standard \| no-limit \| constrained \| <dynamic-range-limit-mix()> | no-limit | yes | see Computed Value for dynamic-range-limit | by dynamic-range-limit-mix() |
| empty-cells | longhand | unimplemented | show \| hide | show | yes | specified keyword | discrete |
| event-trigger | longhand | unimplemented | none \| [ <'event-trigger-name'> <'event-trigger-source'> ]# | none | no | as specified | not animatable |
| event-trigger-name | longhand | unimplemented | none \| <dashed-ident># | none | no | as specified | not animatable |
| event-trigger-source | longhand | unimplemented | [ none \| <event-trigger-event>+ [ / <event-trigger-event>+ ]? ]# | none | no | as specified | not animatable |
| field-sizing | longhand | unimplemented | fixed \| content | fixed | no | as specified | discrete |
| fill | longhand | unimplemented | <paint> | black | yes | as specified, but with <color> values computed and <url> values made absolute | by computed value |
| fill-break | longhand | unimplemented | bounding-box \| slice \| clone | bounding-box | yes? | as specified | discrete |
| fill-color | longhand | unimplemented | <color> | currentcolor | yes | the computed color | by computed value |
| fill-image | longhand | unimplemented | <paint># | none | yes | as specified, with any <image> computed | repeatable list |
| fill-opacity | longhand | unimplemented | <'opacity'> | 1 | yes | the specified value converted to a <number>, clamped to the range [0,1] | by computed value |
| fill-origin | longhand | unimplemented | match-parent \| fill-box \| stroke-box \| content-box \| padding-box \| border-box | match-parent | no | as specified | discrete |
| fill-position | longhand | unimplemented | <position># | 0% 0% | yes | A list, each item consisting of: a pair of offsets (horizontal and vertical) from the top left origin each given as a combination of an absolute length and a percentage | repeatable list |
| fill-repeat | longhand | unimplemented | <repeat-style># | repeat | yes | A list, each item consisting of: two keywords, one per dimension | discrete |
| fill-rule | longhand | unimplemented | nonzero \| evenodd | nonzero | yes | as specified | discrete |
| fill-size | longhand | unimplemented | <bg-size># | auto | yes | as specified, but with lengths made absolute and omitted auto keywords filled in | repeatable list |
| filter | longhand | unimplemented | none \| <filter-value-list> | none | no | as specified | See prose in Animation of Filters. |
| flex | shorthand | unimplemented | none \| [ <'flex-grow'> <'flex-shrink'>? \|\| <'flex-basis'> ] | 0 1 auto | no | see individual properties | by computed value type |
| flex-basis | longhand | unimplemented | content \| <'width'> | auto | no | specified keyword or a computed <length-percentage> value | by computed value type |
| flex-direction | longhand | unimplemented | row \| row-reverse \| column \| column-reverse | row | no | specified keyword | discrete |
| flex-flow | shorthand | unimplemented | <'flex-direction'> \|\| <'flex-wrap'> | see individual properties | see individual properties | see individual properties | see individual properties |
| flex-grow | longhand | unimplemented | <number [0,∞]> | 0 | no | specified number | by computed value type |
| flex-line-count | longhand | unimplemented | <integer [1,∞]> | 1 | no | the specified integer, computed | as integer |
| flex-shrink | longhand | unimplemented | <number [0,∞]> | 1 | no | specified value | number |
| flex-wrap | longhand | unimplemented | nowrap \| [ wrap \| wrap-reverse ] \|\| balance | nowrap | no | specified keyword | discrete |
| float | longhand | unimplemented | block-start \| block-end \| inline-start \| inline-end \| snap-block \| <snap-block()> \| snap-inline \| <snap-inline()> \| left \| right \| top \| bottom \| none \| footnote | none | no | as specified | by computed value type |
| float-defer | longhand | unimplemented | <integer> \| last \| none | none | no | specified keyword or integer | discrete |
| float-offset | longhand | unimplemented | <length-percentage> | 0 | no | computed <length-percentage> value | by computed value type |
| float-reference | longhand | unimplemented | inline \| column \| region \| page | inline | no | specified keyword | discrete |
| flood-color | longhand | unimplemented | <color> | black | no | as specified | by computed value |
| flood-opacity | longhand | unimplemented | <'opacity'> | 1 | no | the specified value converted to a number, clamped to the range [0,1] | by computed value |
| flow-from | longhand | unimplemented | <custom-ident> \| none | none | no | as specified | not animatable |
| flow-into | longhand | unimplemented | none \| <custom-ident> [element \| content]? | none | no | as specified | not animatable |
| flow-tolerance | longhand | unimplemented | normal \| <length-percentage [0,∞]> \| infinite | normal | no | a computed <length-percentage> value | as length |
| font | shorthand | partial | [ [ <'font-style'> \|\| <font-variant-css2> \|\| <'font-weight'> \|\| <font-width-css3> ]? <'font-size'> [ / <'line-height'> ]? <'font-family'># ] \| <system-font-family-name> | see individual properties | yes | see individual properties | see individual properties |
| font-family | longhand | partial | [ <font-family-name> \| <generic-font-family> ]# | depends on user agent | yes | list, each item a string and/or <generic-font-family> keywords | discrete |
| font-feature-settings | longhand | unimplemented | normal \| <feature-tag-value># | normal | yes | as specified | discrete |
| font-kerning | longhand | unimplemented | auto \| normal \| none | auto | yes | as specified | discrete |
| font-language-override | longhand | unimplemented | normal \| <string> | normal | yes | specified string or the keyword none | discrete |
| font-optical-sizing | longhand | unimplemented | auto \| none | auto | yes | specified keyword | discrete |
| font-palette | longhand | unimplemented | normal \| light \| dark \| <palette-identifier> \| <palette-mix()> | normal | yes | specified keyword, identifier or <palette-mix()> function. <palette-mix()> must be simplified to a single keyword or identifier if resulting palette is equivalent. | by computed value |
| font-size | longhand | partial | <absolute-size> \| <relative-size> \| <length-percentage [0,∞]> \| math | medium | yes | an absolute length | by computed value type |
| font-size-adjust | longhand | unimplemented | none \| [ ex-height \| cap-height \| ch-width \| ic-width \| ic-height ]? [ from-font \| <number [0,∞]> ] | none | yes | the keyword none, or a pair of a metric keyword and a <number> | discrete if the keywords differ, otherwise by computed value type |
| font-stretch | alias of font-width | alias unimplemented | normal \| <percentage [0,∞]> \| ultra-condensed \| extra-condensed \| condensed \| semi-condensed \| semi-expanded \| expanded \| extra-expanded \| ultra-expanded | (missing in snapshot) | (missing in snapshot) | (missing in snapshot) | (missing in snapshot) |
| font-style | longhand | partial | normal \| italic \| left \| right \| oblique <angle [-90deg,90deg]>? | normal | yes | the keyword specified, plus angle in degrees if specified | by computed value type; normal animates as oblique 0deg |
| font-synthesis | shorthand | unimplemented | none \| [ weight \|\| style \|\| small-caps \|\| position] | weight style small-caps position | yes | specified keyword(s) | discrete |
| font-synthesis-position | longhand | unimplemented | auto \| none | auto | yes | specified keyword | discrete |
| font-synthesis-small-caps | longhand | unimplemented | auto \| none | auto | yes | specified keyword | discrete |
| font-synthesis-style | longhand | unimplemented | auto \| none \| oblique-only | auto | yes | specified keyword | discrete |
| font-synthesis-weight | longhand | unimplemented | auto \| none | auto | yes | specified keyword | discrete |
| font-variant | shorthand | unimplemented | normal \| none \| [ [ <common-lig-values> \|\| <discretionary-lig-values> \|\| <historical-lig-values> \|\| <contextual-alt-values> ] \|\| [ small-caps \| all-small-caps \| petite-caps \| all-petite-caps \| unicase \| titling-caps ] \|\| [ stylistic(<font-feature-value-name>) \|\| historical-forms \|\| styleset(<font-feature-value-name>#) \|\| character-variant(<font-feature-value-name>#) \|\| swash(<font-feature-value-name>) \|\| ornaments(<font-feature-value-name>) \|\| annotation(<font-feature-value-name>) ] \|\| [ <numeric-figure-values> \|\| <numeric-spacing-values> \|\| <numeric-fraction-values> \|\| ordinal \|\| slashed-zero ] \|\| [ <east-asian-variant-values> \|\| <east-asian-width-values> \|\| ruby ] \|\| [ sub \| super ] \|\| [ text \| emoji \| unicode ] ] | normal | yes | as specified | discrete |
| font-variant-alternates | longhand | unimplemented | normal \| [ stylistic(<font-feature-value-name>) \|\| historical-forms \|\| styleset(<font-feature-value-name>#) \|\| character-variant(<font-feature-value-name>#) \|\| swash(<font-feature-value-name>) \|\| ornaments(<font-feature-value-name>) \|\| annotation(<font-feature-value-name>) ] | normal | yes | as specified | discrete |
| font-variant-caps | longhand | unimplemented | normal \| small-caps \| all-small-caps \| petite-caps \| all-petite-caps \| unicase \| titling-caps | normal | yes | as specified | discrete |
| font-variant-east-asian | longhand | unimplemented | normal \| [ <east-asian-variant-values> \|\| <east-asian-width-values> \|\| ruby ] | normal | yes | as specified | discrete |
| font-variant-emoji | longhand | unimplemented | normal \| text \| emoji \| unicode | normal | yes | specified keyword | discrete |
| font-variant-ligatures | longhand | unimplemented | normal \| none \| [ <common-lig-values> \|\| <discretionary-lig-values> \|\| <historical-lig-values> \|\| <contextual-alt-values> ] | normal | yes | as specified | discrete |
| font-variant-numeric | longhand | unimplemented | normal \| [ <numeric-figure-values> \|\| <numeric-spacing-values> \|\| <numeric-fraction-values> \|\| ordinal \|\| slashed-zero ] | normal | yes | as specified | discrete |
| font-variant-position | longhand | unimplemented | normal \| sub \| super | normal | yes | as specified | discrete |
| font-variation-settings | longhand | unimplemented | normal \| [ <opentype-tag> <number> ]# | normal | yes | the keyword normal or a list, each item a string paired with a number | see prose |
| font-weight | longhand | partial | <font-weight-absolute> \| bolder \| lighter | normal | yes | a number, see below | by computed value type |
| font-width | longhand | unimplemented | normal \| <percentage [0,∞]> \| ultra-condensed \| extra-condensed \| condensed \| semi-condensed \| semi-expanded \| expanded \| extra-expanded \| ultra-expanded | normal | yes | a percentage, see below | by computed value type |
| footnote-display | longhand | unimplemented | block \| inline \| compact | block | no | specified value | discrete |
| footnote-policy | longhand | unimplemented | auto \| line \| block | auto | no | specified value | discrete |
| forced-color-adjust | longhand | unimplemented | auto \| none \| preserve-parent-color | auto | yes | as specified | not animatable |
| frame-sizing | longhand | unimplemented | auto \| content-width \| content-height \| content-block-size \| content-inline-size | auto | no | as specified | discrete |
| gap | shorthand | unimplemented | <'row-gap'> <'column-gap'>? | see individual properties | no | see individual properties | by computed value type |
| glyph-orientation-vertical | longhand | unimplemented | auto \| 0deg \| 90deg \| 0 \| 90 | n/a | n/a | n/a | n/a |
| grid | shorthand | unimplemented | <'grid-template'> \| <'grid-template-rows'> / [ auto-flow && dense? ] <'grid-auto-columns'>? \| [ auto-flow && dense? ] <'grid-auto-rows'>? / <'grid-template-columns'> | none | see individual properties | see individual properties | see individual properties |
| grid-area | shorthand | unimplemented | <grid-line> [ / <grid-line> ]{0,3} | auto | no | see individual properties | discrete |
| grid-auto-columns | longhand | unimplemented | <track-size>+ | auto | no | see Track Sizing | if the list lengths match, by computed value type per item; discrete otherwise |
| grid-auto-flow | longhand | unimplemented | [ row \| column ] \|\| dense | row | no | specified keyword(s) | discrete |
| grid-auto-rows | longhand | unimplemented | <track-size>+ | auto | no | see Track Sizing | if the list lengths match, by computed value type per item; discrete otherwise |
| grid-column | shorthand | unimplemented | <grid-line> [ / <grid-line> ]? | auto | no | see individual properties | discrete |
| grid-column-end | longhand | unimplemented | <grid-line> | auto | no | specified keyword, identifier, and/or integer | discrete |
| grid-column-gap | alias of column-gap | alias unimplemented | normal \| <length-percentage [0,∞]> \| <line-width> | (missing in snapshot) | (missing in snapshot) | (missing in snapshot) | (missing in snapshot) |
| grid-column-start | longhand | unimplemented | <grid-line> | auto | no | specified keyword, identifier, and/or integer | discrete |
| grid-gap | alias of gap | alias unimplemented | <'row-gap'> <'column-gap'>? | (missing in snapshot) | (missing in snapshot) | (missing in snapshot) | (missing in snapshot) |
| grid-row | shorthand | unimplemented | <grid-line> [ / <grid-line> ]? | auto | no | see individual properties | discrete |
| grid-row-end | longhand | unimplemented | <grid-line> | auto | no | specified keyword, identifier, and/or integer | discrete |
| grid-row-gap | alias of row-gap | alias unimplemented | normal \| <length-percentage [0,∞]> \| <line-width> | (missing in snapshot) | (missing in snapshot) | (missing in snapshot) | (missing in snapshot) |
| grid-row-start | longhand | unimplemented | <grid-line> | auto | no | specified keyword, identifier, and/or integer | discrete |
| grid-template | shorthand | unimplemented | none \| [ <'grid-template-rows'> / <'grid-template-columns'> ] \| [ <line-names>? <string> <track-size>? <line-names>? ]+ [ / <explicit-track-list> ]? | none | see individual properties | see individual properties | see individual properties |
| grid-template-areas | longhand | unimplemented | none \| <string>+ | none | no | the keyword none or a list of string values | discrete |
| grid-template-columns | longhand | unimplemented | none \| <track-list> \| <auto-track-list> \| subgrid <line-name-list>? | none | no | the keyword none or a computed track list | if the list lengths match, by computed value type per item in the computed track list (see § 7.2.5 Computed Value of a Track Listing and § 7.2.3.3 Interpolation/Combination of repeat()); discrete otherwise |
| grid-template-rows | longhand | unimplemented | none \| <track-list> \| <auto-track-list> \| subgrid <line-name-list>? | none | no | the keyword none or a computed track list | if the list lengths match, by computed value type per item in the computed track list (see § 7.2.5 Computed Value of a Track Listing and § 7.2.3.3 Interpolation/Combination of repeat()); discrete otherwise |
| hanging-punctuation | longhand | unimplemented | none \| [ first \|\| [ force-end \| allow-end ] \|\| last ] | none | yes | specified keyword(s) | discrete |
| height | longhand | partial | auto \| <box-size> \| <anchor-size()> | auto | no | as specified, with <length-percentage> values computed | by computed value type, recursing into fit-content() |
| hyphenate-character | longhand | unimplemented | auto \| <string> | auto | yes | specified keyword | discrete |
| hyphenate-limit-chars | longhand | unimplemented | [ auto \| <integer [0,∞]> ]{1,3} | auto | yes | three values, each either the auto keyword or an integer | by computed value type |
| hyphenate-limit-last | longhand | unimplemented | none \| always \| column \| page \| spread | none | yes | specified keyword | discrete |
| hyphenate-limit-lines | longhand | unimplemented | no-limit \| <integer [0,∞]> | no-limit | yes | specified keyword or integer | by computed value type |
| hyphenate-limit-zone | longhand | unimplemented | <length-percentage> | 0 | yes | computed <length-percentage> value | by computed value type |
| hyphens | longhand | unimplemented | none \| manual \| auto | manual | yes | specified keyword | discrete |
| image-animation | longhand | unimplemented | normal \| paused \| stopped \| running | normal | yes | as specified | discrete |
| image-orientation | longhand | unimplemented | from-image \| none \| [ <angle> \|\| flip ] | from-image | yes | the specified keyword, or an <angle>, rounded and normalized (see text), plus optionally a flip keyword | discrete |
| image-rendering | longhand | unimplemented | auto \| smooth \| high-quality \| pixelated \| crisp-edges | auto | yes | specified keyword | discrete |
| image-resolution | longhand | unimplemented | [ from-image \|\| <resolution> ] && snap? | 1dppx | yes | specified keyword(s) and/or <resolution> (possibly adjusted for snap, see below) | discrete |
| initial-letter | longhand | unimplemented | normal \| <number [1,∞]> <integer [1,∞]> \| <number [1,∞]> && [ drop \| raise ]? | normal | no | the keyword normal or a number paired with an integer | by computed value type |
| initial-letter-align | longhand | unimplemented | [ border-box? [ alphabetic \| ideographic \| hanging \| leading ]? ]! | alphabetic | yes | specified keyword(s) | discrete |
| initial-letter-wrap | longhand | unimplemented | none \| first \| all \| grid \| <length-percentage> | none | yes | specified keyword or computed <length-percentage> value | by computed value type |
| inline-size | longhand | unimplemented | auto \| <box-size> \| <anchor-size()> | auto | no | as specified, with <length-percentage> values computed | by computed value type, recursing into fit-content() |
| inline-sizing | longhand | unimplemented | normal \| stretch | normal | yes | specified keyword | discrete |
| input-security | longhand | unimplemented | auto \| none | auto | no | as specified | by computed value type |
| inset | shorthand | unimplemented | <'top'>{1,4} | auto | no | see individual properties | by computed value type |
| inset-block | shorthand | unimplemented | <'top'>{1,2} | auto | no | see individual properties | by computed value type |
| inset-block-end | longhand | unimplemented | auto \| <length-percentage> | auto | no | the keyword auto or a computed <length-percentage> value | by computed value type |
| inset-block-start | longhand | unimplemented | auto \| <length-percentage> | auto | no | the keyword auto or a computed <length-percentage> value | by computed value type |
| inset-inline | shorthand | unimplemented | <'top'>{1,2} | auto | no | see individual properties | by computed value type |
| inset-inline-end | longhand | unimplemented | auto \| <length-percentage> | auto | no | the keyword auto or a computed <length-percentage> value | by computed value type |
| inset-inline-start | longhand | unimplemented | auto \| <length-percentage> | auto | no | the keyword auto or a computed <length-percentage> value | by computed value type |
| interactivity | longhand | unimplemented | auto \| inert | auto | yes | as specified | discrete |
| interest-delay | shorthand | unimplemented | <'interest-delay-start'>{1,2} | see individual properties | see individual properties | see individual properties | see individual properties |
| interest-delay-end | longhand | unimplemented | normal \| <time> | normal | yes | the keyword normal or a computed time | by computed value type |
| interest-delay-start | longhand | unimplemented | normal \| <time> | normal | yes | the keyword normal or a computed time | by computed value type |
| interpolate-size | longhand | unimplemented | numeric-only \| allow-keywords | numeric-only | yes | as specified | not animatable |
| isolation | longhand | unimplemented | <isolation-mode> | auto | no | as specified | discrete |
| justify-content | longhand | unimplemented | normal \| <content-distribution> \| <overflow-position>? [ <content-position> \| left \| right ] | normal | no | specified keyword(s) | discrete |
| justify-items | longhand | unimplemented | normal \| stretch \| <baseline-position> \| <overflow-position>? [ <self-position> \| left \| right ] \| legacy \| legacy && [ left \| right \| center ] | legacy | no | specified keyword(s), except for legacy (see prose) | discrete |
| justify-self | longhand | unimplemented | auto \| <overflow-position>? [ normal \| <self-position> \| left \| right ] \| stretch \| <baseline-position> \| anchor-center | auto | no | specified keyword(s) | discrete |
| left | longhand | unimplemented | auto \| <length-percentage> \| <anchor()> \| <anchor-size()> | auto | no | the keyword auto or a computed <length-percentage> value | by computed value type |
| letter-spacing | longhand | unimplemented | normal \| <length-percentage> | normal | yes | an absolute length and/or a percentage | by computed value type |
| lighting-color | longhand | unimplemented | <color> | white | no | as specified | by computed value |
| line-break | longhand | unimplemented | auto \| loose \| normal \| strict \| anywhere | auto | yes | specified keyword | discrete |
| line-clamp | shorthand | unimplemented | none \| [<integer [1,∞]> \|\| <'block-ellipsis'>] -webkit-legacy? | none | see individual properties | see individual properties | see individual properties |
| line-fit-edge | longhand | unimplemented | leading \| <text-edge> | leading | yes | the specified keyword | discrete |
| line-grid | longhand | unimplemented | match-parent \| create | match-parent | no | specified keyword | discrete |
| line-height | longhand | partial | normal \| <number [0,∞]> \| <length-percentage [0,∞]> | normal | yes | the specified keyword, a number, or a computed <length> value | by computed value type |
| line-height-step | longhand | unimplemented | <length [0,∞]> | 0 | yes | absolute length | by computed value type |
| line-padding | longhand | unimplemented | <length> | 0 | yes | absolute length | by computed value type |
| line-snap | longhand | unimplemented | none \| baseline \| contain | none | yes | specified keyword | discrete |
| link-parameters | longhand | unimplemented | none \| <param()># | none | no | as specified | discrete |
| list-style | shorthand | unimplemented | <'list-style-position'> \|\| <'list-style-image'> \|\| <'list-style-type'> | see individual properties | see individual properties | see individual properties | see individual properties |
| list-style-image | longhand | unimplemented | <image> \| none | none | yes | the keyword noneor the computed <image> | discrete |
| list-style-position | longhand | unimplemented | inside \| outside | outside | yes | keyword, but see prose | discrete |
| list-style-type | longhand | unimplemented | <counter-style> \| <string> \| none | disc | yes | specified value | discrete |
| margin | shorthand | partial | <'margin-top'>{1,4} | 0 | no | see individual properties | by computed value type |
| margin-block | shorthand | unimplemented | <'margin-top'>{1,2} | see individual properties | see individual properties | see individual properties | see individual properties |
| margin-block-end | longhand | unimplemented | <'margin-top'> | 0 | no | Same as corresponding margin-* properties | by computed value type |
| margin-block-start | longhand | unimplemented | <'margin-top'> | 0 | no | Same as corresponding margin-* properties | by computed value type |
| margin-bottom | longhand | partial | <length-percentage> \| auto \| <anchor-size()> | 0 | no | the keyword auto or a computed <length-percentage> value | by computed value type |
| margin-break | longhand | unimplemented | auto \| keep \| discard | auto | no | specified keyword | discrete |
| margin-inline | shorthand | unimplemented | <'margin-top'>{1,2} | see individual properties | see individual properties | see individual properties | see individual properties |
| margin-inline-end | longhand | unimplemented | <'margin-top'> | 0 | no | Same as corresponding margin-* properties | by computed value type |
| margin-inline-start | longhand | unimplemented | <'margin-top'> | 0 | no | Same as corresponding margin-* properties | by computed value type |
| margin-left | longhand | partial | <length-percentage> \| auto \| <anchor-size()> | 0 | no | the keyword auto or a computed <length-percentage> value | by computed value type |
| margin-right | longhand | partial | <length-percentage> \| auto \| <anchor-size()> | 0 | no | the keyword auto or a computed <length-percentage> value | by computed value type |
| margin-top | longhand | partial | <length-percentage> \| auto \| <anchor-size()> | 0 | no | the keyword auto or a computed <length-percentage> value | by computed value type |
| margin-trim | longhand | unimplemented | none \| block \| [ block-start \|\| block-end ] | none | no | a set of zero to two keywords indicating which sides to trim | discrete |
| marker | shorthand | unimplemented | none \| <marker-ref> | not defined for shorthand properties | yes | see individual properties | discrete |
| marker-end | longhand | unimplemented | none \| <marker-ref> | none | yes | as specified, but with <url> values (that are part of a <marker-ref>) made absolute | discrete |
| marker-mid | longhand | unimplemented | none \| <marker-ref> | none | yes | as specified, but with <url> values (that are part of a <marker-ref>) made absolute | discrete |
| marker-side | longhand | unimplemented | match-self \| match-parent | match-self | yes | specified keyword | discrete |
| marker-start | longhand | unimplemented | none \| <marker-ref> | none | yes | as specified, but with <url> values (that are part of a <marker-ref>) made absolute | discrete |
| mask | shorthand | unimplemented | <mask-layer># | see individual properties | no | see individual properties | see individual properties |
| mask-border | shorthand | unimplemented | <'mask-border-source'> \|\| <'mask-border-slice'> [ / <'mask-border-width'>? [ / <'mask-border-outset'> ]? ]? \|\| <'mask-border-repeat'> \|\| <'mask-border-mode'> | See individual properties | no | See individual properties | See individual properties |
| mask-border-mode | longhand | unimplemented | luminance \| alpha | alpha | no | as specified | discrete |
| mask-border-outset | longhand | unimplemented | <'border-image-outset'> | 0 | no | all <length>s made absolute, otherwise as specified | by computed value |
| mask-border-repeat | longhand | unimplemented | <'border-image-repeat'> | stretch | no | as specified | discrete |
| mask-border-slice | longhand | unimplemented | <'border-image-slice'> | 0 | no | as specified | by computed value |
| mask-border-source | longhand | unimplemented | <'border-image-source'> | none | no | they keyword none or the computed <image> | discrete |
| mask-border-width | longhand | unimplemented | <'border-image-width'> | auto | no | all <length>s made absolute, otherwise as specified | by computed value |
| mask-clip | longhand | unimplemented | [ <coord-box> \| no-clip ]# | border-box | no | list, each item the keyword as specified | discrete |
| mask-composite | longhand | unimplemented | <compositing-operator># | add | no | list, each item the keyword as specified | discrete |
| mask-image | longhand | unimplemented | <mask-reference># | none | no | list, each item the keyword none, a computed <image>, or a computed <url> | discrete |
| mask-mode | longhand | unimplemented | <masking-mode># | match-source | no | list, each item the keyword as specified | discrete |
| mask-origin | longhand | unimplemented | <coord-box># | border-box | no | list, each item the keyword as specified | discrete |
| mask-position | longhand | unimplemented | <position># | 0% 0% | no | list, each item consists of two keywords representing the origin and two offsets from that origin, each given as an absolute length (if given a <length>), otherwise as a percentage. | repeatable list |
| mask-repeat | longhand | unimplemented | <repeat-style># | repeat | no | list, each item a pair of keywords, one per dimension | discrete |
| mask-size | longhand | unimplemented | <bg-size># | auto | no | list, each item as specified, but with lengths made absolute | repeatable list |
| mask-type | longhand | unimplemented | luminance \| alpha | luminance | no | as specified | discrete |
| math-depth | longhand | unimplemented | auto-add \| add(<integer>) \| <integer> | 0 | yes | an integer, see below | by computed value type |
| math-shift | longhand | unimplemented | normal \| compact | normal | yes | specified keyword | by computed value type |
| math-style | longhand | unimplemented | normal \| compact | normal | yes | specified keyword | by computed value type |
| max-block-size | longhand | unimplemented | none \| <box-size> \| <anchor-size()> | none | no | as specified, with <length-percentage> values computed | by computed value, recursing into fit-content() |
| max-height | longhand | unimplemented | none \| <box-size> \| <anchor-size()> | none | no | as specified, with <length-percentage> values computed | by computed value, recursing into fit-content() |
| max-inline-size | longhand | unimplemented | none \| <box-size> \| <anchor-size()> | none | no | as specified, with <length-percentage> values computed | by computed value, recursing into fit-content() |
| max-lines | longhand | unimplemented | none \| <integer [1,∞]> | none | no | the keyword none or an integer | by computed value type |
| max-size | shorthand | unimplemented | <'max-width'> <'max-height'>? | none | see individual properties | see individual properties | see individual properties |
| max-width | longhand | unimplemented | none \| <box-size> \| <anchor-size()> | none | no | as specified, with <length-percentage> values computed | by computed value, recursing into fit-content() |
| min-block-size | longhand | unimplemented | auto \| <box-size> \| <anchor-size()> | auto | no | as specified, with <length-percentage> values computed | by computed value, recursing into fit-content() |
| min-height | longhand | unimplemented | auto \| <box-size> \| <anchor-size()> | auto | no | as specified, with <length-percentage> values computed | by computed value, recursing into fit-content() |
| min-inline-size | longhand | unimplemented | auto \| <box-size> \| <anchor-size()> | auto | no | as specified, with <length-percentage> values computed | by computed value, recursing into fit-content() |
| min-intrinsic-sizing | longhand | unimplemented | legacy \| zero-if-scroll \|\| zero-if-extrinsic | legacy | no | as specified | discrete |
| min-size | shorthand | unimplemented | <'min-width'> <'min-height'>? | auto | see individual properties | see individual properties | see individual properties |
| min-width | longhand | unimplemented | auto \| <box-size> \| <anchor-size()> | auto | no | as specified, with <length-percentage> values computed | by computed value, recursing into fit-content() |
| mix-blend-mode | longhand | unimplemented | <blend-mode> \| plus-lighter | normal | no | as specified | discrete |
| nav-down | longhand | unimplemented | auto \| <id> [ current \| root \| <target-name> ]? | auto | no | as specified | discrete |
| nav-left | longhand | unimplemented | auto \| <id> [ current \| root \| <target-name> ]? | auto | no | as specified | discrete |
| nav-right | longhand | unimplemented | auto \| <id> [ current \| root \| <target-name> ]? | auto | no | as specified | discrete |
| nav-up | longhand | unimplemented | auto \| <id> [ current \| root \| <target-name> ]? | auto | no | as specified | discrete |
| object-fit | longhand | unimplemented | fill \| none \| [contain \| cover] \|\| scale-down | fill | no | specified keyword(s) | discrete |
| object-position | longhand | unimplemented | <position> | 50% 50% | no | as for background-position | as for background-position |
| object-view-box | longhand | unimplemented | none \| <basic-shape-rect> | none | no | specified keyword, or computed <basic-shape> function | as <basic-shape> if possible, otherwise discrete |
| offset | shorthand | unimplemented | [ <'offset-position'>? [ <'offset-path'> [ <'offset-distance'> \|\| <'offset-rotate'> ]? ]? ]! [ / <'offset-anchor'> ]? | see individual properties | see individual properties | see individual properties | see individual properties |
| offset-anchor | longhand | unimplemented | auto \| <position> | auto | no | the auto keyword or a computed <position> | by computed value |
| offset-distance | longhand | unimplemented | <length-percentage> | 0 | no | a computed <length-percentage> value | by computed value |
| offset-path | longhand | unimplemented | none \| <offset-path> \|\| <coord-box> | none | no | as specified | by computed value |
| offset-position | longhand | unimplemented | normal \| auto \| <position> | normal | no | The normal or auto keywords, or a computed <position> | by computed value |
| offset-rotate | longhand | unimplemented | [ auto \| reverse ] \|\| <angle> | auto | no | computed <angle> value, optionally preceded by auto | by computed value |
| opacity | longhand | unimplemented | <opacity-value> | 1 | no | specified number, clamped to the range [0,1] | by computed value type |
| order | longhand | unimplemented | <integer> | 0 | no | specified integer | by computed value type |
| orphans | longhand | unimplemented | <integer [1,∞]> | 2 | yes | specified integer | by computed value type |
| outline | shorthand | unimplemented | <'outline-width'> \|\| <'outline-style'> \|\| <'outline-color'> | see individual properties | no | see individual properties | see individual properties |
| outline-color | longhand | unimplemented | auto \| <'border-top-color'> | auto | no | see below | by computed value |
| outline-offset | longhand | unimplemented | <length> | 0 | no | absolute length | by computed value |
| outline-style | longhand | unimplemented | auto \| <outline-line-style> | none | no | specified keyword | by computed value |
| outline-width | longhand | unimplemented | <line-width> | medium | no | absolute length, snapped as a border width | by computed value |
| overflow | shorthand | unimplemented | <'overflow-block'>{1,2} | visible | no | see individual properties | discrete |
| overflow-anchor | longhand | unimplemented | auto \| none | auto | no | specified keyword | discrete |
| overflow-block | longhand | unimplemented | visible \| hidden \| clip \| scroll \| auto | visible | no | usually specified value, but see text | discrete |
| overflow-clip-margin | shorthand | unimplemented | <visual-box> \|\| <length> | 0px | no | see individual properties | see individual properties |
| overflow-clip-margin-block | shorthand | unimplemented | <visual-box> \|\| <length> | 0px | no | see individual properties | see individual properties |
| overflow-clip-margin-block-end | longhand | unimplemented | <visual-box> \|\| <length> | 0px | no | the computed <length> and a <visual-box> keyword | per computed value if the <visual-box> values match; otherwise discrete |
| overflow-clip-margin-block-start | longhand | unimplemented | <visual-box> \|\| <length> | 0px | no | the computed <length> and a <visual-box> keyword | per computed value if the <visual-box> values match; otherwise discrete |
| overflow-clip-margin-bottom | longhand | unimplemented | <visual-box> \|\| <length> | 0px | no | the computed <length> and a <visual-box> keyword | per computed value if the <visual-box> values match; otherwise discrete |
| overflow-clip-margin-inline | shorthand | unimplemented | <visual-box> \|\| <length> | 0px | no | see individual properties | see individual properties |
| overflow-clip-margin-inline-end | longhand | unimplemented | <visual-box> \|\| <length> | 0px | no | the computed <length> and a <visual-box> keyword | per computed value if the <visual-box> values match; otherwise discrete |
| overflow-clip-margin-inline-start | longhand | unimplemented | <visual-box> \|\| <length> | 0px | no | the computed <length> and a <visual-box> keyword | per computed value if the <visual-box> values match; otherwise discrete |
| overflow-clip-margin-left | longhand | unimplemented | <visual-box> \|\| <length> | 0px | no | the computed <length> and a <visual-box> keyword | per computed value if the <visual-box> values match; otherwise discrete |
| overflow-clip-margin-right | longhand | unimplemented | <visual-box> \|\| <length> | 0px | no | the computed <length> and a <visual-box> keyword | per computed value if the <visual-box> values match; otherwise discrete |
| overflow-clip-margin-top | longhand | unimplemented | <visual-box> \|\| <length> | 0px | no | the computed <length> and a <visual-box> keyword | per computed value if the <visual-box> values match; otherwise discrete |
| overflow-inline | longhand | unimplemented | visible \| hidden \| clip \| scroll \| auto | visible | no | usually specified value, but see text | discrete |
| overflow-wrap | longhand | unimplemented | normal \| break-word \| anywhere | normal | yes | specified keyword | discrete |
| overflow-x | longhand | unimplemented | visible \| hidden \| clip \| scroll \| auto | visible | no | usually specified value, but see text | discrete |
| overflow-y | longhand | unimplemented | visible \| hidden \| clip \| scroll \| auto | visible | no | usually specified value, but see text | discrete |
| overlay | longhand | unimplemented | none \| auto | none | no | as specified | see prose |
| overscroll-behavior | shorthand | unimplemented | [ contain \| none \| auto \| chain ]{1,2} | auto auto | no | see individual properties | discrete |
| overscroll-behavior-block | longhand | unimplemented | contain \| none \| auto \| chain | auto | no | as specified | discrete |
| overscroll-behavior-inline | longhand | unimplemented | contain \| none \| auto \| chain | auto | no | as specified | discrete |
| overscroll-behavior-x | longhand | unimplemented | contain \| none \| auto \| chain | auto | no | as specified | discrete |
| overscroll-behavior-y | longhand | unimplemented | contain \| none \| auto \| chain | auto | no | as specified | discrete |
| padding | shorthand | partial | <'padding-top'>{1,4} | 0 | no | see individual properties | by computed value type |
| padding-block | shorthand | unimplemented | <'padding-top'>{1,2} | see individual properties | see individual properties | see individual properties | see individual properties |
| padding-block-end | longhand | unimplemented | <'padding-top'> | 0 | no | Same as corresponding padding-* properties | by computed value type |
| padding-block-start | longhand | unimplemented | <'padding-top'> | 0 | no | Same as corresponding padding-* properties | by computed value type |
| padding-bottom | longhand | partial | <length-percentage [0,∞]> | 0 | no | a computed <length-percentage> value | by computed value type |
| padding-inline | shorthand | unimplemented | <'padding-top'>{1,2} | see individual properties | see individual properties | see individual properties | see individual properties |
| padding-inline-end | longhand | unimplemented | <'padding-top'> | 0 | no | Same as corresponding padding-* properties | by computed value type |
| padding-inline-start | longhand | unimplemented | <'padding-top'> | 0 | no | Same as corresponding padding-* properties | by computed value type |
| padding-left | longhand | partial | <length-percentage [0,∞]> | 0 | no | a computed <length-percentage> value | by computed value type |
| padding-right | longhand | partial | <length-percentage [0,∞]> | 0 | no | a computed <length-percentage> value | by computed value type |
| padding-top | longhand | partial | <length-percentage [0,∞]> | 0 | no | a computed <length-percentage> value | by computed value type |
| page | longhand | unimplemented | auto \| <custom-ident> | auto | no (but see prose) | specified value | discrete |
| page-break-after | longhand | unimplemented | auto \| always \| avoid \| left \| right \| inherit | auto | no | as specified | discrete |
| page-break-before | longhand | unimplemented | auto \| always \| avoid \| left \| right \| inherit | auto | no | as specified | discrete |
| page-break-inside | longhand | unimplemented | avoid \| auto \| inherit | auto | no | as specified | discrete |
| paint-order | longhand | unimplemented | normal \| [ fill \|\| stroke \|\| markers ] | normal | yes | as specified | discrete |
| path-length | longhand | unimplemented | none \| <length> [0,∞] | none | no | the keyword none, or an absolute length | by computed value |
| pause | shorthand | unimplemented | <'pause-before'> <'pause-after'>? | see individual properties | no | see individual properties | see individual properties |
| pause-after | longhand | unimplemented | <time [0s,∞]> \| none \| x-weak \| weak \| medium \| strong \| x-strong | none | no | specified value | by computed value type |
| pause-before | longhand | unimplemented | <time [0s,∞]> \| none \| x-weak \| weak \| medium \| strong \| x-strong | none | no | specified value | by computed value type |
| perspective | longhand | unimplemented | none \| <length [0,∞]> | none | no | the keyword none or an absolute length | by computed value |
| perspective-origin | longhand | unimplemented | <position> | 50% 50% | no | see background-position | by computed value |
| place-content | shorthand | unimplemented | <'align-content'> <'justify-content'>? | normal | no | see individual properties | discrete |
| place-items | shorthand | unimplemented | <'align-items'> <'justify-items'>? | see individual properties | no | see individual properties | discrete |
| place-self | shorthand | unimplemented | <'align-self'> <'justify-self'>? | auto | no | see individual properties | discrete |
| pointer-events | longhand | unimplemented | auto \| bounding-box \| visiblePainted \| visibleFill \| visibleStroke \| visible \| painted \| fill \| stroke \| all \| none | auto | yes | as specified | discrete |
| pointer-timeline | shorthand | unimplemented | [ <'pointer-timeline-name'> <'pointer-timeline-axis'>? ]# | see individual properties | see individual properties | see individual properties | see individual properties |
| pointer-timeline-axis | longhand | unimplemented | [ block \| inline \| x \| y ]# | block | no | a list of the keywords specified | not animatable |
| pointer-timeline-name | longhand | unimplemented | [ none \| <dashed-ident> ]# | none | no | the keyword none or a list of CSS identifiers | not animatable |
| position | longhand | unimplemented | static \| relative \| absolute \| sticky \| fixed \| <running()> | static | no | specified keyword | discrete |
| position-anchor | longhand | unimplemented | normal \| none \| auto \| <anchor-name> \| match-parent | normal | no | as specified | discrete |
| position-area | longhand | unimplemented | none \| <position-area> | none | no | the keyword none or a pair of keywords, see § 3.1.3 Computed Value and Serialization of <position-area> | discrete |
| position-try | shorthand | unimplemented | <'position-try-order'>? <'position-try-fallbacks'> | see individual properties | see individual properties | see individual properties | see individual properties |
| position-try-fallbacks | longhand | unimplemented | none \| [ [<dashed-ident> \|\| <try-tactic>] \| <position-area> ]# | none | no | as specified | discrete |
| position-try-order | longhand | unimplemented | normal \| <try-size> | normal | no | as specified | discrete |
| position-visibility | longhand | unimplemented | always \| [ anchor-valid \|\| anchor-visible \|\| no-overflow ] | anchor-visible | no | as specified | discrete |
| print-color-adjust | longhand | unimplemented | economy \| exact | economy | yes | specified keyword | discrete |
| quotes | longhand | unimplemented | auto \| none \| match-parent \| [ <string> <string> ]+ | auto | yes | the keyword none, the keyword auto or match-parent, or a list, each item a pair of string values | discrete |
| r | longhand | unimplemented | <length-percentage> | 0 | no | an absolute length or percentage | by computed value |
| reading-flow | longhand | unimplemented | normal \| source-order \| flex-visual \| flex-flow \| grid-rows \| grid-columns \| grid-order | normal | no | as specified | not animatable |
| reading-order | longhand | unimplemented | <integer> | 0 | no | specified integer | by computed value type |
| region-fragment | longhand | unimplemented | auto \| break | auto | no | specified keyword | discrete |
| resize | longhand | unimplemented | none \| both \| horizontal \| vertical \| block \| inline | none | no | specified keyword | discrete |
| rest | shorthand | unimplemented | <'rest-before'> <'rest-after'>? | see individual properties | no | see individual properties | see individual properties |
| rest-after | longhand | unimplemented | <time [0s,∞]> \| none \| x-weak \| weak \| medium \| strong \| x-strong | none | no | specified value | by computed value type |
| rest-before | longhand | unimplemented | <time [0s,∞]> \| none \| x-weak \| weak \| medium \| strong \| x-strong | none | no | specified value | by computed value type |
| right | longhand | unimplemented | auto \| <length-percentage> \| <anchor()> \| <anchor-size()> | auto | no | the keyword auto or a computed <length-percentage> value | by computed value type |
| rotate | longhand | unimplemented | none \| <angle> \| [ x \| y \| z \| <number>{3} ] && <angle> | none | no | the keyword none, or an <angle> with an axis consisting of a list of three <number>s | as SLERP, but see below for none |
| row-gap | longhand | unimplemented | normal \| <length-percentage [0,∞]> \| <line-width> | normal | no | specified keyword, else a computed <length-percentage> value | by computed value type |
| row-rule | shorthand | unimplemented | <gap-rule-list> \| <gap-auto-rule-list> | see individual properties | see individual properties | see individual properties | see individual properties |
| row-rule-break | longhand | unimplemented | none \| normal \| intersection | normal | no | as specified | discrete |
| row-rule-color | longhand | unimplemented | <line-color-list> \| <auto-line-color-list> | currentcolor | no | as specified | repeatable list, see § 4.7 Interpolation of list values. |
| row-rule-inset | shorthand | unimplemented | <'column-rule-inset-cap'> [ / <'column-rule-inset-junction'> ]? | see individual properties | see individual properties | see individual properties | see individual properties |
| row-rule-inset-cap | shorthand | unimplemented | <inset-value> <inset-value>? | see individual properties | see individual properties | see individual properties | see individual properties |
| row-rule-inset-cap-end | longhand | unimplemented | <inset-value> | 0 | no | as specified | by computed value type |
| row-rule-inset-cap-start | longhand | unimplemented | <inset-value> | 0 | no | as specified | by computed value type |
| row-rule-inset-end | shorthand | unimplemented | <inset-value> | see individual properties | see individual properties | see individual properties | see individual properties |
| row-rule-inset-junction | shorthand | unimplemented | <inset-value> <inset-value>? | see individual properties | see individual properties | see individual properties | see individual properties |
| row-rule-inset-junction-end | longhand | unimplemented | <inset-value> | 0 | no | as specified | by computed value type |
| row-rule-inset-junction-start | longhand | unimplemented | <inset-value> | 0 | no | as specified | by computed value type |
| row-rule-inset-start | shorthand | unimplemented | <inset-value> | see individual properties | see individual properties | see individual properties | see individual properties |
| row-rule-style | longhand | unimplemented | <line-style-list> \| <auto-line-style-list> | none | no | as specified | discrete |
| row-rule-visibility-items | longhand | unimplemented | all \| around \| between \| normal | normal | no | as specified | discrete |
| row-rule-width | longhand | unimplemented | <line-width-list> \| <auto-line-width-list> | medium | no | list of absolute lengths, snapped as a border width | repeatable list, see § 4.7 Interpolation of list values. |
| ruby-align | longhand | unimplemented | start \| center \| space-between \| space-around | space-around | yes | specified keyword | by computed value type |
| ruby-merge | longhand | unimplemented | separate \| merge \| auto | separate | yes | specified keyword | by computed value type |
| ruby-overhang | longhand | unimplemented | auto \| spaces | auto | yes | specified keyword | by computed value type |
| ruby-position | longhand | unimplemented | [ alternate \|\| [ over \| under ] ] \| inter-character | alternate | yes | specified keyword | discrete |
| rule | shorthand | unimplemented | <'column-rule'> | see individual properties | no | see individual properties | see individual properties |
| rule-break | shorthand | unimplemented | <'column-rule-break'> | see individual properties | see individual properties | see individual properties | see individual properties |
| rule-color | shorthand | unimplemented | <'column-rule-color'> | see individual properties | no | see individual properties | see individual properties |
| rule-inset | shorthand | unimplemented | <'column-rule-inset'> | see individual properties | see individual properties | see individual properties | see individual properties |
| rule-inset-cap | shorthand | unimplemented | <'column-rule-inset-cap'> | see individual properties | see individual properties | see individual properties | see individual properties |
| rule-inset-end | shorthand | unimplemented | <'column-rule-inset-end'> | see individual properties | see individual properties | see individual properties | see individual properties |
| rule-inset-junction | shorthand | unimplemented | <'column-rule-inset-junction'> | see individual properties | see individual properties | see individual properties | see individual properties |
| rule-inset-start | shorthand | unimplemented | <'column-rule-inset-start'> | see individual properties | see individual properties | see individual properties | see individual properties |
| rule-overlap | longhand | unimplemented | row-over-column \| column-over-row | row-over-column | no | as specified | discrete |
| rule-style | shorthand | unimplemented | <'column-rule-style'> | see individual properties | no | see individual properties | see individual properties |
| rule-visibility-items | shorthand | unimplemented | <'column-rule-visibility-items'> | see individual properties | see individual properties | see individual properties | see individual properties |
| rule-width | shorthand | unimplemented | <'column-rule-width'> | see individual properties | no | see individual properties | see individual properties |
| rx | longhand | unimplemented | <length-percentage> \| auto | auto | no | an absolute length or percentage | by computed value |
| ry | longhand | unimplemented | <length-percentage> \| auto | auto | no | an absolute length or percentage | (missing in snapshot) |
| scale | longhand | unimplemented | none \| [ <number> \| <percentage> ]{1,3} | none | no | the keyword none, or a list of 3 <number>s | by computed value, but see below for none |
| scroll-axis-lock | longhand | unimplemented | auto \| none | auto | no | specified value | discrete |
| scroll-behavior | longhand | unimplemented | auto \| smooth | auto | no | specified value | not animatable |
| scroll-initial-target | longhand | unimplemented | none \| nearest | none | no | specified keyword | none |
| scroll-margin | shorthand | unimplemented | <length>{1,4} | 0 | no | per side, an absolute length | by computed value type |
| scroll-margin-block | shorthand | unimplemented | <length>{1,2} | 0 | no | see individual properties | by computed value type |
| scroll-margin-block-end | longhand | unimplemented | <length> | 0 | no | absolute length | by computed value type |
| scroll-margin-block-start | longhand | unimplemented | <length> | 0 | no | absolute length | by computed value type |
| scroll-margin-bottom | longhand | unimplemented | <length> | 0 | no | absolute length | by computed value type |
| scroll-margin-inline | shorthand | unimplemented | <length>{1,2} | 0 | no | see individual properties | by computed value type |
| scroll-margin-inline-end | longhand | unimplemented | <length> | 0 | no | absolute length | by computed value type |
| scroll-margin-inline-start | longhand | unimplemented | <length> | 0 | no | absolute length | by computed value type |
| scroll-margin-left | longhand | unimplemented | <length> | 0 | no | absolute length | by computed value type |
| scroll-margin-right | longhand | unimplemented | <length> | 0 | no | absolute length | by computed value type |
| scroll-margin-top | longhand | unimplemented | <length> | 0 | no | absolute length | by computed value type |
| scroll-marker-group | longhand | unimplemented | none \| [ [ before \| after ] \|\| [ links \| tabs ] ] | none | no | specified keyword(s) | discrete |
| scroll-padding | shorthand | unimplemented | [ auto \| <length-percentage [0,∞]> ]{1,4} | auto | no | per side, either the keyword auto or a computed <length-percentage> value | by computed value type |
| scroll-padding-block | shorthand | unimplemented | [ auto \| <length-percentage [0,∞]> ]{1,2} | auto | no | see individual properties | by computed value |
| scroll-padding-block-end | longhand | unimplemented | auto \| <length-percentage [0,∞]> | auto | no | the keyword auto or a computed <length-percentage> value | by computed value type |
| scroll-padding-block-start | longhand | unimplemented | auto \| <length-percentage [0,∞]> | auto | no | the keyword auto or a computed <length-percentage> value | by computed value type |
| scroll-padding-bottom | longhand | unimplemented | auto \| <length-percentage [0,∞]> | auto | no | the keyword auto or a computed <length-percentage> value | by computed value type |
| scroll-padding-inline | shorthand | unimplemented | [ auto \| <length-percentage [0,∞]> ]{1,2} | auto | no | see individual properties | by computed value |
| scroll-padding-inline-end | longhand | unimplemented | auto \| <length-percentage [0,∞]> | auto | no | the keyword auto or a computed <length-percentage> value | by computed value type |
| scroll-padding-inline-start | longhand | unimplemented | auto \| <length-percentage [0,∞]> | auto | no | the keyword auto or a computed <length-percentage> value | by computed value type |
| scroll-padding-left | longhand | unimplemented | auto \| <length-percentage [0,∞]> | auto | no | the keyword auto or a computed <length-percentage> value | by computed value type |
| scroll-padding-right | longhand | unimplemented | auto \| <length-percentage [0,∞]> | auto | no | the keyword auto or a computed <length-percentage> value | by computed value type |
| scroll-padding-top | longhand | unimplemented | auto \| <length-percentage [0,∞]> | auto | no | the keyword auto or a computed <length-percentage> value | by computed value type |
| scroll-snap-align | longhand | unimplemented | [ none \| start \| end \| center ]{1,2} | none | no | two keywords | discrete |
| scroll-snap-stop | longhand | unimplemented | normal \| always | normal | no | specified keyword | discrete |
| scroll-snap-type | longhand | unimplemented | none \| [ x \| y \| block \| inline \| both \| pair ] [ mandatory \| proximity ]? | none | no | specified keyword(s) | discrete |
| scroll-target-group | longhand | unimplemented | none \| auto | none | no | specified value | discrete |
| scroll-timeline | shorthand | unimplemented | [ <'scroll-timeline-name'> <'scroll-timeline-axis'>? ]# | see individual properties | no | see individual properties | not animatable |
| scroll-timeline-axis | longhand | unimplemented | [ block \| inline \| x \| y ]# | block | no | a list of the keywords specified | not animatable |
| scroll-timeline-name | longhand | unimplemented | [ none \| <dashed-ident> ]# | none | no | list, each item either a CSS identifier or the keyword none | not animatable |
| scrollbar-color | longhand | unimplemented | auto \| <color>{2} | auto | yes | specified keyword or two computed colors | by computed value |
| scrollbar-gutter | longhand | unimplemented | auto \| stable && both-edges? | auto | no | specified keyword(s) | discrete |
| scrollbar-width | longhand | unimplemented | auto \| thin \| none | auto | no | specified keyword | discrete |
| shape-image-threshold | longhand | unimplemented | <opacity-value> | 0 | no | specified number, clamped to the range [0,1] | by computed value |
| shape-inside | longhand | unimplemented | auto \| outside-shape \| [ <basic-shape> \|\| shape-box ] \| <image> \| display | auto | no | computed lengths for <basic-shape>, the absolute URL for <url>, otherwise as specified | as defined for <basic-shape>, otherwise discrete |
| shape-margin | longhand | unimplemented | <length-percentage [0,∞]> | 0 | no | computed <length-percentage> value | by computed value |
| shape-outside | longhand | unimplemented | none \| [ <basic-shape> \|\| <shape-box> ] \| <image> | none | no | as defined for <basic-shape> (with <shape-box> following, if supplied); else the computed <image>; else the keyword as specified | as defined for <basic-shape>, otherwise discrete |
| shape-padding | longhand | unimplemented | <length-percentage [0,∞]> | 0 | no | computed <length-percentage> value | by computed value |
| shape-rendering | longhand | unimplemented | auto \| optimizeSpeed \| crispEdges \| geometricPrecision | auto | yes | as specified | discrete |
| size | shorthand | unimplemented | <'width'> <'height'>? | auto | see individual properties | see individual properties | see individual properties |
| slider-orientation | longhand | unimplemented | auto \| left-to-right \| right-to-left \| top-to-bottom \| bottom-to-top | auto | no | as specified | discrete |
| spatial-navigation-action | longhand | unimplemented | auto \| focus \| scroll | auto | no | as specified | discrete |
| spatial-navigation-contain | longhand | unimplemented | auto \| contain | auto | no | as specified | discrete |
| spatial-navigation-function | longhand | unimplemented | normal \| grid | normal | no | as specified | discrete |
| speak | longhand | unimplemented | auto \| never \| always | auto | yes | specified value | discrete |
| speak-as | longhand | unimplemented | normal \| spell-out \|\| digits \|\| [ literal-punctuation \| no-punctuation ] | normal | yes | specified value | discrete |
| stop-color | longhand | unimplemented | <'color'> | (missing in snapshot) | (missing in snapshot) | (missing in snapshot) | (missing in snapshot) |
| stop-opacity | longhand | unimplemented | <'opacity'> | (missing in snapshot) | (missing in snapshot) | (missing in snapshot) | (missing in snapshot) |
| string-set | longhand | unimplemented | none \| [ <custom-ident> <string>+ ]# | none | no | the keyword none or a list, each item an identifier paired with a list of string values | discrete |
| stroke | longhand | unimplemented | <paint> | none | yes | as specified, but with <color> values computed and <url> values made absolute | by computed value |
| stroke-align | longhand | unimplemented | center \| inset \| outset | center | yes | as specified | discrete |
| stroke-alignment | longhand | unimplemented | center \| inner \| outer | center | yes | as specified | (missing in snapshot) |
| stroke-break | longhand | unimplemented | bounding-box \| slice \| clone | bounding-box | ? | as specified | discrete |
| stroke-color | longhand | unimplemented | <color># | transparent | yes | the computed color | by computed value |
| stroke-dash-corner | longhand | unimplemented | none \| <length> | none | yes | specified value, with lengths made absolute | by computed value if <length>, otherwise discrete |
| stroke-dash-justify | longhand | unimplemented | none \| [ stretch \| compress ] \|\| [ dashes \|\| gaps ] | none | yes | specified value, with lengths made absolute | discrete |
| stroke-dashadjust | longhand | unimplemented | none \| [stretch \| compress] [dashes \| gaps]? | none | yes | as specified | (missing in snapshot) |
| stroke-dasharray | longhand | unimplemented | none \| [<length-percentage> \| <number>]+# | none | yes | as specified | repeatable list |
| stroke-dashcorner | longhand | unimplemented | none \| <length> | none | yes | absolute length or keyword specified | (missing in snapshot) |
| stroke-dashoffset | longhand | unimplemented | <length-percentage> \| <number> | 0 | yes | as specified | repeatable list |
| stroke-image | longhand | unimplemented | <paint># | none | yes | as specified, with any <image> computed | repeatable list |
| stroke-linecap | longhand | unimplemented | butt \| round \| square | butt | yes | as specified | discrete |
| stroke-linejoin | longhand | unimplemented | [ crop \| arcs \| miter ] \|\| [ bevel \| round \| fallback ] | miter | yes | as specified | discrete |
| stroke-miterlimit | longhand | unimplemented | <number> | 4 | yes | a number | discrete |
| stroke-opacity | longhand | unimplemented | <'opacity'> | 1 | yes | the specified value converted to a <number>, clamped to the range [0,1] | by computed value |
| stroke-origin | longhand | unimplemented | match-parent \| fill-box \| stroke-box \| content-box \| padding-box \| border-box | match-parent | no | as specified | discrete |
| stroke-position | longhand | unimplemented | <position># | 0% 0% | yes | A list, each item consisting of: a pair of offsets (horizontal and vertical) from the top left origin each given as a combination of an absolute length and a percentage | repeatable list |
| stroke-repeat | longhand | unimplemented | <repeat-style># | repeat | yes | A list, each item consisting of: two keywords, one per dimension | discrete |
| stroke-size | longhand | unimplemented | <bg-size># | auto | yes | as specified, but with lengths made absolute and omitted auto keywords filled in | repeatable list |
| stroke-width | longhand | unimplemented | [ <length-percentage> \| <line-width> \| <number> ]# | 1px | yes | the absolute length, or percentage | by computed value |
| tab-size | longhand | unimplemented | <number [0,∞]> \| <length [0,∞]> | 8 | yes | the specified number or absolute length | by computed value type |
| table-layout | longhand | unimplemented | auto \| fixed | auto | no | specified keyword | discrete |
| text-align | shorthand | unimplemented | start \| end \| left \| right \| center \| <string> \| justify \| match-parent \| justify-all | start | yes | see individual properties | discrete |
| text-align-all | longhand | unimplemented | start \| end \| left \| right \| center \| <string> \| justify \| match-parent | start | yes | keyword as specified, except for match-parent which computes as defined above | discrete |
| text-align-last | longhand | unimplemented | auto \| start \| end \| left \| right \| center \| justify \| match-parent | auto | yes | keyword as specified, except for match-parent which computes as defined above | discrete |
| text-anchor | longhand | unimplemented | start \| middle \| end | start | yes | as specified | discrete |
| text-autospace | longhand | unimplemented | normal \| <autospace> \| auto | normal | yes | specified keyword(s) | discrete |
| text-box | longhand | unimplemented | normal \| <'text-box-trim'> \|\| <'text-box-edge'> | normal | no | the specified keyword | discrete |
| text-box-edge | longhand | unimplemented | auto \| <text-edge> | auto | yes | the specified keyword | discrete |
| text-box-trim | longhand | unimplemented | none \| trim-start \| trim-end \| trim-both | none | no | the specified keyword | discrete |
| text-combine-upright | longhand | unimplemented | none \| all \| [ digits <integer [2,4]>? ] | none | yes | specified keyword, plus integer if digits | not animatable |
| text-decoration | shorthand | unimplemented | <'text-decoration-line'> \|\| <'text-decoration-thickness'> \|\| <'text-decoration-style'> \|\| <'text-decoration-color'> | see individual properties | see individual properties | see individual properties | see individual properties |
| text-decoration-color | longhand | unimplemented | <color> | currentcolor | no | computed color | by computed value type |
| text-decoration-inset | longhand | unimplemented | <length-percentage>{1,2} \| auto | 0 | no | specified keyword or absolute length | by computed value |
| text-decoration-line | longhand | unimplemented | none \| [ underline \|\| overline \|\| line-through \|\| blink ] \| spelling-error \| grammar-error | none | no (but see prose, above) | specified keyword(s) | discrete |
| text-decoration-skip | shorthand | unimplemented | none \| auto | See individual properties | yes | See individual properties | discrete |
| text-decoration-skip-box | longhand | unimplemented | none \| all | none | yes | specified keyword(s) | discrete |
| text-decoration-skip-ink | longhand | unimplemented | auto \| none \| all | auto | yes | specified keyword | discrete |
| text-decoration-skip-self | longhand | unimplemented | auto \| skip-all \| [ skip-underline \|\| skip-overline \|\| skip-line-through ] \| no-skip | auto | no | specified keyword(s) except for skip-all, see below | discrete |
| text-decoration-skip-spaces | longhand | unimplemented | none \| all \| [ start \|\| end ] | start end | yes | specified keyword(s) | discrete |
| text-decoration-style | longhand | unimplemented | solid \| double \| dotted \| dashed \| wavy | solid | no | specified keyword | discrete |
| text-decoration-thickness | longhand | unimplemented | auto \| from-font \| <length-percentage> \| <line-width> | auto | no | as specified, with <length-percentage> values computed | by computed value |
| text-emphasis | shorthand | unimplemented | <'text-emphasis-style'> \|\| <'text-emphasis-color'> | see individual properties | see individual properties | see individual properties | see individual properties |
| text-emphasis-color | longhand | unimplemented | <color> | currentcolor | yes | computed color | by computed value type |
| text-emphasis-position | longhand | unimplemented | [ over \| under ] && [ right \| left ]? | over right | yes | specified keyword(s) | discrete |
| text-emphasis-skip | longhand | unimplemented | spaces \|\| punctuation \|\| symbols \|\| narrow | spaces punctuation | yes | specified keyword(s) | discrete |
| text-emphasis-style | longhand | unimplemented | none \| [ [ filled \| open ] \|\| [ dot \| circle \| double-circle \| triangle \| sesame ] ] \| <string> | none | yes | the keyword none, a pair of keywords representing the shape and fill, or a string | discrete |
| text-fit | longhand | unimplemented | [ none \| grow \| shrink ] [consistent \| per-line \| per-line-all]? <percentage>? | none | yes | specified keywords or computed <percentage> value | discrete |
| text-group-align | longhand | unimplemented | none \| start \| end \| left \| right \| center | none | no | specified keyword | discrete |
| text-indent | longhand | unimplemented | [ <length-percentage> ] && hanging? && each-line? | 0 | yes | computed <length-percentage> value, plus any specified keywords | by computed value type |
| text-justify | longhand | unimplemented | [ auto \| none \| inter-word \| inter-character \| ruby ] \|\| no-compress | auto | yes | specified keyword (except for the distribute legacy value) | discrete |
| text-orientation | longhand | unimplemented | mixed \| upright \| sideways | mixed | yes | specified value | not animatable |
| text-overflow | longhand | unimplemented | [ clip \| ellipsis \| <string> \| fade \| <fade()> ]{1,2} | clip | no | as specified, with lengths made absolute | by computed value type |
| text-rendering | longhand | unimplemented | auto \| optimizeSpeed \| optimizeLegibility \| geometricPrecision | auto | yes | as specified | discrete |
| text-shadow | longhand | unimplemented | none \| <shadow># | none | yes | either the keyword none or a list, each item consisting of four absolute lengths plus a computed color and optionally also an inset keyword | as shadow list |
| text-size-adjust | longhand | unimplemented | auto \| none \| <percentage [0,∞]> | auto | yes | specified keyword or percentage | by computed value |
| text-spacing | shorthand | unimplemented | none \| auto \| <spacing-trim> \|\| <autospace> | see individual properties | yes | specified keyword(s) | discrete |
| text-spacing-trim | longhand | unimplemented | <spacing-trim> \| auto | normal | yes | specified keyword(s) | discrete |
| text-transform | longhand | unimplemented | none \| [capitalize \| uppercase \| lowercase ] \|\| full-width \|\| full-size-kana \| math-auto | none | yes | specified keyword | discrete |
| text-underline-offset | longhand | unimplemented | auto \| <length-percentage> | auto | yes | as specified, with <length-percentage> values computed | by computed value |
| text-underline-position | longhand | unimplemented | auto \| [ from-font \| under ] \|\| [ left \| right ] | auto | yes | specified keyword(s) | discrete |
| text-wrap | shorthand | unimplemented | <'text-wrap-mode'> \|\| <'text-wrap-style'> | wrap | see individual properties | see individual properties | see individual properties |
| text-wrap-mode | longhand | unimplemented | wrap \| nowrap | wrap | yes | specified keyword | discrete |
| text-wrap-style | longhand | unimplemented | auto \| balance \| stable \| pretty \| avoid-short-last-line | auto | yes | specified keyword | discrete |
| timeline-scope | longhand | unimplemented | none \| all \| <dashed-ident># | none | no | the keyword none, the keyword all, or a list of CSS identifiers | not animatable |
| timeline-trigger | shorthand | unimplemented | none \| [ <'timeline-trigger-name'> <'timeline-trigger-source'> <'timeline-trigger-activation-range'> [ / <'timeline-trigger-active-range'> ]? ]# | see individual properties | see individual properties | see individual properties | see individual properties |
| timeline-trigger-activation-range | shorthand | unimplemented | [ <'timeline-trigger-activation-range-start'> <'timeline-trigger-activation-range-end'>? ]# | see individual properties | see individual properties | see individual properties | see individual properties |
| timeline-trigger-activation-range-end | longhand | unimplemented | [ normal \| <length-percentage> \| <timeline-range-name> <length-percentage>? ]# | normal | no | list, each item either the keyword normal or a timeline range and progress percentage | not animatable |
| timeline-trigger-activation-range-start | longhand | unimplemented | [ normal \| <length-percentage> \| <timeline-range-name> <length-percentage>? ]# | normal | no | list, each item either the keyword normal or a timeline range and progress percentage | not animatable |
| timeline-trigger-active-range | shorthand | unimplemented | [ <'timeline-trigger-active-range-start'> <'timeline-trigger-active-range-end'>? ]# | see individual properties | see individual properties | see individual properties | see individual properties |
| timeline-trigger-active-range-end | longhand | unimplemented | [ auto \| normal \| <length-percentage> \| <timeline-range-name> <length-percentage>? ]# | auto | no | list, each item either the keyword normal or a timeline range and progress percentage | not animatable |
| timeline-trigger-active-range-start | longhand | unimplemented | [ auto \| normal \| <length-percentage> \| <timeline-range-name> <length-percentage>? ]# | auto | no | list, each item either the keyword normal or a timeline range and progress percentage | not animatable |
| timeline-trigger-name | longhand | unimplemented | none \| <dashed-ident># | none | no | as specified | not animatable |
| timeline-trigger-source | longhand | unimplemented | [ none \| auto \| [ <dashed-ident> \| <scroll()> \| <view()> ]+ ]# | auto | no | as specified | not animatable |
| top | longhand | unimplemented | auto \| <length-percentage> \| <anchor()> \| <anchor-size()> | auto | no | the keyword auto or a computed <length-percentage> value | by computed value type |
| touch-action | longhand | unimplemented | auto \| none \| [ [ pan-x \| pan-left \| pan-right ] \|\| [ pan-y \| pan-up \| pan-down ] \|\| pinch-zoom ] \| manipulation | auto | no | Same as specified value | not animatable |
| transform | longhand | unimplemented | none \| <transform-list> | none | no | as specified, but with lengths made absolute | transform list, see interpolation rules |
| transform-box | longhand | unimplemented | content-box \| border-box \| fill-box \| stroke-box \| view-box | view-box | no | specified keyword | discrete |
| transform-origin | longhand | unimplemented | [ left \| center \| right \| top \| bottom \| <length-percentage> ] \| [ left \| center \| right \| <length-percentage> ] [ top \| center \| bottom \| <length-percentage> ] <length>? \| [ [ center \| left \| right ] && [ center \| top \| bottom ] ] <length>? | 50% 50% | no | see background-position | by computed value |
| transform-style | longhand | unimplemented | flat \| preserve-3d | flat | no | specified keyword | discrete |
| transition | shorthand | unimplemented | <single-transition># | see individual properties | no | see individual properties | not animatable |
| transition-behavior | longhand | unimplemented | <transition-behavior-value># | normal | no | as specified | not animatable |
| transition-delay | longhand | unimplemented | <time># | 0s | no | list, each item a duration | not animatable |
| transition-duration | longhand | unimplemented | <time [0s,∞]># | 0s | no | list, each item a duration | not animatable |
| transition-property | longhand | unimplemented | none \| <single-transition-property># | all | no | the keyword none else a list of identifiers | not animatable |
| transition-timing-function | longhand | unimplemented | <easing-function># | ease | no | as specified | not animatable |
| translate | longhand | unimplemented | none \| <length-percentage> [ <length-percentage> <length>? ]? | none | no | the keyword none or a pair of computed <length-percentage> values and an absolute length | by computed value, but see below for none |
| trigger-scope | longhand | unimplemented | none \| all \| <dashed-ident># | none | no | as specified | not animatable |
| unicode-bidi | longhand | unimplemented | normal \| embed \| isolate \| bidi-override \| isolate-override \| plaintext | normal | no | specified value | not animatable |
| user-select | longhand | unimplemented | auto \| text \| none \| contain \| all | auto | no | specified keyword | discrete |
| vector-effect | longhand | unimplemented | none \| non-scaling-stroke \| non-scaling-size \| non-rotation \| fixed-position | none | no | as specified | discrete |
| vertical-align | shorthand | unimplemented | [ first \| last] \|\| <'alignment-baseline'> \|\| <'baseline-shift'> | baseline | no | see individual properties | see individual properties |
| view-timeline | shorthand | unimplemented | [ <'view-timeline-name'> [ <'view-timeline-axis'> \|\| <'view-timeline-inset'> ]? ]# | see individual properties | see individual properties | see individual properties | see individual properties |
| view-timeline-axis | longhand | unimplemented | [ block \| inline \| x \| y ]# | block | no | a list of the keywords specified | not animatable |
| view-timeline-inset | longhand | unimplemented | [ [ auto \| <length-percentage> ]{1,2} ]# | auto | no | a list consisting of two-value pairs representing the start and end insets each as either the keyword auto or a computed <length-percentage> value | by computed value type |
| view-timeline-name | longhand | unimplemented | [ none \| <dashed-ident> ]# | none | no | list, each item either a CSS identifier or the keyword none | not animatable |
| view-transition-class | longhand | unimplemented | none \| <custom-ident>+ | none | no | as specified | discrete |
| view-transition-group | longhand | unimplemented | normal \| contain \| nearest \| <custom-ident> | normal | no | as specified | discrete |
| view-transition-name | longhand | unimplemented | none \| <custom-ident> | none | no | as specified | discrete |
| view-transition-scope | longhand | unimplemented | none \| all | none | no | as specified | discrete |
| visibility | longhand | unimplemented | visible \| hidden \| force-hidden \| collapse | visible | yes | as specified | discrete |
| voice-balance | longhand | unimplemented | <number> \| left \| center \| right \| leftwards \| rightwards | center | yes | the specified value resolved to a <number> between -100 and 100 (inclusive) | by computed value type |
| voice-duration | longhand | unimplemented | auto \| <time [0s,∞]> | auto | no | specified value | by computed value type |
| voice-family | longhand | unimplemented | [ <voice-family-name> \| <generic-voice> ]# \| preserve | implementation-dependent | yes | specified value | discrete |
| voice-pitch | longhand | unimplemented | <frequency [0Hz,∞]> && absolute \| [ [ x-low \| low \| medium \| high \| x-high ] \|\| [ <frequency [0Hz,∞]> \| <semitones> \| <percentage> ] ] | medium | yes | one of the predefined pitch keywords if only the keyword is specified by itself, otherwise an absolute frequency calculated by converting the keyword value (if any) to a fixed frequency based on the current voice-family and by applying the specified relative offset (if any) | by computed value type |
| voice-range | longhand | unimplemented | <frequency [0Hz,∞]> && absolute \| [ [ x-low \| low \| medium \| high \| x-high ] \|\| [ <frequency [0Hz,∞]> \| <semitones> \| <percentage> ] ] | medium | yes | one of the predefined pitch keywords if only the keyword is specified by itself, otherwise an absolute frequency calculated by converting the keyword value (if any) to a fixed frequency based on the current voice-family and by applying the specified relative offset (if any) | by computed value type |
| voice-rate | longhand | unimplemented | [ normal \| x-slow \| slow \| medium \| fast \| x-fast ] \|\| <percentage [0,∞]> | normal | yes | a keyword value, and optionally also a percentage relative to the keyword (if not 100%) | by computed value type |
| voice-stress | longhand | unimplemented | normal \| strong \| moderate \| none \| reduced | normal | yes | specified value | discrete |
| voice-volume | longhand | unimplemented | silent \| [ [ x-soft \| soft \| medium \| loud \| x-loud ] \|\| <decibel> ] | medium | yes | silent, or a keyword value and optionally also a decibel offset (if not zero) | by computed value type |
| white-space | shorthand | unimplemented | normal \| pre \| pre-wrap \| pre-line \| <'white-space-collapse'> \|\| <'text-wrap-mode'> \|\| <'white-space-trim'> | normal | see individual properties | specified keyword | discrete |
| white-space-collapse | longhand | unimplemented | collapse \| discard \| preserve \| preserve-breaks \| preserve-spaces \| break-spaces | collapse | yes | specified keyword | discrete |
| white-space-trim | longhand | unimplemented | none \| discard-before \|\| discard-after \|\| discard-inner | none | no | specified keyword(s) | discrete |
| widows | longhand | unimplemented | <integer [1,∞]> | 2 | yes | specified integer | by computed value type |
| width | longhand | partial | auto \| <box-size> \| <anchor-size()> | auto | no | as specified, with <length-percentage> values computed | by computed value type, recursing into fit-content() |
| will-change | longhand | unimplemented | auto \| <animateable-feature># | auto | no | specified value | not animatable |
| window-drag | longhand | unimplemented | none \| move | none | yes | as specified | discrete |
| word-break | longhand | unimplemented | normal \| break-all \| keep-all \| manual \| auto-phrase \| break-word | normal | yes | specified keyword | discrete |
| word-space-transform | longhand | unimplemented | none \| [ space \| ideographic-space ] && auto-phrase? | none | yes | as specified | discrete |
| word-spacing | longhand | unimplemented | normal \| <length-percentage> | normal | yes | an absolute length and/or a percentage | by computed value type |
| word-wrap | alias of overflow-wrap | alias unimplemented | normal \| break-word \| anywhere | (missing in snapshot) | (missing in snapshot) | (missing in snapshot) | (missing in snapshot) |
| wrap-after | longhand | unimplemented | auto \| avoid \| avoid-line \| avoid-flex \| line \| flex | auto | no | specified keyword | discrete |
| wrap-before | longhand | unimplemented | auto \| avoid \| avoid-line \| avoid-flex \| line \| flex | auto | no | specified keyword | discrete |
| wrap-flow | longhand | unimplemented | auto \| both \| start \| end \| minimum \| maximum \| clear | auto | no | as specified except for element’s whose float computed value is not none, in which case the computed value is auto. | discrete |
| wrap-inside | longhand | unimplemented | auto \| avoid | auto | no | specified keyword | discrete |
| wrap-through | longhand | unimplemented | wrap \| none | wrap | no | as specified | discrete |
| writing-mode | longhand | unimplemented | horizontal-tb \| vertical-rl \| vertical-lr \| sideways-rl \| sideways-lr | horizontal-tb | yes | specified value | not animatable |
| x | longhand | unimplemented | <length-percentage> | 0 | no | an absolute length or percentage | by computed value |
| y | longhand | unimplemented | <length-percentage> | 0 | no | an absolute length or percentage | by computed value |
| z-index | longhand | unimplemented | auto \| <integer> \| inherit | auto | no | as specified | by computed value type |
| zoom | longhand | unimplemented | <number [0,∞]> \| <percentage [0,∞]> | 1 | no | as specified, but with <percentage> converted to the equivalent <number> | by computed value type |

All unimplemented rows: No executable property parser, cascade, computed or used-value handler. No animation engine (including discrete animation).

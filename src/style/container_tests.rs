#[cfg(feature = "style-bench-reference")]
use super::ContainerSnapshot;
use super::{InteractionState, PseudoElement};
use crate::{
    css::{media::MediaEnvironment, values::Length},
    dom::NodeId,
    engine::Frame,
    html::parser,
    layout::{LayoutBox, LayoutTree},
    paint,
};

fn frame(source: &str) -> Frame {
    Frame::new(
        url::Url::parse("https://example.test/").unwrap(),
        parser::parse(source).unwrap(),
    )
}

fn id(frame: &Frame, name: &str) -> NodeId {
    frame
        .document
        .preorder()
        .find(|&id| {
            frame
                .document
                .node(id)
                .unwrap()
                .as_element()
                .is_some_and(|e| e.attribute("id") == Some(name))
        })
        .unwrap()
}

fn box_for(tree: &LayoutTree, node: NodeId) -> &LayoutBox {
    fn find(layout: &LayoutBox, node: NodeId) -> Option<&LayoutBox> {
        if layout.dom_node_id() == Some(node) && layout.pseudo_element().is_none() {
            return Some(layout);
        }
        layout.children().iter().find_map(|child| find(child, node))
    }
    find(tree.root(), node).unwrap()
}

fn layout(frame: &mut Frame) -> LayoutTree {
    frame.layout_with_container_queries(&paint::load_bundled_font().unwrap())
}

#[test]
fn centered_containers_use_content_width_feedback_and_stay_stable_across_resizes() {
    let mut frame = frame(
        "<!doctype html><style>body{margin:0}\
        #owner{container:owner / inline-size;width:50%;margin:auto;padding:10px}\
        #child{width:50cqw;margin:0 auto}\
        @container owner (width >= 300px){#child{color:blue}}\
        </style><section id='owner'>Before<div id='child'>Child</div>After</section>",
    );
    for (viewport, width, left, child_width) in [
        (848.0, 400.0, 190.0, 200.0),
        (448.0, 200.0, 90.0, 100.0),
        (1048.0, 500.0, 240.0, 250.0),
        (848.0, 400.0, 190.0, 200.0),
    ] {
        frame.set_media_environment(MediaEnvironment::new(viewport, 600.0));
        let tree = layout(&mut frame);
        let owner = box_for(&tree, id(&frame, "owner"));
        assert_eq!(owner.content_rect().width, width);
        assert_eq!(owner.rect().x, 24.0 + left);
        assert_eq!(owner.used_margins().left.0, left);
        let child = box_for(&tree, id(&frame, "child"));
        assert_eq!(child.content_rect().width, child_width);
        assert_eq!(child.rect().x, owner.content_rect().x + child_width / 2.0);
        assert_eq!(
            child.style().color,
            Some(if width >= 300.0 { 0x0000ff } else { 0x000000 })
        );
        assert_eq!(owner.children()[0].content_rect().width, width);
        assert_eq!(owner.children()[2].content_rect().width, width);
        assert!(owner.children()[0].is_anonymous());
        assert!(
            frame.stylesheets.diagnostics.is_empty(),
            "{:?}",
            frame.stylesheets.diagnostics
        );
        assert!(tree.diagnostics().is_empty());
        assert_eq!(layout(&mut frame), tree);
    }
}

#[test]
fn margin_collapsing_and_containment_preserve_content_geometry_feedback() {
    let mut frame = frame(
        "<!doctype html><style>body{margin:0;font:16px/20px serif}\
         #owner{container:owner / inline-size;width:50%;margin:10px 0 15px}\
         #a{height:20px;margin:30px 0 20px}#empty{margin:-10px 0 50px}\
         #b{height:20px;margin:15px 0 35px;width:50cqw}\
         @container owner (width >= 300px){#b{color:blue}}\
         </style><section id='owner'><div id='a'></div><div id='empty'></div><div id='b'></div></section>",
    );
    for (viewport, width) in [(848.0, 400.0), (448.0, 200.0), (848.0, 400.0)] {
        frame.set_media_environment(MediaEnvironment::new(viewport, 600.0));
        let tree = layout(&mut frame);
        let owner = box_for(&tree, id(&frame, "owner"));
        let a = box_for(&tree, id(&frame, "a"));
        let b = box_for(&tree, id(&frame, "b"));
        assert_eq!(owner.content_rect().width, width);
        assert_eq!(owner.content_rect().height, 145.0);
        assert_eq!(a.rect().y - owner.content_rect().y, 30.0);
        assert_eq!(b.rect().y - a.rect().y - a.rect().height, 40.0);
        assert_eq!(b.content_rect().width, width / 2.0);
        assert_eq!(
            b.style().color,
            Some(if width >= 300.0 { 0x0000ff } else { 0x000000 })
        );
        assert!(tree.diagnostics().is_empty());
        assert!(frame.stylesheets.diagnostics.is_empty());
        assert_eq!(layout(&mut frame), tree);
    }
}

#[test]
fn anonymous_wrappers_and_inline_continuations_never_replace_real_container_geometry() {
    let mut frame = frame(
        "<style>\
         #owner{container:owner / inline-size;width:240px;padding:10px}\
         #inline{container:ghost / size;font-size:24px}\
         #block{display:block;width:80px;height:20px}\
         @container owner (width:240px){#inline{color:blue}}\
         @container ghost (width > 0px){#block{color:red}}\
         </style>\
         <section id='owner'>Leading <em id='inline'>Before<span id='block'>Block</span>\
         After</em> Trailing</section>",
    );
    let tree = layout(&mut frame);
    let owner = box_for(&tree, id(&frame, "owner"));
    assert_eq!(owner.content_rect().width, 240.0);
    assert_eq!(owner.children().len(), 3);
    for child in [&owner.children()[0], &owner.children()[2]] {
        assert!(child.is_anonymous());
        assert_eq!(child.dom_node_id(), None);
        assert_eq!(
            child.style().container_type,
            crate::css::container::ContainerType::Normal
        );
        assert!(child.style().container_names.is_empty());
        assert_eq!(child.content_rect().width, 240.0);
    }
    assert_eq!(
        frame.styles.get(id(&frame, "inline")).unwrap().color,
        Some(0x0000ff)
    );
    assert_eq!(
        frame.styles.get(id(&frame, "block")).unwrap().color,
        Some(0x0000ff)
    );
    assert!(
        frame.stylesheets.diagnostics.is_empty(),
        "{:?}",
        frame.stylesheets.diagnostics
    );
    let stable = tree.clone();
    assert_eq!(layout(&mut frame), stable);
}

#[test]
fn container_root_font_metrics_skip_anonymous_root_wrappers() {
    let mut frame = frame(
        "<style>html{display:inline;font-size:30px}\
         body{container-type:inline-size;width:300px}\
         #card{width:1rem}\
         @container (width:10rem){#card{width:2rem}}\
         </style><div id='card'>Root font</div>",
    );
    let tree = layout(&mut frame);
    assert!(tree.root().children()[0].is_anonymous());
    assert_eq!(
        box_for(&tree, id(&frame, "card")).content_rect().width,
        60.0
    );
    assert!(
        frame.stylesheets.diagnostics.is_empty(),
        "{:?}",
        frame.stylesheets.diagnostics
    );
}

#[test]
fn named_nearest_ancestors_axis_eligibility_nesting_and_no_self_query() {
    let mut frame = frame(
        r#"<style>
        #outer {container:Panel alias / size; width:500px; height:240px}
        #inner {container:inner / inline-size; width:200px}
        .card {color:red; width:70px}
        @container (width > 300px) {.card {color:purple}}
        @container Panel (400px < width < 600px) {.card {color:blue}}
        @container panel (width > 0px) {.card {color:purple}}
        @container alias (height:240px) {.card {padding-top:12px}}
        @container (height:240px) {.card {padding-bottom:13px}}
        @container (orientation:landscape) {.card {margin-top:14px}}
        @container Panel (width:500px) {
          @container inner (inline-size:12.5em) {.card {background:green}}
        }
        @container inner (width:200px) {
          #inner {color:purple}
          #inner::before {content:'self pseudo'; width:10cqw}
        }
        @container missing (width > 0px) {.card {color:purple}}
        @container inner (height > 0px) {.card {color:purple}}
        @container (width > 0px) {#orphan {color:purple}}
        </style><div id='outer'><div id='inner'><p id='card' class='card'>Card</p></div></div>
        <p id='orphan' style='color:red'>Outside</p>"#,
    );
    let tree = layout(&mut frame);
    let card = frame.styles.get(id(&frame, "card")).unwrap();
    assert_eq!(card.color, Some(0x0000ff));
    assert_eq!(card.background_color, Some(0x008000));
    assert_eq!(card.padding.top, Length::Px(12.0));
    assert_eq!(card.padding.bottom, Length::Px(13.0));
    assert_eq!(card.margin.top, Length::Px(14.0).into());
    assert_ne!(
        frame.styles.get(id(&frame, "inner")).unwrap().color,
        Some(0x800080)
    );
    assert_eq!(
        frame.styles.get(id(&frame, "orphan")).unwrap().color,
        Some(0xff0000)
    );
    assert_eq!(
        frame
            .styles
            .pseudo(id(&frame, "inner"), PseudoElement::Before)
            .unwrap()
            .width,
        Some(Length::Px(20.0))
    );
    assert_eq!(
        box_for(&tree, id(&frame, "inner")).content_rect().width,
        200.0
    );
    assert!(
        frame.stylesheets.diagnostics.is_empty(),
        "{:?}",
        frame.stylesheets.diagnostics
    );
}

#[test]
fn container_units_use_real_content_geometry_per_axis_math_variables_and_inheritance() {
    let mut frame = frame(
        r#"<style>
      #outer {container-type:size; width:500px; height:240px; padding:10px; border:2px solid}
      #inner {container-type:inline-size; width:200px}
      #card {--size:calc(10cqw + 5px); width:var(--size);
        margin:10cqw 10cqh 10cqi 10cqb; padding:10cqmin 10cqmax;
        font-size:10cqi; line-height:1}
      #child {width:1em}
      @container (width = 200px) {#card::before {content:'units'; width:calc(10cqi + 1em)}}
      </style><div id='outer'><div id='inner'><div id='card'><div id='child'>Child</div></div></div></div>"#,
    );
    let tree = layout(&mut frame);
    let card = frame.styles.get(id(&frame, "card")).unwrap();
    assert_eq!(card.width, Some(Length::Px(25.0)));
    assert_eq!(card.margin.top, Length::Px(20.0).into());
    assert_eq!(card.margin.right, Length::Px(24.0).into());
    assert_eq!(card.margin.bottom, Length::Px(20.0).into());
    assert_eq!(card.margin.left, Length::Px(24.0).into());
    assert_eq!(card.padding.top, Length::Px(20.0));
    assert_eq!(card.padding.right, Length::Px(24.0));
    assert_eq!(card.font_size, 20.0);
    assert_eq!(
        frame.styles.get(id(&frame, "child")).unwrap().width,
        Some(Length::Px(20.0))
    );
    assert_eq!(
        frame
            .styles
            .pseudo(id(&frame, "card"), PseudoElement::Before)
            .unwrap()
            .width,
        Some(Length::Px(40.0))
    );
    assert_eq!(
        box_for(&tree, id(&frame, "outer")).content_rect().width,
        500.0
    );
    assert_eq!(box_for(&tree, id(&frame, "outer")).rect().width, 524.0);
    assert!(
        frame.stylesheets.diagnostics.is_empty(),
        "{:?}",
        frame.stylesheets.diagnostics
    );
}

#[test]
fn query_threshold_container_units_use_the_selected_containers_ancestors_and_not_its_own_size() {
    let mut frame = frame(
        r#"<style>
      #outer {container:outer / size; width:500px; height:240px}
      #inner {container:inner / inline-size; width:200px; font-size:20px}
      #card {color:red}
      @container inner (width = 40cqw) and (width = 10em) {#card {color:blue}}
      @container inner (width > 50cqw) {#card {color:purple}}
      @container outer (width = 62.5cqi) {@container inner (width > 20vw) {#card {background:green}}}
      </style><div id='outer'><div id='inner'><div id='card'>Thresholds</div></div></div>"#,
    );
    layout(&mut frame);
    let card = frame.styles.get(id(&frame, "card")).unwrap();
    assert_eq!(card.color, Some(0x0000ff));
    assert_eq!(card.background_color, Some(0x008000));
    assert!(
        frame.stylesheets.diagnostics.is_empty(),
        "{:?}",
        frame.stylesheets.diagnostics
    );
}

#[test]
fn unsupported_predicates_and_non_finite_math_cannot_leak_through_boolean_negation() {
    let mut frame = frame(
        r#"<style>
      #container {container-type:size; width:200px; height:100px}
      #card {color:red}
      @container not (style(--theme:dark) and (width:0px)) {#card {color:purple}}
      @container scroll-state(stuck:top) or (width:200px) {#card {color:purple}}
      @container not ((width:calc(1px / (100vw - 800px) * 1px)) and (width:0px)) {
        #card, #card::before {color:purple; content:'invalid'}
      }
      </style><div id='container'><div id='card'>Non-finite</div></div>"#,
    );
    layout(&mut frame);
    assert_eq!(
        frame.styles.get(id(&frame, "card")).unwrap().color,
        Some(0xff0000)
    );
    assert!(
        frame
            .styles
            .pseudo(id(&frame, "card"), PseudoElement::Before)
            .is_none()
    );
    assert_eq!(
        frame
            .stylesheets
            .diagnostics
            .iter()
            .filter(|d| d.message.contains("non-finite query"))
            .count(),
        1
    );
    assert_eq!(
        frame.stylesheets.diagnostics.len(),
        3,
        "{:?}",
        frame.stylesheets.diagnostics
    );
    layout(&mut frame);
    assert_eq!(frame.stylesheets.diagnostics.len(), 3);
}

#[test]
fn missing_axes_and_non_container_inline_boxes_use_small_viewport_not_zero_or_large_viewport() {
    let mut frame = frame(
        r#"<style>
      #normal {container-name:ignored; width:100px}
      #inline {container-type:size}
      #fallback {width:10cqw; margin:10cqh; padding:10cqmin 10cqmax}
      #own {container-type:inline-size; width:50cqw}
      #child {width:10cqi; margin-top:10cqb}
      @container (width > 0px) {#fallback {color:purple}}
      </style><div id='normal'><span id='inline'><div id='fallback'>Fallback</div></span></div>
      <div id='own'><div id='child'>Child</div></div>"#,
    );
    frame.set_media_environment(MediaEnvironment {
        small_viewport: Some((300.0, 400.0)),
        large_viewport: Some((1000.0, 800.0)),
        ..MediaEnvironment::new(900.0, 700.0)
    });
    let tree = layout(&mut frame);
    let fallback = frame.styles.get(id(&frame, "fallback")).unwrap();
    assert_eq!(fallback.width, Some(Length::Px(30.0)));
    assert_eq!(fallback.margin.top, Length::Px(40.0).into());
    assert_eq!(fallback.padding.top, Length::Px(30.0));
    assert_eq!(fallback.padding.right, Length::Px(40.0));
    assert_ne!(fallback.color, Some(0x800080));
    assert_eq!(
        box_for(&tree, id(&frame, "own")).content_rect().width,
        150.0
    );
    assert_eq!(
        frame.styles.get(id(&frame, "child")).unwrap().width,
        Some(Length::Px(15.0))
    );
    assert_eq!(
        frame.styles.get(id(&frame, "child")).unwrap().margin.top,
        Length::Px(40.0).into()
    );
}

#[test]
fn fresh_feedback_on_resize_and_hover_does_not_keep_stale_dimensions_or_diagnostics() {
    let mut frame = frame(
        r#"<style>
      #container {container:component / inline-size; width:calc(50vw - 24px)}
      #container:hover {width:600px}
      #card {color:red; width:10cqw}
      @container component (width >= 320px) {#card {color:blue}}
      @media (width < 700px) {@container component (width > 500px) {#card {color:green}}}
      </style><div id='container'><div id='card'>Responsive</div></div>"#,
    );
    let container = id(&frame, "container");
    let card = id(&frame, "card");
    let retained = std::sync::Arc::clone(&frame.stylesheets.stylesheets[0].stylesheet);
    for (viewport, expected) in [
        (800.0, 376.0),
        (600.0, 276.0),
        (1000.0, 476.0),
        (600.0, 276.0),
    ] {
        frame.set_media_environment(MediaEnvironment::new(viewport, 600.0));
        let tree = layout(&mut frame);
        assert_eq!(box_for(&tree, container).content_rect().width, expected);
        let style = frame.styles.get(card).unwrap();
        let Some(Length::Px(width)) = style.width else {
            panic!("computed absolute container width")
        };
        assert!((width - expected * 0.1).abs() < 0.0001);
        assert_eq!(
            style.color,
            Some(if expected >= 320.0 {
                0x0000ff
            } else {
                0xff0000
            })
        );
        assert!(frame.stylesheets.diagnostics.is_empty());
    }
    frame.set_interaction_state(InteractionState {
        hovered: Some(container),
        ..InteractionState::default()
    });
    let tree = layout(&mut frame);
    assert_eq!(box_for(&tree, container).content_rect().width, 600.0);
    assert_eq!(frame.styles.get(card).unwrap().color, Some(0x008000));
    frame.set_interaction_state(InteractionState::default());
    layout(&mut frame);
    assert_eq!(frame.styles.get(card).unwrap().color, Some(0xff0000));
    assert!(std::sync::Arc::ptr_eq(
        &retained,
        &frame.stylesheets.stylesheets[0].stylesheet
    ));
}

#[test]
fn size_containment_removes_intrinsic_height_and_inline_size_retains_it() {
    let mut frame = frame(
        r#"<style>
      #size {container-type:size; width:200px}
      #inline {container-type:inline-size; width:200px}
      #definite {container-type:size; width:200px; height:80px}
      .child {height:120px}
      @container (height = 0px) {#zero {color:blue}}
      @container (height = 80px) {#height {color:green}}
      </style><div id='size'><div id='zero' class='child'>Overflow</div></div>
      <div id='inline'><div class='child'>Intrinsic</div></div>
      <div id='definite'><div id='height' class='child'>Overflow</div></div>"#,
    );
    let tree = layout(&mut frame);
    assert_eq!(
        box_for(&tree, id(&frame, "size")).content_rect().height,
        0.0
    );
    assert_eq!(
        box_for(&tree, id(&frame, "inline")).content_rect().height,
        120.0
    );
    assert_eq!(
        box_for(&tree, id(&frame, "definite")).content_rect().height,
        80.0
    );
    assert_eq!(
        frame.styles.get(id(&frame, "zero")).unwrap().color,
        Some(0x0000ff)
    );
    assert_eq!(
        frame.styles.get(id(&frame, "height")).unwrap().color,
        Some(0x008000)
    );
}

#[test]
fn unsupported_style_scroll_state_and_invalid_negation_do_not_match() {
    let mut frame = frame(
        r#"<style>
      #container {container-type:size; width:200px; height:100px; --theme:dark}
      #card {color:red}
      @container not style(--theme:dark) {#card {color:purple}}
      @container not scroll-state(stuck:top) {#card {color:purple}}
      @container not (200px < width > 0px) {#card {color:purple}}
      @container not (prefers-color-scheme:dark) {#card {color:purple}}
      @container not (width:var(--size)) {#card {color:purple}}
      @container not (width:calc(10cqw + 1%)) {#card {color:purple}}
      @container (width:200px), (width:100px) {#card {color:purple}}
      </style><div id='container'><div id='card'>Unsupported</div></div>"#,
    );
    layout(&mut frame);
    assert_eq!(
        frame.styles.get(id(&frame, "card")).unwrap().color,
        Some(0xff0000)
    );
    assert_eq!(
        frame.stylesheets.diagnostics.len(),
        8,
        "{:?}",
        frame.stylesheets.diagnostics
    );
}

#[test]
fn bounded_feedback_failure_is_persistent_observable_and_recoverable() {
    let mut frame = frame(
        "<style>#container {container-type:inline-size; width:200px}\
        @container (width:200px) {#card {color:blue}}</style>\
        <div id='container'><p id='card'>bounded</p></div>",
    );
    let tree = frame.layout_with_container_query_limit(&paint::load_bundled_font().unwrap(), 1);
    assert_eq!(
        box_for(&tree, id(&frame, "container")).content_rect().width,
        200.0
    );
    assert!(
        frame
            .stylesheets
            .diagnostics
            .iter()
            .any(|d| d.message.contains("did not converge within 1 passes"))
    );
    layout(&mut frame);
    assert_eq!(
        frame.styles.get(id(&frame, "card")).unwrap().color,
        Some(0x0000ff)
    );
    assert!(frame.stylesheets.diagnostics.is_empty());
}

#[test]
fn global_names_inside_valid_container_rules_are_unconditional_but_invalid_rules_do_not_register() {
    let mut frame = frame(
        r#"<style>
      @container (width > 1000px) {
        @layer early;
        @property --size {syntax:'<length>'; inherits:false; initial-value:30px}
      }
      @container (width:) {
        @layer invalid;
        @property --size {syntax:'<length>'; inherits:false; initial-value:99px}
      }
      @layer later {#card, #probe {color:blue}}
      @layer early {#card, #probe {color:red}}
      @layer invalid {#card {color:green}}
      #card {width:var(--size)}
      </style><div id='card'>Global names</div><div id='probe'>Layer order</div>"#,
    );
    layout(&mut frame);
    let card = frame.styles.get(id(&frame, "card")).unwrap();
    assert_eq!(card.width, Some(Length::Px(30.0)));
    assert_eq!(card.color, Some(0x008000));
    assert_eq!(
        frame.styles.get(id(&frame, "probe")).unwrap().color,
        Some(0x0000ff)
    );
    assert_eq!(frame.stylesheets.diagnostics.len(), 1);
}

#[test]
fn container_properties_are_typed_noninherited_and_support_wide_var_shorthand_reset() {
    let mut frame = frame(
        r#"<style>
      #outer {--type:inline-size; --names:Panel extra; container:var(--names) / var(--type); width:200px}
      #inherited {container:inherit}
      #reset {container:Reset / size; container:Reset}
      #invalid {container-type:inline-size size; container-name:not; container:bad / scroll-state}
      @supports (container:Panel / inline-size) {#plain {color:blue}}
      </style><div id='outer'><div id='plain'>Plain</div><div id='inherited'></div>
      <div id='reset'></div><div id='invalid'></div></div>"#,
    );
    layout(&mut frame);
    use crate::css::container::ContainerType as T;
    assert_eq!(
        frame
            .styles
            .get(id(&frame, "outer"))
            .unwrap()
            .container_type,
        T::InlineSize
    );
    assert_eq!(
        frame
            .styles
            .get(id(&frame, "outer"))
            .unwrap()
            .container_names,
        ["Panel", "extra"]
    );
    assert_eq!(
        frame
            .styles
            .get(id(&frame, "plain"))
            .unwrap()
            .container_type,
        T::Normal
    );
    assert_eq!(
        frame.styles.get(id(&frame, "plain")).unwrap().color,
        Some(0x0000ff)
    );
    assert_eq!(
        frame
            .styles
            .get(id(&frame, "inherited"))
            .unwrap()
            .container_type,
        T::InlineSize
    );
    assert_eq!(
        frame
            .styles
            .get(id(&frame, "reset"))
            .unwrap()
            .container_type,
        T::Normal
    );
    assert_eq!(
        frame
            .styles
            .get(id(&frame, "invalid"))
            .unwrap()
            .container_type,
        T::Normal
    );
    assert_eq!(frame.stylesheets.diagnostics.len(), 3);
}

#[cfg(feature = "style-bench-reference")]
#[test]
fn query_context_layers_registrations_scopes_nesting_and_pseudos_match_all_modes() {
    let mut frame = frame(
        r#"<style>
      @layer base, app;
      @media (width >= 600px) {@layer base {#container {container:Panel / inline-size; width:300px}}}
      @layer base {#card {color:red}}
      @container Panel (width = calc(100vw - 500px)) {
        @layer app {
          @property --size {syntax:'<length>'; inherits:false; initial-value:12px}
          @scope (#container) {
            .card {--size:20px; width:var(--size);
              @supports (container-type:size) { & > span {color:blue; width:10cqw} }
              color:green;
            }
          }
          #card::before {content:'Conditional'; color:blue}
        }
      }
      @container (width > 1000px) {@layer base {#card {color:purple}}}
      @layer base {#card {color:blue}}
      </style><div id='container'><div id='card' class='card'><span id='child'>Child</span></div></div>"#,
    );
    let tree = layout(&mut frame);
    assert_eq!(
        frame.styles.get(id(&frame, "card")).unwrap().color,
        Some(0x008000)
    );
    assert_eq!(
        frame.styles.get(id(&frame, "card")).unwrap().width,
        Some(Length::Px(20.0))
    );
    assert_eq!(
        frame.styles.get(id(&frame, "child")).unwrap().width,
        Some(Length::Px(30.0))
    );
    let snapshot = ContainerSnapshot::from_layout(&tree, frame.media_environment());
    for mode in [
        super::BenchmarkMatching::Unindexed,
        super::BenchmarkMatching::Indexed,
        super::BenchmarkMatching::IndexedWithBloom,
    ] {
        let result = super::resolve_for_benchmark_with_containers(
            &frame.document,
            super::collect_stylesheets(&frame.document),
            &[],
            mode,
            frame.media_environment(),
            frame.interaction_state(),
            &snapshot,
        );
        assert_eq!(result.styles.nodes, frame.styles.nodes);
        assert_eq!(result.styles.pseudos, frame.styles.pseudos);
        assert_eq!(
            result.stylesheets.diagnostics,
            frame.stylesheets.diagnostics
        );
    }
    assert!(
        frame.stylesheets.diagnostics.is_empty(),
        "{:?}",
        frame.stylesheets.diagnostics
    );
}

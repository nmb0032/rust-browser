use super::*;
use crate::{
    css::values::Length, dom::NodeId, engine::Frame, html::parser, layout::ScrollOffset, style,
    text::FontContext,
};

fn frame(css: &str, body: &str) -> Frame {
    Frame::new(
        url::Url::parse("https://animation.test/").unwrap(),
        parser::parse(&format!(
            "<!doctype html><style>html,body{{margin:0}}{css}</style>{body}"
        ))
        .unwrap(),
    )
}
fn find(f: &Frame, id: &str) -> NodeId {
    f.document
        .preorder()
        .find(|&n| {
            f.document
                .node(n)
                .and_then(|n| n.as_element())
                .is_some_and(|e| e.attribute("id") == Some(id))
        })
        .unwrap()
}
fn sample(f: &mut Frame, t: f64) -> crate::layout::LayoutTree {
    f.set_animation_time(t).unwrap();
    f.layout_with_container_queries(&FontContext::bundled())
}
fn width(f: &Frame, id: NodeId) -> f32 {
    f.styles
        .get(id)
        .unwrap()
        .width
        .as_ref()
        .unwrap()
        .resolve(16.0, 800.0)
}
fn close(a: f64, b: f64) {
    assert!((a - b).abs() < 0.00001, "{a} != {b}")
}
fn tokens(s: &str) -> Vec<Component> {
    components::from_tokens(&crate::css::values::math::tokenize(s).unwrap()).unwrap()
}

#[test]
fn timing_phases_fractional_iterations_negative_delay_and_exact_end() {
    let t = Timing {
        duration: 2.0,
        delay: 1.0,
        iterations: 2.5,
        fill: Fill::Both,
        ..Timing::default()
    };
    assert_eq!(t.sample(0.0).phase, Phase::Before);
    close(t.sample(0.0).progress.unwrap(), 0.0);
    close(t.sample(2.0).progress.unwrap(), 0.5);
    assert_eq!(t.sample(3.0).iteration, 1.0);
    close(t.sample(3.0).progress.unwrap(), 0.0);
    assert_eq!(t.sample(6.0).phase, Phase::After);
    close(t.sample(6.0).progress.unwrap(), 0.5);
    let t = Timing { delay: -1.0, ..t };
    close(t.sample(0.0).progress.unwrap(), 0.5);
    close(t.end_time(), 4.0);
    let t = Timing {
        fill: Fill::None,
        ..t
    };
    assert_eq!(t.sample(10.0).progress, None);
    let zero = Timing {
        duration: 0.0,
        fill: Fill::Both,
        ..Timing::default()
    };
    close(zero.sample(0.0).progress.unwrap(), 1.0);
    let zero = Timing {
        iterations: 0.0,
        ..zero
    };
    close(zero.sample(0.0).progress.unwrap(), 0.0);
}
#[test]
fn reversed_alternating_iteration_boundaries_and_infinite_sampling() {
    for (direction, expected) in [
        (Direction::Normal, 0.0),
        (Direction::Reverse, 1.0),
        (Direction::Alternate, 1.0),
        (Direction::AlternateReverse, 0.0),
    ] {
        let t = Timing {
            duration: 1.0,
            iterations: 2.0,
            direction,
            fill: Fill::Both,
            ..Timing::default()
        };
        close(t.sample(1.0).progress.unwrap(), expected);
        close(
            t.sample(2.0).progress.unwrap(),
            if matches!(direction, Direction::Reverse | Direction::Alternate) {
                0.0
            } else {
                1.0
            },
        );
    }
    let t = Timing {
        duration: 1.0,
        iterations: f64::INFINITY,
        ..Timing::default()
    };
    close(t.sample(100000000.25).progress.unwrap(), 0.25);
    assert!(t.sample(1.0).active_duration.is_infinite());
}
#[test]
fn lifecycle_pause_resume_seek_rate_finish_cancel_and_invalid_time() {
    let mut a = Animation::new(
        Timing {
            duration: 2.0,
            fill: Fill::Both,
            ..Timing::default()
        },
        3.0,
    )
    .unwrap();
    close(a.current_time(4.0).unwrap(), 1.0);
    a.pause(4.0).unwrap();
    close(a.current_time(100.0).unwrap(), 1.0);
    assert_eq!(a.play_state(100.0), PlayState::Paused);
    a.play(100.0).unwrap();
    close(a.current_time(100.5).unwrap(), 1.5);
    a.set_playback_rate(-2.0, 100.5).unwrap();
    close(a.current_time(101.0).unwrap(), 0.5);
    a.seek(1.0, 101.0).unwrap();
    close(a.current_time(101.25).unwrap(), 0.5);
    a.finish(101.25).unwrap();
    assert_eq!(a.play_state(101.25), PlayState::Finished);
    a.cancel();
    assert_eq!(a.current_time(200.0), None);
    assert_eq!(a.play_state(200.0), PlayState::Idle);
    a.play(200.0).unwrap();
    close(a.current_time(200.0).unwrap(), 2.0);
    assert!(a.seek(f64::NAN, 200.0).is_err());
    assert!(a.set_playback_rate(f64::INFINITY, 200.0).is_err());
    a.set_playback_rate(0.0, 200.0).unwrap();
    assert!(a.finish(200.0).is_err());
}
#[test]
fn easing_steps_jump_variants_before_flag_and_bezier_accuracy() {
    close(Easing::Steps(4, Jump::End).sample(0.25, false), 0.25);
    close(Easing::Steps(4, Jump::End).sample(0.25, true), 0.0);
    close(Easing::Steps(4, Jump::Start).sample(0.0, false), 0.25);
    close(Easing::Steps(4, Jump::Both).sample(0.0, false), 0.2);
    close(Easing::Steps(4, Jump::None).sample(1.0, false), 1.0);
    close(Easing::Bezier([0.0, 0.0, 1.0, 1.0]).sample(0.3, false), 0.3);
    close(Easing::default().sample(1.0, false), 1.0);
    assert!(easing(&tokens("steps(1,jump-none)")[0]).is_none());
    assert!(easing(&tokens("cubic-bezier(-1,0,1,1)")[0]).is_none());
    assert!(easing(&tokens("linear(0, 1)")[0]).is_none());
}
#[test]
fn animation_shorthand_is_atomic_keyword_ambiguous_and_preserves_case() {
    let p = style::declarations::properties("animation").unwrap();
    let a = shorthand(
        "animation",
        &p,
        &tokens("1s linear linear, 2s reverse reverse"),
    )
    .unwrap();
    let index = p
        .iter()
        .position(|p| *p == style::declarations::Property::Animation(Property::Name))
        .unwrap();
    assert!(
        matches!(&a[index],style::declarations::Value::Animation(Value::AnimationNames(v)) if v==&[AnimationName::Name("linear".into()),AnimationName::Name("reverse".into())])
    );
    for invalid in [
        "-1s move",
        "1s 2s 3s 4s move",
        "1s ease ease ease",
        "1s move,",
        "steps(0) move",
        "1s -2 move",
    ] {
        assert!(
            shorthand("animation", &p, &tokens(invalid)).is_none(),
            "{invalid}"
        );
    }
    assert!(
        matches!(parse(Property::Name,&tokens("Spin, spin")),Some(Value::AnimationNames(v)) if v==[AnimationName::Name("Spin".into()),AnimationName::Name("spin".into())])
    );
    assert!(parse(Property::Duration, &tokens("-1s")).is_none());
    assert!(parse(Property::Iterations, &tokens("-1")).is_none());
}
#[test]
fn production_frame_animates_layout_fonts_inheritance_transforms_and_opacity() {
    let mut f = frame(
        "@keyframes move{from{width:20px;font-size:10px;transform:translateX(0px);opacity:1}to{width:60px;font-size:30px;transform:translateX(40px);opacity:.5}}#a{animation:2s linear both move;height:20px;background:red}",
        "<div id=a><span id=b>Text</span></div>",
    );
    let a = find(&f, "a");
    let b = find(&f, "b");
    sample(&mut f, 0.0);
    let tree = sample(&mut f, 1.0);
    close(width(&f, a).into(), 40.0);
    close(f.styles.get(a).unwrap().font_size.into(), 20.0);
    close(f.styles.get(b).unwrap().font_size.into(), 20.0);
    close(tree.node_rect(a).unwrap().width.into(), 40.0);
    close(
        f.styles
            .get(a)
            .unwrap()
            .effects
            .number(crate::style::effects::Property::Opacity)
            .into(),
        0.75,
    );
    close(f.author_styles.get(a).unwrap().font_size.into(), 16.0);
    let transforms = f
        .styles
        .get(a)
        .unwrap()
        .effects
        .transforms(crate::style::effects::Property::Transform);
    assert!(
        matches!(&transforms[0],crate::style::effects::Transform::Translate(v) if v[0]==Length::Px(20.0))
    );
}
#[test]
fn missing_endpoints_duplicate_offsets_segment_easing_variables_and_important() {
    let mut f = frame(
        "@keyframes k{50%{width:60px}50%{height:var(--height);animation-timing-function:steps(1,start)}to{width:100px!important}}#a{width:20px;height:20px;--height:40px;animation:k 2s linear both}",
        "<div id=a></div>",
    );
    let a = find(&f, "a");
    sample(&mut f, 0.0);
    sample(&mut f, 0.5);
    close(width(&f, a).into(), 40.0);
    close(
        f.styles
            .get(a)
            .unwrap()
            .height
            .as_ref()
            .unwrap()
            .resolve(16.0, 600.0)
            .into(),
        30.0,
    );
    sample(&mut f, 1.5);
    close(width(&f, a).into(), 20.0);
    let mut f = frame(
        "@keyframes k{to{width:100px;color:blue}}#a{width:20px!important;color:red!important;animation:k 2s linear both}",
        "<div id=a></div>",
    );
    let a = find(&f, "a");
    sample(&mut f, 0.0);
    sample(&mut f, 1.0);
    close(width(&f, a).into(), 20.0);
    assert_eq!(f.styles.get(a).unwrap().color, Some(0xff0000));
}
#[test]
fn conditional_keyframes_layer_precedence_and_pseudo_effects() {
    let mut f = frame(
        "@layer low{@keyframes k{to{width:30px}}} @keyframes k{to{width:100px}}@media(max-width:400px){@keyframes k{to{width:200px}}}#a{width:0px;animation:2s linear both k}#a::before{content:'';display:block;width:10px;height:10px;animation:2s linear both k}",
        "<div id=a></div>",
    );
    let a = find(&f, "a");
    sample(&mut f, 0.0);
    sample(&mut f, 1.0);
    close(width(&f, a).into(), 50.0);
    close(
        f.styles
            .pseudo(a, style::PseudoElement::Before)
            .unwrap()
            .width
            .as_ref()
            .unwrap()
            .resolve(16.0, 800.0)
            .into(),
        55.0,
    );
    let mut env = f.media_environment();
    env.viewport_width = 300.0;
    f.set_media_environment(env);
    sample(&mut f, 1.0);
    close(width(&f, a).into(), 100.0);
}
#[test]
fn css_pause_restyle_retains_created_time_clones_and_new_navigation() {
    let css = "@keyframes k{from{width:0px}to{width:100px}}#a{animation:k 2s linear both}#a:hover{animation-play-state:paused}";
    let mut f = frame(css, "<div id=a></div>");
    let a = find(&f, "a");
    sample(&mut f, 0.0);
    sample(&mut f, 0.5);
    let ids = f.animations().effect_ids(a, None);
    let mut cloned = f.clone();
    sample(&mut cloned, 1.0);
    assert_eq!(cloned.animations().effect_ids(a, None), ids);
    close(width(&cloned, a).into(), 50.0);
    close(width(&f, a).into(), 25.0);
    let mut interaction = f.interaction_state().clone();
    interaction.hovered = Some(a);
    f.set_interaction_state(interaction);
    sample(&mut f, 0.5);
    sample(&mut f, 1.5);
    close(width(&f, a).into(), 25.0);
    f.set_interaction_state(style::InteractionState::default());
    sample(&mut f, 1.5);
    sample(&mut f, 2.0);
    close(width(&f, a).into(), 50.0);
    let mut other = frame(css, "<div id=a></div>");
    sample(&mut other, 0.0);
    assert!(!other.same_animation_document(&f));
    close(width(&other, a).into(), 0.0);
}
#[test]
fn rust_effect_seek_cancel_and_events_affect_real_styles() {
    let mut f = frame(
        "@keyframes k{from{width:0px}to{width:100px}}#a{width:7px;animation:k 2s linear both}",
        "<div id=a></div>",
    );
    let a = find(&f, "a");
    sample(&mut f, 0.0);
    let id = f.animations().effect_ids(a, None)[0];
    f.animations_mut()
        .animation_mut(id)
        .unwrap()
        .seek(1.0, 0.0)
        .unwrap();
    sample(&mut f, 0.0);
    close(width(&f, a).into(), 50.0);
    f.animations_mut().animation_mut(id).unwrap().cancel();
    sample(&mut f, 0.0);
    close(width(&f, a).into(), 7.0);
    assert!(
        f.animations_mut()
            .take_events()
            .iter()
            .any(|e| e.kind == EventKind::Run)
    );
}
#[test]
fn transitions_initial_load_interruption_reversal_last_list_match_and_important() {
    let mut f = frame(
        "#a{width:0px!important;transition:all 4s linear,width 2s linear}#a:hover{width:100px!important}",
        "<div id=a></div>",
    );
    let a = find(&f, "a");
    sample(&mut f, 0.0);
    assert!(f.animations().effect_ids(a, None).is_empty());
    let mut state = f.interaction_state().clone();
    state.hovered = Some(a);
    f.set_interaction_state(state);
    sample(&mut f, 0.0);
    sample(&mut f, 0.5);
    close(width(&f, a).into(), 25.0);
    f.set_interaction_state(style::InteractionState::default());
    sample(&mut f, 0.5);
    close(width(&f, a).into(), 25.0);
    sample(&mut f, 0.75);
    close(width(&f, a).into(), 12.5);
    sample(&mut f, 1.0);
    close(width(&f, a).into(), 0.0);
}
#[test]
fn transition_cancels_on_display_none_and_pseudo_recalculation() {
    let mut f = frame(
        "#a{width:0px;transition:width 2s linear}#a:hover{width:100px}#a:active{display:none}#a::before{content:'';width:0px;display:block;transition:width 2s linear}#a:hover::before{width:100px}",
        "<div id=a></div>",
    );
    let a = find(&f, "a");
    sample(&mut f, 0.0);
    let mut s = f.interaction_state().clone();
    s.hovered = Some(a);
    f.set_interaction_state(s.clone());
    sample(&mut f, 0.0);
    sample(&mut f, 1.0);
    close(width(&f, a).into(), 50.0);
    close(
        f.styles
            .pseudo(a, style::PseudoElement::Before)
            .unwrap()
            .width
            .as_ref()
            .unwrap()
            .resolve(16.0, 800.0)
            .into(),
        50.0,
    );
    s.active = Some(a);
    f.set_interaction_state(s);
    sample(&mut f, 1.0);
    assert!(f.animations().effect_ids(a, None).is_empty());
}
#[test]
fn scroll_root_and_nested_nearest_timeline_are_geometry_driven_and_idle() {
    let mut f = frame(
        "@keyframes k{from{width:0px}to{width:100px}}#p{width:200px;height:100px;overflow:auto}#a{height:300px;animation:k linear both;animation-timeline:scroll(nearest block)}#tail{height:1000px}",
        "<div id=p><div id=a></div></div><div id=tail></div>",
    );
    let a = find(&f, "a");
    let p = find(&f, "p");
    let mut tree = sample(&mut f, 0.0);
    tree.set_viewport_scroll(ScrollOffset { x: 0.0, y: 100.0 });
    tree.set_scroll_offset(p, ScrollOffset { x: 0.0, y: 100.0 });
    f.retain_scroll_state(&tree);
    f.sample_scroll_animations(tree, &FontContext::bundled());
    close(width(&f, a).into(), 50.0);
    assert!(!f.animations().needs_tick());
    assert!(f.animations().has_scroll_effects());
    sample(&mut f, 50.0);
    close(width(&f, a).into(), 50.0);
}
#[test]
fn named_scroll_timeline_ranges_and_inactive_zero_range() {
    let mut f = frame(
        "@keyframes k{from{width:0px}to{width:100px}}#p{width:200px;height:100px;overflow:auto;scroll-timeline:--p block}#a{height:500px;width:9px;animation:k linear both;animation-timeline:--p;animation-range:25% 75%}",
        "<div id=p><div id=a></div></div>",
    );
    let p = find(&f, "p");
    let a = find(&f, "a");
    let mut tree = sample(&mut f, 0.0);
    tree.set_scroll_offset(p, ScrollOffset { x: 0.0, y: 200.0 });
    f.retain_scroll_state(&tree);
    f.sample_scroll_animations(tree, &FontContext::bundled());
    close(width(&f, a).into(), 50.0);
    let mut f = frame(
        "@keyframes k{to{width:100px}}#a{width:9px;animation:k linear both;animation-timeline:scroll(root block)}",
        "<div id=a></div>",
    );
    let a = find(&f, "a");
    sample(&mut f, 0.0);
    close(width(&f, a).into(), 9.0);
}
#[test]
fn view_progress_entry_exit_contain_and_insets_use_subject_not_other_nodes() {
    let mut f = frame(
        "@keyframes k{from{opacity:0}to{opacity:1}}#p{height:100px;overflow:auto}#before{height:100px}#a{height:100px;animation:k linear both;animation-timeline:view(block)}#after{height:200px}",
        "<div id=p><div id=before></div><div id=a></div><div id=after></div></div>",
    );
    let p = find(&f, "p");
    let a = find(&f, "a");
    let mut tree = sample(&mut f, 0.0);
    tree.set_scroll_offset(p, ScrollOffset { x: 0.0, y: 100.0 });
    f.retain_scroll_state(&tree);
    f.sample_scroll_animations(tree, &FontContext::bundled());
    close(
        f.styles
            .get(a)
            .unwrap()
            .effects
            .number(crate::style::effects::Property::Opacity)
            .into(),
        0.5,
    );
}
#[test]
fn color_interpolation_is_premultiplied_oklab_and_filters_interpolate() {
    use style::declarations::Value as V;
    let a = V::Color(crate::css::values::Color::Rgb(0xff0000));
    let b = V::Color(crate::css::values::Color::Rgb(0x0000ff));
    let V::Color(c) = effects::interpolate(&a, &b, 0.5).unwrap() else {
        panic!()
    };
    let c = c.resolve(crate::css::values::ResolvedColor::BLACK).unwrap();
    assert_eq!(c.space, crate::css::values::ColorSpace::Oklab);
    let mut f = frame(
        "@keyframes k{from{filter:brightness(1)}to{filter:brightness(2)}}#a{animation:k 2s linear both}",
        "<div id=a></div>",
    );
    let a = find(&f, "a");
    sample(&mut f, 0.0);
    sample(&mut f, 1.0);
    assert_eq!(
        f.styles
            .get(a)
            .unwrap()
            .effects
            .filters(crate::style::effects::Property::Filter),
        vec![crate::style::effects::Filter::Brightness(1.5)]
    );
}
#[test]
fn malformed_keyframes_and_unsupported_matrix_pairs_report_diagnostics() {
    let mut f = frame(
        "@keyframes bad{101%{width:2px}}@keyframes k{from{transform:matrix(1,0,0,1,0,0)}to{transform:matrix(1,0,0,1,20,0)}}#a{animation:k 2s linear both}",
        "<div id=a></div>",
    );
    sample(&mut f, 0.0);
    sample(&mut f, 1.0);
    assert!(
        f.stylesheets
            .diagnostics
            .iter()
            .any(|d| d.message.contains("offset outside"))
    );
    assert!(
        f.stylesheets
            .diagnostics
            .iter()
            .any(|d| d.message.contains("discrete"))
    );
}

#[test]
fn zero_duration_infinite_iterations_are_finite_and_do_not_spin() {
    let t = Timing {
        iterations: f64::INFINITY,
        fill: Fill::Both,
        ..Timing::default()
    };
    assert!(t.sample(0.0).progress.unwrap().is_finite());
    let mut f = frame(
        "@keyframes k{from{opacity:0}to{opacity:1}}#a{animation:k 0s infinite both}",
        "<div id=a></div>",
    );
    let a = find(&f, "a");
    sample(&mut f, 0.0);
    assert!(
        f.styles
            .get(a)
            .unwrap()
            .effects
            .number(crate::style::effects::Property::Opacity)
            .is_finite()
    );
    assert!(!f.animations().needs_tick());
}
#[test]
fn mixed_math_percentage_font_units_and_font_style_animate_before_layout() {
    let mut f = frame(
        "@keyframes k{from{font-size:10px;width:calc(10% + 2em);font-style:normal}to{font-size:30px;width:calc(30% + 2em);font-style:italic}}#a{animation:k 2s linear both}",
        "<div id=a>Text</div>",
    );
    let a = find(&f, "a");
    sample(&mut f, 0.0);
    let tree = sample(&mut f, 1.0);
    close(width(&f, a).into(), 200.0);
    assert!((tree.node_rect(a).unwrap().width - 190.4).abs() < 0.0001);
    close(f.styles.get(a).unwrap().font_size.into(), 20.0);
    assert!(f.styles.get(a).unwrap().italic);
}
#[test]
fn transition_priority_beats_important_and_running_animation_without_resetting_it() {
    let mut f = frame(
        "@keyframes k{from{width:0px}to{width:100px}}#a{width:0px;animation:k 2s linear both;transition:width 2s linear}#a:hover{width:200px!important}",
        "<div id=a></div>",
    );
    let a = find(&f, "a");
    sample(&mut f, 0.0);
    sample(&mut f, 1.0);
    let id = f.animations().effect_ids(a, None)[0];
    close(width(&f, a).into(), 50.0);
    let mut state = f.interaction_state().clone();
    state.hovered = Some(a);
    f.set_interaction_state(state);
    sample(&mut f, 1.0);
    close(width(&f, a).into(), 50.0);
    sample(&mut f, 1.5);
    close(width(&f, a).into(), 87.5);
    assert!(f.animations().effect_ids(a, None).contains(&id));
    sample(&mut f, 3.0);
    close(width(&f, a).into(), 200.0);
}
#[test]
fn ancestor_display_none_cancels_descendant_effects_and_restart_is_new() {
    let mut f = frame(
        "@keyframes k{to{opacity:0}}#a{animation:k 2s linear both}#p:hover{display:none}",
        "<div id=p><div id=a></div></div>",
    );
    let p = find(&f, "p");
    let a = find(&f, "a");
    sample(&mut f, 0.0);
    let old = f.animations().effect_ids(a, None)[0];
    let mut state = f.interaction_state().clone();
    state.hovered = Some(p);
    f.set_interaction_state(state);
    sample(&mut f, 1.0);
    assert!(f.animations().effect_ids(a, None).is_empty());
    f.set_interaction_state(style::InteractionState::default());
    sample(&mut f, 1.0);
    assert_ne!(f.animations().effect_ids(a, None)[0], old);
}
#[test]
fn vertical_negative_origin_scroll_and_stale_source_removal_are_sampled() {
    let mut f = frame(
        "@keyframes k{from{opacity:0}to{opacity:1}}#p{writing-mode:vertical-rl;width:100px;height:100px;overflow:auto;scroll-timeline:--p block}#a{width:300px;height:100px;animation:k linear both;animation-timeline:--p}@media(max-width:400px){#p{overflow:clip}}",
        "<div id=p><div id=a></div></div>",
    );
    let p = find(&f, "p");
    let a = find(&f, "a");
    let mut tree = sample(&mut f, 0.0);
    assert!(tree.scroll_container(p).unwrap().range.min.x < 0.0);
    assert!(tree.set_scroll_offset(p, ScrollOffset { x: -100.0, y: 0.0 }));
    f.retain_scroll_state(&tree);
    f.sample_scroll_animations(tree, &FontContext::bundled());
    close(
        f.styles
            .get(a)
            .unwrap()
            .effects
            .number(crate::style::effects::Property::Opacity)
            .into(),
        0.5,
    );
    let mut environment = f.media_environment();
    environment.viewport_width = 300.0;
    f.set_media_environment(environment);
    sample(&mut f, 0.0);
    close(
        f.styles
            .get(a)
            .unwrap()
            .effects
            .number(crate::style::effects::Property::Opacity)
            .into(),
        1.0,
    );
}
#[test]
fn view_entry_range_and_named_view_inset_use_correct_geometry() {
    for (timeline, range, offset) in [
        ("view(block)", "entry 0% entry 100%", 50.0),
        ("--v", "cover 0% cover 100%", 95.0),
    ] {
        let mut f = frame(
            &format!(
                "@keyframes k{{from{{opacity:0}}to{{opacity:1}}}}#p{{height:100px;overflow:auto}}#before{{height:100px}}#a{{height:100px;view-timeline:--v block;view-timeline-inset:20% 10px;animation:k linear both;animation-timeline:{timeline};animation-range:{range}}}#after{{height:200px}}"
            ),
            "<div id=p><div id=before></div><div id=a></div><div id=after></div></div>",
        );
        let a = find(&f, "a");
        let p = find(&f, "p");
        let mut tree = sample(&mut f, 0.0);
        tree.set_scroll_offset(p, ScrollOffset { x: 0.0, y: offset });
        f.retain_scroll_state(&tree);
        f.sample_scroll_animations(tree, &FontContext::bundled());
        close(
            f.styles
                .get(a)
                .unwrap()
                .effects
                .number(crate::style::effects::Property::Opacity)
                .into(),
            0.5,
        );
    }
}
#[test]
fn animation_shorthand_resets_ranges_and_custom_variable_shorthand_is_atomic() {
    let f = frame(
        "#a{--effect:k 2s linear both;animation-range:entry 20% exit 80%;animation:var(--effect)}",
        "<div id=a></div>",
    );
    let a = find(&f, "a");
    assert!(
        matches!(f.styles.get(a).unwrap().motion.get(Property::RangeStart),Value::Ranges(v) if v[0].name==RangeName::Normal)
    );
    assert!(
        matches!(f.styles.get(a).unwrap().motion.get(Property::Name),Value::AnimationNames(v) if v==[AnimationName::Name("k".into())])
    );
}
#[test]
fn resource_stylesheet_updates_keep_animations_and_start_real_transitions() {
    let mut f = frame(
        "@keyframes k{from{opacity:0}to{opacity:1}}#a{width:20px;transition:width 2s linear;animation:k 2s linear both}",
        "<div id=a></div>",
    );
    let a = find(&f, "a");
    sample(&mut f, 0.0);
    sample(&mut f, 1.0);
    let id = f.animations().effect_ids(a, None)[0];
    let mut sheets = f.stylesheets.clone();
    sheets.stylesheets[0].stylesheet=std::sync::Arc::new(crate::css::parse("@keyframes k{from{opacity:0}to{opacity:1}}#a{width:100px;transition:width 2s linear;animation:k 2s linear both}").unwrap());
    f.replace_author_stylesheets(sheets);
    sample(&mut f, 1.0);
    close(width(&f, a).into(), 20.0);
    sample(&mut f, 2.0);
    close(width(&f, a).into(), 60.0);
    assert!(f.animations().effect_ids(a, None).contains(&id));
}
#[test]
fn progressive_loader_snapshots_share_identity_but_separate_loads_never_do() {
    let url = url::Url::from_file_path(
        std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/reftests-animations/moving-box.html"),
    )
    .unwrap();
    let mut retained: Option<Frame> = None;
    let finished = crate::engine::load_page(
        &url,
        crate::engine::LoadOptions::default(),
        |progress| {
            let mut progress = progress.clone();
            if let Some(previous) = &retained {
                assert!(progress.same_animation_document(previous));
                progress.copy_scroll_state_from(previous);
            }
            sample(&mut progress, 0.0);
            retained = Some(progress);
            Ok(())
        },
        || false,
    )
    .unwrap();
    assert!(finished.same_animation_document(retained.as_ref().unwrap()));
    let next = crate::engine::load_page(
        &url,
        crate::engine::LoadOptions::default(),
        |_| Ok(()),
        || false,
    )
    .unwrap();
    assert!(!finished.same_animation_document(&next));
}

#[test]
fn keyframe_invalid_declarations_recover_and_quoted_none_is_a_real_name() {
    let mut f = frame(
        "@keyframes \"none\"{0%,50%{width:20px;unknown-property:x;opacity:0}101%{width:1000px}to{width:100px;opacity:1}}#a{animation:2s linear both \"none\"}",
        "<div id=a></div>",
    );
    let a = find(&f, "a");
    sample(&mut f, 0.0);
    sample(&mut f, 1.5);
    close(width(&f, a).into(), 60.0);
    assert!(
        f.stylesheets
            .diagnostics
            .iter()
            .any(|d| d.message.contains("declaration ignored"))
    );
    assert!(parse(Property::TransitionProperty, &tokens("\"width\"")).is_none());
}
#[test]
fn relative_keyframe_units_follow_sampled_font_not_each_offset_font() {
    let mut f = frame(
        "@keyframes k{from{font-size:10px;width:2em}to{font-size:30px;width:4em}}#a{animation:k 2s linear both}",
        "<div id=a></div>",
    );
    let a = find(&f, "a");
    sample(&mut f, 0.0);
    sample(&mut f, 1.0);
    close(f.styles.get(a).unwrap().font_size.into(), 20.0);
    close(width(&f, a).into(), 60.0);
}

#[test]
fn reversed_steps_start_at_the_end_and_transition_uses_actual_important_snapshot() {
    let mut f = frame(
        "@keyframes k{from{width:0px}to{width:100px}}#a{animation:k 2s steps(4,end) reverse both}",
        "<div id=a></div>",
    );
    let a = find(&f, "a");
    sample(&mut f, 0.0);
    close(width(&f, a).into(), 100.0);
    let mut f = frame(
        "@keyframes k{from{width:0px}to{width:100px}}#a{width:20px!important;animation:k 2s linear both;transition:width 2s linear}#a:hover{width:200px!important}",
        "<div id=a></div>",
    );
    let a = find(&f, "a");
    sample(&mut f, 0.0);
    sample(&mut f, 1.0);
    let mut state = f.interaction_state().clone();
    state.hovered = Some(a);
    f.set_interaction_state(state);
    sample(&mut f, 1.0);
    close(width(&f, a).into(), 20.0);
    sample(&mut f, 1.5);
    close(width(&f, a).into(), 65.0);
}

#[test]
fn native_timing_validation_zero_rate_and_negative_playback_end_boundaries() {
    for timing in [
        Timing {
            duration: -1.0,
            ..Timing::default()
        },
        Timing {
            duration: f64::INFINITY,
            ..Timing::default()
        },
        Timing {
            iterations: f64::NAN,
            ..Timing::default()
        },
        Timing {
            end_delay: -1.0,
            ..Timing::default()
        },
    ] {
        assert!(Animation::new(timing, 0.0).is_err());
    }
    let mut a = Animation::new(
        Timing {
            duration: 2.0,
            ..Timing::default()
        },
        0.0,
    )
    .unwrap();
    a.set_playback_rate(0.0, 1.0).unwrap();
    assert_eq!(a.play_state(2.0), PlayState::Running);
    close(a.current_time(2.0).unwrap(), 1.0);
    a.pause(2.0).unwrap();
    a.set_playback_rate(1.0, 2.0).unwrap();
    assert_eq!(a.play_state(4.0), PlayState::Paused);
    a.play(4.0).unwrap();
    a.seek(2.0, 4.0).unwrap();
    a.set_playback_rate(-1.0, 4.0).unwrap();
    assert_eq!(a.computed_timing(4.0).unwrap().phase, Phase::Active);
    close(a.computed_timing(4.0).unwrap().progress.unwrap(), 1.0);
    assert_eq!(a.computed_timing(6.0).unwrap().phase, Phase::Before);
    assert!(a.pause(f64::NAN).is_err());
    assert!(a.play(f64::NAN).is_err());
    assert!(a.set_playback_rate(1.0, f64::INFINITY).is_err());
    a.update_timing(Timing {
        duration: 3.0,
        ..Timing::default()
    })
    .unwrap();
    close(a.timing().duration, 3.0);
}

#[test]
fn native_events_report_active_seconds_not_document_clock_or_iteration_numbers() {
    let mut f = frame(
        "@keyframes k{from{width:0px}to{width:100px}}#a{animation:k 2s linear -0.5s 3 both}",
        "<div id=a></div>",
    );
    sample(&mut f, 100.0);
    let events = f.animations_mut().take_events();
    close(
        events
            .iter()
            .find(|e| e.kind == EventKind::Start)
            .unwrap()
            .elapsed_time,
        0.5,
    );
    sample(&mut f, 102.0);
    close(
        f.animations_mut()
            .take_events()
            .iter()
            .find(|e| e.kind == EventKind::Iteration)
            .unwrap()
            .elapsed_time,
        2.0,
    );
    sample(&mut f, 106.0);
    close(
        f.animations_mut()
            .take_events()
            .iter()
            .find(|e| e.kind == EventKind::Finish)
            .unwrap()
            .elapsed_time,
        6.0,
    );
}

#[test]
fn container_style_changes_start_transitions_and_view_insets_compute_viewport_units() {
    let mut f = frame(
        "#p{width:200px;container-type:inline-size}#p:hover{width:400px}#a{width:20px;transition:width 2s linear}@container(min-width:300px){#a{width:100px}}",
        "<div id=p><div id=a></div></div>",
    );
    let p = find(&f, "p");
    let a = find(&f, "a");
    sample(&mut f, 0.0);
    let mut state = f.interaction_state().clone();
    state.hovered = Some(p);
    f.set_interaction_state(state);
    sample(&mut f, 0.0);
    sample(&mut f, 1.0);
    close(width(&f, a).into(), 60.0);
    let mut f = frame(
        "@keyframes k{from{opacity:0}to{opacity:1}}#p{height:100px;overflow:auto}#before{height:100px}#a{height:100px;animation:k linear both;animation-timeline:view(block 10vh 0px)}#after{height:200px}",
        "<div id=p><div id=before></div><div id=a></div><div id=after></div></div>",
    );
    let p = find(&f, "p");
    let a = find(&f, "a");
    let mut tree = sample(&mut f, 0.0);
    tree.set_scroll_offset(p, ScrollOffset { x: 0.0, y: 70.0 });
    f.retain_scroll_state(&tree);
    f.sample_scroll_animations(tree, &FontContext::bundled());
    close(
        f.styles
            .get(a)
            .unwrap()
            .effects
            .number(crate::style::effects::Property::Opacity)
            .into(),
        0.5,
    );
}

#[test]
fn overshooting_easing_clamps_opacity_and_filter_amounts_but_not_layout_progress() {
    let mut f = frame(
        "@keyframes k{from{width:0px;opacity:0;filter:grayscale(0)}to{width:100px;opacity:1;filter:grayscale(1)}}#a{animation:k 2s cubic-bezier(0,2,1,2) both}",
        "<div id=a></div>",
    );
    let a = find(&f, "a");
    sample(&mut f, 0.0);
    sample(&mut f, 1.0);
    close(width(&f, a).into(), 162.5);
    close(
        f.styles
            .get(a)
            .unwrap()
            .effects
            .number(crate::style::effects::Property::Opacity)
            .into(),
        1.0,
    );
    assert_eq!(
        f.styles
            .get(a)
            .unwrap()
            .effects
            .filters(crate::style::effects::Property::Filter),
        [crate::style::effects::Filter::Grayscale(1.0)]
    );
    assert!(
        f.stylesheets.diagnostics.is_empty(),
        "{:?}",
        f.stylesheets.diagnostics
    );
}

#[test]
fn scroll_pause_retains_timeline_time_and_native_timing_updates_survive_identical_restyle() {
    let mut f = frame(
        "@keyframes k{from{width:0px}to{width:100px}}#p{height:100px;overflow:auto}#a{height:400px;animation:k linear both;animation-timeline:scroll()}#a:hover{animation-play-state:paused}",
        "<div id=p><div id=a></div></div>",
    );
    let a = find(&f, "a");
    let p = find(&f, "p");
    let mut tree = sample(&mut f, 0.0);
    tree.set_scroll_offset(p, ScrollOffset { x: 0.0, y: 150.0 });
    f.retain_scroll_state(&tree);
    f.sample_scroll_animations(tree, &FontContext::bundled());
    close(width(&f, a).into(), 50.0);
    let mut state = f.interaction_state().clone();
    state.hovered = Some(a);
    f.set_interaction_state(state);
    let mut tree = sample(&mut f, 0.0);
    tree.set_scroll_offset(p, ScrollOffset { x: 0.0, y: 300.0 });
    f.retain_scroll_state(&tree);
    f.sample_scroll_animations(tree, &FontContext::bundled());
    close(width(&f, a).into(), 50.0);
    let mut state = f.interaction_state().clone();
    state.hovered = None;
    f.set_interaction_state(state);
    sample(&mut f, 0.0);
    close(width(&f, a).into(), 100.0);
    let mut f = frame(
        "@keyframes k{from{width:0px}to{width:100px}}#a{animation:k 2s linear both}",
        "<div id=a></div>",
    );
    let a = find(&f, "a");
    sample(&mut f, 0.0);
    let id = f.animations().effect_ids(a, None)[0];
    f.animations_mut()
        .animation_mut(id)
        .unwrap()
        .update_timing(Timing {
            duration: 4.0,
            fill: Fill::Both,
            ..Timing::default()
        })
        .unwrap();
    sample(&mut f, 1.0);
    close(width(&f, a).into(), 25.0);
}

#[test]
fn legitimate_discrete_keywords_switch_at_half_progress_without_fallback_diagnostics() {
    let mut f = frame(
        "@keyframes k{from{font-style:normal;overflow:visible;width:auto}to{font-style:italic;overflow:hidden;width:100px}}#a{animation:k 2s linear both}",
        "<div id=a>Text</div>",
    );
    let a = find(&f, "a");
    sample(&mut f, 0.0);
    sample(&mut f, 0.5);
    assert!(!f.styles.get(a).unwrap().italic);
    assert!(f.styles.get(a).unwrap().width.is_none());
    sample(&mut f, 1.0);
    assert!(f.styles.get(a).unwrap().italic);
    close(width(&f, a).into(), 100.0);
    assert_eq!(
        f.styles.get(a).unwrap().overflow_x,
        crate::style::Overflow::Hidden
    );
    assert!(
        f.stylesheets.diagnostics.is_empty(),
        "{:?}",
        f.stylesheets.diagnostics
    );
}

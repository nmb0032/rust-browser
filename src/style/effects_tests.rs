use super::{
    declarations,
    effects::{self, Property, Value},
    resolve,
};
use crate::{css, html::parser};

#[test]
fn effects_grammar_is_typed_atomic_finite_and_complete_for_functions() {
    for source in [
        "transform:matrix(1,0,0,1,20,30) matrix3d(1,0,0,0,0,1,0,0,0,0,1,0,2,3,4,1)",
        "transform:translateX(10%) translateY(2em) translateZ(3px) translate3d(2%,3%,4px)",
        "transform:scaleX(2) scaleY(3) scaleZ(1) scale3d(1,2,3)",
        "transform:rotateX(.25turn) rotateY(1rad) rotateZ(0deg) rotate3d(1,2,3,90deg) skew(20deg,30deg) skewX(0) skewY(0rad) perspective(10px)",
        "filter:blur(1px) brightness(2) contrast(.5) grayscale(50%) sepia(1) invert(1) opacity(.5) saturate(2) hue-rotate(90deg) drop-shadow(red 2px 4px 1px)",
        "clip-path:polygon(evenodd,0 0,100% 0,100% 100%,0 100%)",
        "clip-path:inset(10% 2px 3px)",
        "clip-path:circle(closest-side at right bottom)",
        "clip-path:ellipse(20% 30% at 50% 50%)",
        "mask:linear-gradient(black,transparent) center / 50% auto no-repeat padding-box content-box subtract alpha, none",
        "mask-image:radial-gradient(white,transparent)",
    ] {
        let v = css::parse_declarations(source).unwrap();
        assert!(declarations::parse(&v[0], 0).is_ok(), "{source}");
    }
    for source in [
        "transform:matrix(1,0,0,1,0)",
        "transform:rotate(1)",
        "transform:rotate(NaN)",
        "transform:translateZ(10%)",
        "transform:translate3d(0,0,10%)",
        "transform:matrix(calc(1 / 0),0,0,1,0,0)",
        "transform:scale(1e999)",
        "transform-origin:top left 10%",
        "transform-origin:left right",
        "perspective:-1px",
        "filter:blur(-1px)",
        "filter:brightness(-1)",
        "filter:drop-shadow(1px 2px 3px 4px red)",
        "filter:url(unknown.svg)",
        "clip-path:path('M0 0')",
        "clip-path:inset(0 round 5px)",
        "mask:linear-gradient(black,white) / 10px",
        "mask:alpha luminance",
        "mask-repeat:repeat,",
        "mask-size:-1px",
        "mask-image:conic-gradient(white,black)",
    ] {
        let v = css::parse_declarations(source).unwrap();
        assert!(declarations::parse(&v[0], 0).is_err(), "{source}");
    }
}

#[test]
fn effects_use_pending_variables_cascade_rollback_units_and_noninheritance() {
    let d=parser::parse("<style>@layer base,top;@layer base{#a{opacity:.5;transform:translate(calc(1em + 10%),2px)}}@layer top{#a{opacity:revert-layer}}#a{--m:linear-gradient(black,transparent);mask:var(--m) center / 50% auto no-repeat;font-size:20px}#b{transform:inherit;opacity:initial}</style><div id=a><div id=b></div><div id=c></div></div>").unwrap();
    let result = resolve(&d);
    assert!(
        result.stylesheets.diagnostics.is_empty(),
        "{:?}",
        result.stylesheets.diagnostics
    );
    let style = |id| {
        result
            .styles
            .get(
                d.preorder()
                    .find(|&n| {
                        d.node(n)
                            .and_then(|n| n.as_element())
                            .is_some_and(|e| e.attribute("id") == Some(id))
                    })
                    .unwrap(),
            )
            .unwrap()
    };
    assert_eq!(style("a").effects.number(Property::Opacity), 0.5);
    assert!(style("a").effects.transformed());
    assert_eq!(
        style("b").effects.get(Property::Transform),
        style("a").effects.get(Property::Transform)
    );
    assert_eq!(style("c").effects, effects::Effects::default());
    assert!(matches!(style("a").effects.get(Property::MaskImage),Value::Images(v) if v.len()==1));
    let r = crate::layout::Rect {
        x: 0.0,
        y: 0.0,
        width: 100.0,
        height: 100.0,
    };
    assert_eq!(
        style("a").effects.matrix(r, r, 20.0).project(0.0, 0.0),
        Some((30.0, 2.0))
    );
}

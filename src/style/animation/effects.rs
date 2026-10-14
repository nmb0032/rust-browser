use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};

use super::*;
use crate::css::values::{Color, FontWeight, LineHeight, ResolvedColor};
use crate::style::declarations::{Property as P, StyledDeclaration, Value as V};
use crate::style::effects::{self, Filter, Transform};
use crate::{
    dom::NodeId,
    style::{ComputedStyle, Diagnostic, PseudoElement},
};

#[derive(Clone, Debug)]
pub(in crate::style) struct Keyframe {
    pub offset: f64,
    pub declarations: Vec<StyledDeclaration>,
}
#[derive(Clone, Debug)]
pub(in crate::style) struct Keyframes {
    pub name: String,
    pub frames: Vec<Keyframe>,
    pub layer: usize,
    pub rank: usize,
}
#[derive(Clone, Debug)]
pub(in crate::style) struct SampleKeyframe {
    pub offset: f64,
    pub values: BTreeMap<P, V>,
    pub easing: Option<Easing>,
}
pub(in crate::style) struct KeyframeSamples {
    pub frames: Vec<SampleKeyframe>,
    pub underlying: ComputedStyle,
}
pub(in crate::style) fn font_dependency(p: P) -> bool {
    matches!(
        p,
        P::FontSize
            | P::FontFamily
            | P::FontStyle
            | P::FontWeight
            | P::Typography(
                crate::style::typography::Property::FontWidth
                    | crate::style::typography::Property::FontFeatures
                    | crate::style::typography::Property::FontVariations
                    | crate::style::typography::Property::FontOpticalSizing
                    | crate::style::typography::Property::FontKerning
                    | crate::style::typography::Property::FontLigatures
            )
    )
}

pub(in crate::style) fn parse_keyframes(
    rule: &crate::css::AtRule,
    context: crate::style::DeclarationContext,
    base: Option<&url::Url>,
    messages: &mut Vec<String>,
) -> Result<Keyframes, String> {
    let mut report = |message: String| {
        if messages.len() < MAX_KEYFRAMES {
            messages.push(message)
        }
    };
    let tokens =
        crate::css::values::math::tokenize(rule.prelude()).ok_or("invalid keyframes name")?;
    let tokens = components::from_tokens(&tokens).ok_or("invalid keyframes name")?;
    let [c] = tokens.as_slice() else {
        return Err("keyframes requires one name".into());
    };
    let name = name(c)
        .filter(|s| {
            !s.eq_ignore_ascii_case("none") || matches!(c, Component::Token(TokenKind::String(_)))
        })
        .ok_or("invalid keyframes name")?;
    let mut frames: Vec<Keyframe> = Vec::new();
    'rules: for (index, item) in rule
        .block()
        .ok_or("keyframes requires a block")?
        .iter()
        .enumerate()
    {
        if index >= MAX_KEYFRAMES {
            report("keyframe rule budget exceeded; remaining rules omitted".into());
            break;
        }
        let crate::css::BlockItem::Rule(crate::css::Rule::Qualified(r)) = item else {
            report("invalid keyframes rule omitted".into());
            continue;
        };
        let Some(tokens) = crate::css::values::math::tokenize(r.prelude())
            .and_then(|v| components::from_tokens(&v))
        else {
            report("invalid keyframe offset".into());
            continue;
        };
        let mut declarations = Vec::new();
        for (order, d) in r.declarations().enumerate() {
            if d.is_important() {
                continue;
            }
            if let Ok(mut values) =
                crate::style::declarations::parse_with_context(d, order, context)
            {
                values.retain(|v|{
                    if matches!(v.property,crate::style::declarations::DeclarationProperty::Custom(_)){
                        report("keyframe custom-property animation is outside the finite native implementation".into());false
                    }else{true}
                });
                for value in &mut values {
                    if let V::Effect(v) = &mut value.value
                        && let Some(base) = base
                    {
                        effects::resolve_urls(v, base)
                    }
                    if let V::Pending(p) = &mut value.value {
                        std::sync::Arc::make_mut(p).base_url = base.cloned();
                    }
                }
                declarations.extend(values);
                if declarations.len() > MAX_EFFECTS {
                    report("keyframe declaration budget exceeded; block omitted".into());
                    continue 'rules;
                }
            } else {
                report(format!(
                    "invalid/unsupported keyframe declaration ignored: {}",
                    d.property()
                ));
            }
        }
        let Some(parts) = lists(&tokens) else {
            report("invalid keyframe offset list".into());
            continue;
        };
        let mut offsets = Vec::new();
        for part in parts {
            let [c] = part else {
                report("invalid keyframe offset".into());
                continue 'rules;
            };
            let offset = if c.is_ident("from") {
                0.0
            } else if c.is_ident("to") {
                1.0
            } else {
                let Some(Length::Percent(n)) = c.length() else {
                    report("invalid keyframe offset".into());
                    continue 'rules;
                };
                f64::from(n) / 100.0
            };
            if !(0.0..=1.0).contains(&offset) {
                report("keyframe offset outside 0..100%".into());
                continue 'rules;
            }
            if !offsets.contains(&offset) {
                offsets.push(offset);
            }
        }
        for offset in offsets {
            if let Some(f) = frames.iter_mut().find(|f| f.offset == offset) {
                if f.declarations.len() + declarations.len() <= MAX_EFFECTS {
                    f.declarations.extend(declarations.clone())
                } else {
                    report("duplicate keyframe declaration budget exceeded".into());
                }
            } else if frames.len() < MAX_KEYFRAMES {
                frames.push(Keyframe {
                    offset,
                    declarations: declarations.clone(),
                })
            } else {
                report(format!("keyframes exceeds {MAX_KEYFRAMES} offsets"));
            }
        }
    }
    frames.sort_by(|a, b| a.offset.total_cmp(&b.offset));
    Ok(Keyframes {
        name,
        frames,
        layer: 0,
        rank: 0,
    })
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct EffectId(pub u64);
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EventKind {
    Run,
    Start,
    Iteration,
    Finish,
    Cancel,
}
#[derive(Clone, Debug)]
pub struct AnimationEvent {
    pub effect: EffectId,
    pub node: NodeId,
    pub pseudo: Option<PseudoElement>,
    pub name: String,
    pub kind: EventKind,
    pub elapsed_time: f64,
}
type Target = (NodeId, Option<PseudoElement>);
#[derive(Clone, Debug)]
struct Effect {
    id: EffectId,
    config: Config,
    animation: Animation,
    last: Option<ComputedTiming>,
    timeline_time: Option<f64>,
}
#[derive(Clone, Debug)]
struct Transition {
    id: EffectId,
    from: V,
    to: V,
    reversing_start: V,
    shortening: f64,
    animation: Animation,
    easing: Easing,
    last: Option<ComputedTiming>,
}
#[derive(Clone, Debug)]
struct TargetState {
    base: ComputedStyle,
    rendered: BTreeMap<P, V>,
    effects: Vec<Effect>,
    transitions: BTreeMap<P, Transition>,
}

/// Retained, cloneable document owner. Frame clones share document identity,
/// but not mutation. Navigation constructs a fresh owner and clock.
#[derive(Clone, Debug, Default)]
pub struct Engine {
    now: f64,
    next_id: u64,
    targets: HashMap<Target, TargetState>,
    seen: HashSet<(NodeId, Option<PseudoElement>)>,
    events: Vec<AnimationEvent>,
    diagnosed: BTreeSet<String>,
    messages: Vec<Diagnostic>,
    pub(crate) timelines: super::timelines::Snapshot,
    initialized: bool,
}
impl Engine {
    pub fn time(&self) -> f64 {
        self.now
    }
    pub fn initialized(&self) -> bool {
        self.initialized
    }
    pub(crate) fn diagnostics(&self) -> &[Diagnostic] {
        &self.messages
    }
    pub fn set_time(&mut self, now: f64) -> Result<(), &'static str> {
        if !valid_time(now) || now < 0.0 {
            return Err("document animation time must be finite and nonnegative");
        }
        self.now = now;
        Ok(())
    }
    pub fn effect_ids(&self, node: NodeId, pseudo: Option<PseudoElement>) -> Vec<EffectId> {
        self.targets
            .get(&(node, pseudo))
            .map_or_else(Vec::new, |t| {
                t.effects
                    .iter()
                    .map(|e| e.id)
                    .chain(t.transitions.values().map(|e| e.id))
                    .collect()
            })
    }
    pub fn animation(&self, id: EffectId) -> Option<&Animation> {
        self.targets.values().find_map(|t| {
            t.effects
                .iter()
                .find(|e| e.id == id)
                .map(|e| &e.animation)
                .or_else(|| {
                    t.transitions
                        .values()
                        .find(|e| e.id == id)
                        .map(|e| &e.animation)
                })
        })
    }
    pub fn animation_mut(&mut self, id: EffectId) -> Option<&mut Animation> {
        self.targets.values_mut().find_map(|t| {
            t.effects
                .iter_mut()
                .find(|e| e.id == id)
                .map(|e| &mut e.animation)
                .or_else(|| {
                    t.transitions
                        .values_mut()
                        .find(|e| e.id == id)
                        .map(|e| &mut e.animation)
                })
        })
    }
    pub fn take_events(&mut self) -> Vec<AnimationEvent> {
        std::mem::take(&mut self.events)
    }
    pub fn needs_tick(&self) -> bool {
        self.targets.values().any(|t| {
            t.effects.iter().any(|e| {
                e.config.timeline == Timeline::Auto
                    && e.animation.play_state(self.now) == PlayState::Running
                    && e.animation.playback_rate() != 0.0
            }) || t.transitions.values().any(|e| {
                e.animation.play_state(self.now) == PlayState::Running
                    && e.animation.playback_rate() != 0.0
            })
        })
    }
    pub fn has_scroll_effects(&self) -> bool {
        self.targets.values().any(|t| {
            t.effects
                .iter()
                .any(|e| !matches!(e.config.timeline, Timeline::Auto | Timeline::None))
        })
    }
    pub(crate) fn begin(&mut self) {
        self.seen.clear();
    }
    /// No retained targets: a cascade without active motion samples nothing.
    pub(crate) fn has_targets(&self) -> bool {
        !self.targets.is_empty()
    }
    /// Some running, pending or filling effect may override a base value.
    pub(crate) fn has_effects(&self) -> bool {
        self.targets
            .values()
            .any(|t| !t.effects.is_empty() || !t.transitions.is_empty())
    }
    pub(in crate::style) fn rendered(&mut self, target: Target, style: &ComputedStyle) {
        if let Some(state) = self.targets.get_mut(&target) {
            for (&p, v) in &mut state.rendered {
                *v = super::super::cascade::animation_value(style, p);
            }
        }
    }
    pub(crate) fn end(&mut self) {
        self.initialized = true;
        let absent: Vec<_> = self
            .targets
            .keys()
            .filter(|k| !self.seen.contains(k))
            .copied()
            .collect();
        for target in absent {
            if let Some(t) = self.targets.remove(&target) {
                for e in t.effects {
                    self.event(
                        target,
                        e.id,
                        &e.config.name,
                        EventKind::Cancel,
                        active_elapsed(&e.animation, self.now),
                    );
                }
                for (p, e) in t.transitions {
                    self.event(
                        target,
                        e.id,
                        p.name(),
                        EventKind::Cancel,
                        active_elapsed(&e.animation, self.now),
                    );
                }
            }
        }
    }
    fn id(&mut self) -> EffectId {
        self.next_id = self.next_id.saturating_add(1);
        EffectId(self.next_id)
    }
    fn count(&self) -> usize {
        self.targets
            .values()
            .map(|t| t.effects.len() + t.transitions.len())
            .sum()
    }
    fn event(&mut self, target: Target, id: EffectId, name: &str, kind: EventKind, elapsed: f64) {
        if self.events.len() < MAX_KEYFRAMES && elapsed.is_finite() {
            self.events.push(AnimationEvent {
                effect: id,
                node: target.0,
                pseudo: target.1,
                name: name.into(),
                kind,
                elapsed_time: elapsed,
            });
        }
    }
    pub(crate) fn diagnose(
        &mut self,
        target: Target,
        message: String,
        diagnostics: &mut Vec<Diagnostic>,
    ) {
        if self.diagnosed.len() < MAX_EFFECTS && self.diagnosed.insert(message.clone()) {
            let diagnostic = Diagnostic {
                node_id: Some(target.0),
                message,
            };
            diagnostics.push(diagnostic.clone());
            self.messages.push(diagnostic);
        }
    }
    fn lifecycle(
        &mut self,
        target: Target,
        id: EffectId,
        name: &str,
        old: Option<ComputedTiming>,
        timing: ComputedTiming,
    ) {
        let entered_forward =
            timing.phase != Phase::Before && old.is_none_or(|t| t.phase == Phase::Before);
        let entered_backward =
            timing.phase != Phase::After && old.is_some_and(|t| t.phase == Phase::After);
        if entered_forward || entered_backward {
            self.event(
                target,
                id,
                name,
                EventKind::Start,
                if entered_backward {
                    timing.active_duration
                } else {
                    (-timing.delay).clamp(0.0, timing.active_duration)
                },
            );
        }
        if timing.phase == Phase::Active
            && old.is_some_and(|t| t.phase == Phase::Active && t.iteration != timing.iteration)
        {
            let reversed = old.is_some_and(|t| t.iteration > timing.iteration);
            let elapsed = (timing.iteration + f64::from(reversed)) * timing.duration;
            self.event(
                target,
                id,
                name,
                EventKind::Iteration,
                elapsed.min(timing.active_duration),
            );
        }
        if timing.phase == Phase::After && old.is_none_or(|t| t.phase != Phase::After) {
            self.event(target, id, name, EventKind::Finish, timing.active_duration);
        } else if timing.phase == Phase::Before && old.is_some_and(|t| t.phase != Phase::Before) {
            self.event(target, id, name, EventKind::Finish, 0.0);
        }
    }

    pub(in crate::style) fn sample(
        &mut self,
        target: Target,
        base: &ComputedStyle,
        keyframes: &BTreeMap<String, Keyframes>,
        mut endpoints: impl FnMut(&Keyframes, &BTreeMap<P, V>) -> KeyframeSamples,
        diagnostics: &mut Vec<Diagnostic>,
    ) -> Vec<(P, V, bool)> {
        self.seen.insert(target);
        let configs = base.motion.configs();
        let had = self.targets.contains_key(&target);
        let enabled = base.display != crate::style::Display::None;
        let has_transitions = matches!(base.motion.get(Property::TransitionDuration),Value::Times(v) if v.iter().any(|t|t.is_some_and(|t|t>0.0)));
        if !had && configs.iter().all(|c| !c.enabled) && !has_transitions {
            return Vec::new();
        }
        if !had && self.targets.len() >= MAX_EFFECTS {
            self.diagnose(
                target,
                "animation target limit exceeded; effect omitted".into(),
                diagnostics,
            );
            return Vec::new();
        }
        let mut state = self.targets.remove(&target).unwrap_or_else(|| TargetState {
            base: base.clone(),
            rendered: BTreeMap::new(),
            effects: Vec::new(),
            transitions: BTreeMap::new(),
        });
        if !enabled {
            for e in state.effects.drain(..) {
                self.event(
                    target,
                    e.id,
                    &e.config.name,
                    EventKind::Cancel,
                    active_elapsed(&e.animation, self.now),
                );
            }
            for (p, e) in std::mem::take(&mut state.transitions) {
                self.event(
                    target,
                    e.id,
                    p.name(),
                    EventKind::Cancel,
                    active_elapsed(&e.animation, self.now),
                );
            }
            state.base = base.clone();
            state.rendered.clear();
            self.targets.insert(target, state);
            return Vec::new();
        }
        // Match old CSS animations right-to-left by name, preserving created
        // time across timing/stylesheet/resource changes and duplicate names.
        let mut previous = std::mem::take(&mut state.effects);
        let mut next = Vec::new();
        let available = MAX_EFFECTS.saturating_sub(self.count() + state.transitions.len());
        for c in configs.into_iter().rev().filter(|c| c.enabled) {
            if !keyframes.contains_key(&c.name) {
                continue;
            }
            if next.len() >= available {
                self.diagnose(
                    target,
                    "native animation effect limit exceeded; excess effects omitted".into(),
                    diagnostics,
                );
                continue;
            }
            let mut e = if let Some(i) = previous.iter().rposition(|e| e.config.name == c.name) {
                previous.remove(i)
            } else {
                let id = self.id();
                self.event(target, id, &c.name, EventKind::Run, 0.0);
                let mut animation = Animation::new(c.timing.clone(), self.now)
                    .expect("validated CSS animation timing");
                if c.paused {
                    animation.pause(self.now).expect("validated document clock");
                }
                Effect {
                    id,
                    config: c.clone(),
                    animation,
                    last: None,
                    timeline_time: None,
                }
            };
            if e.config.timing != c.timing {
                e.animation
                    .update_timing(c.timing.clone())
                    .expect("validated CSS animation timing");
            }
            if c.paused != e.config.paused
                || c.paused && e.animation.play_state(self.now) == PlayState::Running
            {
                if c.paused {
                    if c.timeline != Timeline::Auto
                        && let Some(time) = e.timeline_time
                    {
                        e.animation
                            .seek(time, self.now)
                            .expect("bounded timeline time");
                    }
                    e.animation
                        .pause(self.now)
                        .expect("validated document clock");
                } else {
                    if let Err(message) = e.animation.play(self.now) {
                        self.diagnose(target, message.into(), diagnostics);
                    }
                }
            }
            e.config = c;
            next.push(e);
        }
        for e in previous {
            self.event(
                target,
                e.id,
                &e.config.name,
                EventKind::Cancel,
                active_elapsed(&e.animation, self.now),
            );
        }
        next.reverse();
        state.effects = next;
        let mut sampled: BTreeMap<P, V> = BTreeMap::new();
        for e in &mut state.effects {
            if e.animation.play_state(self.now) == PlayState::Idle {
                if let Some(last) = e.last.take() {
                    self.event(
                        target,
                        e.id,
                        &e.config.name,
                        EventKind::Cancel,
                        last.active_time,
                    );
                }
                continue;
            }
            let time = match &e.config.timeline {
                Timeline::Auto => e.animation.current_time(self.now),
                Timeline::None => None,
                _ => self
                    .timelines
                    .progress(target.0, &e.config, base)
                    .and_then(|p| {
                        let time = p * e.animation.timing().active_duration();
                        if !valid_time(time) {
                            return None;
                        }
                        if e.animation.play_state(self.now) == PlayState::Paused
                            || e.animation.playback_rate() == 0.0
                        {
                            e.animation.current_time(self.now)
                        } else {
                            e.animation.seek(time, self.now).ok()?;
                            Some(time)
                        }
                    }),
            };
            let Some(time) = time else { continue };
            e.timeline_time = Some(time);
            let timing = if e.config.timeline == Timeline::Auto {
                e.animation
                    .computed_timing(self.now)
                    .expect("active animation clock")
            } else {
                e.animation.timing().sample(time)
            };
            if e.config.timeline == Timeline::Auto {
                self.lifecycle(target, e.id, &e.config.name, e.last, timing);
            }
            e.last = Some(timing);
            let Some(progress) = timing.progress else {
                continue;
            };
            let mut resolved = endpoints(&keyframes[&e.config.name], &sampled);
            let mut props: Vec<_> = resolved
                .frames
                .iter()
                .flat_map(|f| f.values.keys().copied())
                .collect::<BTreeSet<_>>()
                .into_iter()
                .collect();
            props.sort_by_key(|p| (!font_dependency(*p), *p));
            let mut primed = !props.iter().any(|p| font_dependency(*p));
            for p in props {
                if !animatable(p) {
                    self.diagnose(
                        target,
                        format!("{} is not animatable in the finite native engine", p.name()),
                        diagnostics,
                    );
                    continue;
                }
                if !font_dependency(p) && !primed {
                    resolved = endpoints(&keyframes[&e.config.name], &sampled);
                    primed = true;
                }
                let underlying = sampled.get(&p).cloned().unwrap_or_else(|| {
                    super::super::cascade::animation_value(
                        if font_dependency(p) {
                            base
                        } else {
                            &resolved.underlying
                        },
                        p,
                    )
                });
                let mut points: Vec<_> = resolved
                    .frames
                    .iter()
                    .filter_map(|f| {
                        f.values
                            .get(&p)
                            .map(|v| (f.offset, v.clone(), f.easing.clone()))
                    })
                    .collect();
                if points.first().is_none_or(|f| f.0 != 0.0) {
                    points.insert(0, (0.0, underlying.clone(), None))
                }
                if points.last().is_none_or(|f| f.0 != 1.0) {
                    points.push((1.0, underlying.clone(), None))
                }
                let idx = points
                    .windows(2)
                    .position(|v| progress < v[1].0)
                    .unwrap_or(points.len() - 2);
                let (a, b) = (&points[idx], &points[idx + 1]);
                let fraction = ((progress - a.0) / (b.0 - a.0)).clamp(0.0, 1.0);
                let eased =
                    a.2.as_ref()
                        .unwrap_or(&e.config.easing)
                        .sample(fraction, timing.before);
                let value = interpolate(&a.1, &b.1, eased);
                if value.is_none() && !discrete(p, &a.1, &b.1) {
                    self.diagnose(
                        target,
                        format!(
                            "{} uses discrete half-progress fallback (unsupported value pair)",
                            p.name()
                        ),
                        diagnostics,
                    );
                }
                let value = value.unwrap_or_else(|| {
                    if eased < 0.5 {
                        a.1.clone()
                    } else {
                        b.1.clone()
                    }
                });
                let value = if e.config.composition == Composition::Accumulate {
                    self.diagnose(
                        target,
                        format!(
                            "{} accumulate composition unsupported; replace fallback",
                            p.name()
                        ),
                        diagnostics,
                    );
                    value
                } else if e.config.composition == Composition::Add {
                    if let Some(v) = add(&underlying, &value) {
                        v
                    } else {
                        self.diagnose(
                            target,
                            format!(
                                "{} additive composition unsupported; replace fallback",
                                p.name()
                            ),
                            diagnostics,
                        );
                        value
                    }
                } else {
                    value
                };
                sampled.insert(p, value);
            }
        }
        let mut output: Vec<_> = sampled
            .iter()
            .map(|(&p, v)| (p, v.clone(), false))
            .collect();
        let Value::Names(properties) = base.motion.get(Property::TransitionProperty) else {
            unreachable!()
        };
        let mut candidates: BTreeSet<P> = state.transitions.keys().copied().collect();
        if properties.iter().any(|s| s.eq_ignore_ascii_case("all")) {
            for meta in crate::style::metadata::PROPERTIES {
                if let Some(p) = crate::style::declarations::properties(meta.name) {
                    candidates.extend(
                        p.into_iter()
                            .map(|p| p.physical(base))
                            .filter(|p| animatable(*p)),
                    );
                }
            }
        } else {
            for name in &properties {
                if let Some(props) = crate::style::declarations::properties(name) {
                    candidates.extend(
                        props
                            .into_iter()
                            .map(|p| p.physical(base))
                            .filter(|p| animatable(*p)),
                    );
                }
            }
        }
        for p in candidates {
            let matched = properties.iter().rposition(|s| {
                s.eq_ignore_ascii_case("all")
                    || crate::style::declarations::properties(s)
                        .is_some_and(|v| v.iter().any(|v| v.physical(base) == p))
            });
            let time = |prop: Property, i: usize| match base.motion.get(prop) {
                Value::Times(v) => v[i % v.len()].unwrap_or(0.0),
                _ => 0.0,
            };
            let changed = !equal(
                &super::super::cascade::animation_value(&state.base, p),
                &super::super::cascade::animation_value(base, p),
            );
            if let Some(i) = matched {
                if had && state.base.display != crate::style::Display::None && changed {
                    let from = state
                        .transitions
                        .get(&p)
                        .map(|t| transition_value(t, self.now))
                        .or_else(|| state.rendered.get(&p).cloned())
                        .unwrap_or_else(|| super::super::cascade::animation_value(&state.base, p));
                    let to = super::super::cascade::animation_value(base, p);
                    let duration = time(Property::TransitionDuration, i);
                    let delay = time(Property::TransitionDelay, i);
                    let old = state.transitions.remove(&p);
                    let shortening = old
                        .as_ref()
                        .filter(|o| equal(&o.reversing_start, &to))
                        .map_or(1.0, |o| {
                            let progress = o
                                .animation
                                .computed_timing(self.now)
                                .and_then(|t| t.progress)
                                .unwrap_or(0.0);
                            (o.easing.sample(progress, false) * o.shortening + 1.0 - o.shortening)
                                .abs()
                                .clamp(0.0, 1.0)
                        });
                    if let Some(o) = &old {
                        self.event(
                            target,
                            o.id,
                            p.name(),
                            EventKind::Cancel,
                            active_elapsed(&o.animation, self.now),
                        );
                    }
                    let compatible = interpolate(&from, &to, 0.5).is_some();
                    if duration + delay > 0.0
                        && !equal(&from, &to)
                        && !compatible
                        && !discrete(p, &from, &to)
                    {
                        self.diagnose(
                            target,
                            format!(
                                "{} transition value pair unsupported; no discrete transition",
                                p.name()
                            ),
                            diagnostics,
                        );
                    }
                    if duration + delay > 0.0
                        && !equal(&from, &to)
                        && compatible
                        && self.count() + state.effects.len() + state.transitions.len()
                            < MAX_EFFECTS
                    {
                        let id = self.id();
                        self.event(target, id, p.name(), EventKind::Run, 0.0);
                        let reversing_start = old
                            .as_ref()
                            .filter(|o| equal(&o.reversing_start, &to))
                            .map_or(from.clone(), |o| o.to.clone());
                        let easing = match base.motion.get(Property::TransitionEasing) {
                            Value::Easings(v) => v[i % v.len()].clone(),
                            _ => Easing::default(),
                        };
                        let animation = Animation::new(
                            Timing {
                                duration: duration * shortening,
                                delay: if delay < 0.0 {
                                    delay * shortening
                                } else {
                                    delay
                                },
                                fill: Fill::Both,
                                ..Timing::default()
                            },
                            self.now,
                        )
                        .expect("validated CSS transition timing");
                        state.transitions.insert(
                            p,
                            Transition {
                                id,
                                from,
                                to,
                                reversing_start,
                                shortening,
                                animation,
                                easing,
                                last: None,
                            },
                        );
                    }
                }
            } else if let Some(o) = state.transitions.remove(&p) {
                self.event(
                    target,
                    o.id,
                    p.name(),
                    EventKind::Cancel,
                    active_elapsed(&o.animation, self.now),
                );
            }
            if state
                .transitions
                .get(&p)
                .is_some_and(|t| t.animation.play_state(self.now) == PlayState::Idle)
            {
                let t = state
                    .transitions
                    .remove(&p)
                    .expect("existing cancelled transition");
                self.event(
                    target,
                    t.id,
                    p.name(),
                    EventKind::Cancel,
                    t.last.map_or(0.0, |t| t.active_time),
                );
            }
            if let Some(t) = state.transitions.get_mut(&p)
                && let Some(timing) = t.animation.computed_timing(self.now)
            {
                self.lifecycle(target, t.id, p.name(), t.last, timing);
                t.last = Some(timing);
                if timing.phase != Phase::After {
                    let value = transition_value(t, self.now);
                    sampled.insert(p, value.clone());
                    output.push((p, value, true));
                } else {
                    state.transitions.remove(&p);
                }
            }
        }
        state.base = base.clone();
        state.rendered = sampled;
        self.targets.insert(target, state);
        output
    }
}
fn transition_value(t: &Transition, now: f64) -> V {
    let timing = t
        .animation
        .computed_timing(now)
        .unwrap_or_else(|| t.animation.timing.sample(0.0));
    let p = t
        .easing
        .sample(timing.progress.unwrap_or(0.0), timing.before);
    interpolate(&t.from, &t.to, p).unwrap_or_else(|| {
        if p < 0.5 {
            t.from.clone()
        } else {
            t.to.clone()
        }
    })
}
fn active_elapsed(animation: &Animation, now: f64) -> f64 {
    animation
        .computed_timing(now)
        .map_or(0.0, |t| t.active_time)
}
fn equal(a: &V, b: &V) -> bool {
    format!("{a:?}") == format!("{b:?}")
}
fn animatable(p: P) -> bool {
    if matches!(
        p,
        P::Animation(_)
            | P::Display
            | P::Content
            | P::Direction
            | P::UnicodeBidi
            | P::ContainerType
            | P::ContainerName
    ) {
        return false;
    }
    crate::style::metadata::property(p.name())
        .is_some_and(|m| m.animation_type.is_some_and(|a| a != "not animatable"))
}
fn discrete(p: P, a: &V, b: &V) -> bool {
    if crate::style::metadata::property(p.name())
        .is_some_and(|m| m.animation_type == Some("discrete"))
    {
        return true;
    }
    let keyword = |v: &V| {
        matches!(
            v,
            V::Auto
                | V::None
                | V::ContentBasis
                | V::Length(Length::MinContent | Length::MaxContent | Length::FitContent(_))
        )
    };
    keyword(a)
        || keyword(b)
        || matches!(
            (a, b),
            (
                V::FontStyle(crate::css::values::FontStyle::Italic),
                V::FontStyle(_)
            ) | (
                V::FontStyle(_),
                V::FontStyle(crate::css::values::FontStyle::Italic)
            )
        )
}
fn lerp(a: f64, b: f64, t: f64) -> f64 {
    a + (b - a) * t
}
fn length(a: &Length, b: &Length, t: f64) -> Option<Length> {
    match (a, b) {
        (Length::Px(a), Length::Px(b)) => {
            Some(Length::Px(lerp(f64::from(*a), f64::from(*b), t) as f32))
        }
        (Length::Percent(a), Length::Percent(b)) => {
            Some(Length::Percent(lerp(f64::from(*a), f64::from(*b), t) as f32))
        }
        _ => crate::css::values::MathExpression::interpolate_lengths(a, b, t).map(Length::Math),
    }
}
pub(crate) fn interpolate(a: &V, b: &V, t: f64) -> Option<V> {
    if t == 0.0 || equal(a, b) {
        return Some(a.clone());
    }
    if t == 1.0 {
        return Some(b.clone());
    }
    Some(match (a, b) {
        (V::Length(a), V::Length(b)) => V::Length(length(a, b, t)?),
        (V::Number(a), V::Number(b)) => V::Number(lerp(f64::from(*a), f64::from(*b), t) as f32),
        (V::Integer(a), V::Integer(b)) => {
            V::Integer(lerp(f64::from(*a), f64::from(*b), t).round() as i32)
        }
        (V::Weight(FontWeight(a)), V::Weight(FontWeight(b))) => {
            V::Weight(FontWeight(lerp(f64::from(*a), f64::from(*b), t) as f32))
        }
        (V::Color(a), V::Color(b)) => {
            // CSS Color 4 defaults to premultiplied Oklab, not channel-wise RGB.
            let a = a.resolve(ResolvedColor::BLACK)?;
            let b = b.resolve(ResolvedColor::BLACK)?;
            let color = Color::Mix(std::sync::Arc::new(crate::css::values::color_mix(a, b, t)));
            V::Color(Color::Absolute(color.resolve(ResolvedColor::BLACK)?))
        }
        (V::LineHeight(LineHeight::Number(a)), V::LineHeight(LineHeight::Number(b))) => {
            V::LineHeight(LineHeight::Number(
                lerp(f64::from(*a), f64::from(*b), t) as f32
            ))
        }
        (V::LineHeight(LineHeight::Length(a)), V::LineHeight(LineHeight::Length(b))) => {
            V::LineHeight(LineHeight::Length(length(a, b, t)?))
        }
        (V::Effect(a), V::Effect(b)) => V::Effect(effect(a, b, t)?),
        (V::Typography(a), V::Typography(b)) => V::Typography(typography(a, b, t)?),
        _ if equal(a, b) => a.clone(),
        _ => return None,
    })
}
fn add(a: &V, b: &V) -> Option<V> {
    Some(match (a, b) {
        (V::Number(a), V::Number(b)) => V::Number(a + b),
        (V::Length(Length::Px(a)), V::Length(Length::Px(b))) => V::Length(Length::Px(a + b)),
        (V::Length(Length::Percent(a)), V::Length(Length::Percent(b))) => {
            V::Length(Length::Percent(a + b))
        }
        (V::Effect(effects::Value::Number(a)), V::Effect(effects::Value::Number(b))) => {
            V::Effect(effects::Value::Number(a + b))
        }
        (V::Effect(effects::Value::Transforms(a)), V::Effect(effects::Value::Transforms(b))) => {
            let mut v = a.clone();
            v.extend(b.clone());
            if v.len() > 64 {
                return None;
            }
            V::Effect(effects::Value::Transforms(v))
        }
        _ => return None,
    })
}
fn typography(
    a: &crate::style::typography::Value,
    b: &crate::style::typography::Value,
    t: f64,
) -> Option<crate::style::typography::Value> {
    use crate::style::typography::Value as T;
    Some(match (a, b) {
        (T::Width(a), T::Width(b)) => T::Width(lerp(f64::from(*a), f64::from(*b), t) as f32),
        (T::Length(a), T::Length(b)) => T::Length(length(a, b, t)?),
        _ => return None,
    })
}
fn identity(t: &Transform) -> Transform {
    match t {
        Transform::Translate(_) => {
            Transform::Translate([Length::Px(0.0), Length::Px(0.0), Length::Px(0.0)])
        }
        Transform::Scale(_) => Transform::Scale([1.0; 3]),
        Transform::Rotate(axis, _) => Transform::Rotate(*axis, 0.0),
        Transform::Skew(_, _) => Transform::Skew(0.0, 0.0),
        Transform::Matrix(_) => Transform::Matrix(crate::layout::Mat4::IDENTITY),
        Transform::Perspective(l) => Transform::Perspective(l.clone()),
    }
}
fn transform(a: &Transform, b: &Transform, t: f64) -> Option<Transform> {
    Some(match (a, b) {
        (Transform::Translate(a), Transform::Translate(b)) => Transform::Translate([
            length(&a[0], &b[0], t)?,
            length(&a[1], &b[1], t)?,
            length(&a[2], &b[2], t)?,
        ]),
        (Transform::Scale(a), Transform::Scale(b)) => {
            Transform::Scale(std::array::from_fn(|i| lerp(a[i], b[i], t)))
        }
        (Transform::Rotate(a, x), Transform::Rotate(b, y)) if a == b => {
            Transform::Rotate(*a, lerp(*x, *y, t))
        }
        (Transform::Skew(a, b), Transform::Skew(c, d)) => {
            Transform::Skew(lerp(*a, *c, t), lerp(*b, *d, t))
        }
        _ => return None, // Singular/mismatched matrices are explicit discrete fallback, not elementwise interpolation.
    })
}
fn filter_identity(f: &Filter) -> Filter {
    match f {
        Filter::Blur(_) => Filter::Blur(Length::Px(0.0)),
        Filter::Brightness(_) => Filter::Brightness(1.0),
        Filter::Contrast(_) => Filter::Contrast(1.0),
        Filter::Grayscale(_) => Filter::Grayscale(0.0),
        Filter::Sepia(_) => Filter::Sepia(0.0),
        Filter::Invert(_) => Filter::Invert(0.0),
        Filter::Opacity(_) => Filter::Opacity(1.0),
        Filter::Saturate(_) => Filter::Saturate(1.0),
        Filter::HueRotate(_) => Filter::HueRotate(0.0),
        Filter::DropShadow { .. } => f.clone(),
    }
}
fn filter(a: &Filter, b: &Filter, t: f64) -> Option<Filter> {
    macro_rules! num {
        ($v:ident,$a:expr,$b:expr) => {
            Filter::$v(lerp(f64::from(*$a), f64::from(*$b), t) as f32)
        };
    }
    Some(match (a, b) {
        (Filter::Blur(a), Filter::Blur(b)) => Filter::Blur(length(a, b, t)?),
        (Filter::Brightness(a), Filter::Brightness(b)) => num!(Brightness, a, b),
        (Filter::Contrast(a), Filter::Contrast(b)) => num!(Contrast, a, b),
        (Filter::Grayscale(a), Filter::Grayscale(b)) => num!(Grayscale, a, b),
        (Filter::Sepia(a), Filter::Sepia(b)) => num!(Sepia, a, b),
        (Filter::Invert(a), Filter::Invert(b)) => num!(Invert, a, b),
        (Filter::Opacity(a), Filter::Opacity(b)) => num!(Opacity, a, b),
        (Filter::Saturate(a), Filter::Saturate(b)) => num!(Saturate, a, b),
        (Filter::HueRotate(a), Filter::HueRotate(b)) => Filter::HueRotate(lerp(*a, *b, t)),
        _ => return None,
    })
}
fn effect(a: &effects::Value, b: &effects::Value, t: f64) -> Option<effects::Value> {
    use effects::Value as E;
    Some(match (a, b) {
        (E::Number(a), E::Number(b)) => E::Number(lerp(f64::from(*a), f64::from(*b), t) as f32),
        (E::Length(a), E::Length(b)) => E::Length(length(a, b, t)?),
        (E::Transforms(a), E::Transforms(b)) => {
            let n = a.len().max(b.len());
            let mut out = Vec::new();
            for i in 0..n {
                let x = a.get(i).cloned().unwrap_or_else(|| identity(&b[i]));
                let y = b.get(i).cloned().unwrap_or_else(|| identity(&a[i]));
                out.push(transform(&x, &y, t)?);
            }
            E::Transforms(out)
        }
        (E::Filters(a), E::Filters(b)) => {
            let n = a.len().max(b.len());
            let mut out = Vec::new();
            for i in 0..n {
                let x = a.get(i).cloned().unwrap_or_else(|| filter_identity(&b[i]));
                let y = b.get(i).cloned().unwrap_or_else(|| filter_identity(&a[i]));
                out.push(filter(&x, &y, t)?);
            }
            E::Filters(out)
        }
        _ if a == b => a.clone(),
        _ => return None,
    })
}

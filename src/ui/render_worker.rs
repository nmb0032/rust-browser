//! Off-UI-thread style/layout/paint. The UI submits at most one job at a time
//! and keeps presenting, scrolling and hit-testing its last accepted
//! frame/layout/display-list triple until the result arrives.

use std::{
    panic::{self, AssertUnwindSafe},
    sync::{Arc, mpsc},
    thread,
    time::{Duration, Instant},
};

use winit::dpi::PhysicalSize;

use crate::{
    css::{
        media::{ColorScheme, MediaEnvironment},
        selectors::InteractionState,
    },
    dom::NodeId,
    engine::Frame,
    layout::{self, LayoutTree},
    paint::{self, DisplayItem},
    text::FontContext,
};

/// Everything the UI owns that affects a render, captured at submission.
#[derive(Clone, Debug)]
pub struct RenderInputs {
    pub size: PhysicalSize<u32>,
    pub scale_factor: f64,
    pub zoom: f64,
    pub color_scheme: Option<ColorScheme>,
    pub animation_time: f64,
    /// Requested hover/active/focus; `target` is owned by the frame's URL.
    pub interaction: InteractionState,
    pub outlines: bool,
    pub scroll: f32,
    pub refresh_target: bool,
    pub fragment_scroll: bool,
    pub restore_scroll: Option<f32>,
}

pub struct RenderJob {
    pub generation: u64,
    pub frame: Arc<Frame>,
    pub inputs: RenderInputs,
}

pub struct Rendered {
    pub frame: Arc<Frame>,
    pub tree: LayoutTree,
    pub list: Vec<DisplayItem>,
    pub scroll: f32,
    /// Web fonts or effect images still to fetch for this frame.
    pub pending_resources: bool,
}

pub struct RenderOutput {
    pub generation: u64,
    pub elapsed: Duration,
    pub result: Result<Rendered, String>,
}

/// Apply `inputs` to `frame` and run the UI render pipeline. Pure apart from
/// diagnostics, so the worker, the synchronous fallback and tests share it.
pub fn render_job(
    mut frame: Arc<Frame>,
    inputs: &RenderInputs,
    font: &FontContext,
) -> Result<Rendered, String> {
    let scale = crate::display::ViewScale::new(inputs.scale_factor, inputs.zoom)
        .map_err(|error| format!("Display scale: {error}"))?;
    let next = Arc::make_mut(&mut frame);
    next.set_viewport(
        inputs.size.width as f32 / inputs.scale_factor as f32,
        inputs.size.height as f32 / inputs.scale_factor as f32,
        scale,
    )
    .map_err(|error| format!("Display scale: {error}"))?;
    if let Err(error) = next.set_animation_time(inputs.animation_time) {
        eprintln!("Animation clock: {error}");
    }
    next.set_media_environment(MediaEnvironment {
        color_scheme: inputs.color_scheme,
        ..next.media_environment()
    });
    if inputs.refresh_target {
        next.refresh_target();
    }
    let mut state = inputs.interaction.clone();
    state.target = next.interaction_state().target;
    let valid = |id: Option<NodeId>| {
        id.filter(|&id| {
            next.document
                .node(id)
                .is_some_and(|node| node.as_element().is_some())
        })
    };
    state.hovered = valid(state.hovered);
    state.active = valid(state.active);
    state.focused = valid(state.focused);
    state.focus_visible &= state.focused.is_some();
    next.set_interaction_state(state);

    let (mut tree, mut list, mut scroll) = super::window::render_for_window_size(
        next,
        inputs.size,
        inputs.scale_factor,
        font,
        inputs.outlines,
        inputs.restore_scroll.unwrap_or(inputs.scroll),
    );
    if inputs.restore_scroll.is_none() && inputs.fragment_scroll {
        match next.scroll_to_fragment(&mut tree, layout::ScrollBehavior::Auto) {
            Ok(true) if next.animations().has_scroll_effects() => {
                let scroll_now = tree.viewport_scroll().y;
                (tree, list, scroll) = super::window::render_for_window_size(
                    next,
                    inputs.size,
                    inputs.scale_factor,
                    font,
                    inputs.outlines,
                    scroll_now,
                );
            }
            Ok(true) => {
                let logical = inputs.size.to_logical::<f32>(scale.effective());
                scroll = tree.viewport_scroll().y;
                list = paint::build_display_list(
                    &tree,
                    logical.width,
                    logical.height,
                    inputs.outlines,
                );
            }
            Ok(false) => {}
            Err(error) => eprintln!("Fragment scroll: {error}"),
        }
    }
    let pending_resources = next.has_pending_web_fonts() || next.has_pending_effect_images();
    Ok(Rendered {
        frame,
        tree,
        list,
        scroll,
        pending_resources,
    })
}

pub struct RenderWorker {
    jobs: Option<mpsc::Sender<RenderJob>>,
    results: mpsc::Receiver<RenderOutput>,
}

impl RenderWorker {
    /// `wake` runs on the worker thread after each result is queued.
    pub fn spawn(font: FontContext, wake: impl Fn() + Send + 'static) -> std::io::Result<Self> {
        let (jobs, job_rx) = mpsc::channel::<RenderJob>();
        let (result_tx, results) = mpsc::channel();
        thread::Builder::new()
            .name("render".into())
            .spawn(move || {
                for job in job_rx {
                    let started = Instant::now();
                    let result = panic::catch_unwind(AssertUnwindSafe(|| {
                        render_job(job.frame, &job.inputs, &font)
                    }))
                    .unwrap_or_else(|panic| {
                        Err(panic
                            .downcast_ref::<&str>()
                            .map(|s| s.to_string())
                            .or_else(|| panic.downcast_ref::<String>().cloned())
                            .unwrap_or_else(|| "render panicked".into()))
                    });
                    let output = RenderOutput {
                        generation: job.generation,
                        elapsed: started.elapsed(),
                        result,
                    };
                    if result_tx.send(output).is_err() {
                        break;
                    }
                    wake();
                }
            })?;
        Ok(Self {
            jobs: Some(jobs),
            results,
        })
    }

    /// Returns the job back if the worker has exited.
    pub fn submit(&self, job: RenderJob) -> Result<(), Box<RenderJob>> {
        match &self.jobs {
            Some(jobs) => jobs.send(job).map_err(|error| Box::new(error.0)),
            None => Err(Box::new(job)),
        }
    }

    pub fn try_recv(&self) -> Option<RenderOutput> {
        self.results.try_recv().ok()
    }
}

impl Drop for RenderWorker {
    /// Closing the queue ends the worker after its current job; exit never
    /// waits on an in-flight render.
    fn drop(&mut self) {
        self.jobs.take();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::html::parser;
    use std::sync::atomic::{AtomicUsize, Ordering};

    fn assert_send<T: Send>() {}

    fn inputs() -> RenderInputs {
        RenderInputs {
            size: PhysicalSize::new(800, 600),
            scale_factor: 1.0,
            zoom: 1.0,
            color_scheme: None,
            animation_time: 0.0,
            interaction: InteractionState::default(),
            outlines: false,
            scroll: 0.0,
            refresh_target: false,
            fragment_scroll: false,
            restore_scroll: None,
        }
    }

    fn frame(url: &str, html: &str) -> Arc<Frame> {
        Arc::new(Frame::new(
            url::Url::parse(url).unwrap(),
            parser::parse(html).unwrap(),
        ))
    }

    fn element(frame: &Frame, id: &str) -> NodeId {
        frame
            .document
            .preorder()
            .find(|&node| {
                frame
                    .document
                    .node(node)
                    .and_then(|n| n.as_element())
                    .is_some_and(|e| e.attribute("id") == Some(id))
            })
            .unwrap()
    }

    #[test]
    fn render_job_types_cross_threads() {
        assert_send::<RenderJob>();
        assert_send::<RenderOutput>();
        assert_send::<FontContext>();
    }

    #[test]
    fn render_job_applies_requested_interaction_and_drops_non_element_targets() {
        let base = frame(
            "https://example.test/",
            "<style>#a:hover {color:red} #a:focus {color:blue}</style><p id='a'>hi</p>",
        );
        let a = element(&base, "a");
        let text = base.document.node(a).unwrap().children()[0];
        let font = paint::load_bundled_font().unwrap();
        let mut request = inputs();
        request.interaction.hovered = Some(a);
        request.interaction.focused = Some(text);
        request.interaction.focus_visible = true;
        let rendered = render_job(Arc::clone(&base), &request, &font).unwrap();
        let state = rendered.frame.interaction_state();
        assert_eq!(state.hovered, Some(a));
        assert_eq!((state.focused, state.focus_visible), (None, false));
        assert_eq!(rendered.frame.styles.get(a).unwrap().color, Some(0xFF0000));
        assert!(rendered.list.iter().any(|item| matches!(item,
            DisplayItem::Text { text, color, .. } if text == "hi" && *color == 0xFF0000)));
        // The submitted base is never mutated: it may still be presented.
        assert_eq!(base.interaction_state().hovered, None);
    }

    #[test]
    fn render_job_applies_fragment_and_restored_scroll_after_layout() {
        let html = "<body style='margin:0'><div style='height:2000px'></div>\
                    <p id='target'>end</p><div style='height:2000px'></div>";
        let font = paint::load_bundled_font().unwrap();
        let base = frame("https://example.test/#target", html);
        let mut request = inputs();
        request.fragment_scroll = true;
        let rendered = render_job(Arc::clone(&base), &request, &font).unwrap();
        let target = element(&rendered.frame, "target");
        let rect = rendered.tree.node_rect(target).unwrap();
        assert_eq!(rendered.scroll, rect.y);
        assert_eq!(rendered.frame.scroll_state().viewport_scroll().y, rect.y);

        request.restore_scroll = Some(250.0);
        let restored = render_job(base, &request, &font).unwrap();
        assert_eq!(restored.scroll, 250.0);
        assert_eq!(restored.tree.viewport_scroll().y, 250.0);
    }

    #[test]
    fn newer_layout_adopts_retained_scrolling_clamped_to_its_own_ranges() {
        let html = "<body style='margin:0'><div id='s' style='overflow:auto;height:100px'>\
                    <div style='height:1000px'></div></div><div style='height:3000px'></div>";
        let font = paint::load_bundled_font().unwrap();
        let base = frame("https://example.test/", html);
        let scroller = element(&base, "s");
        let presented = render_job(Arc::clone(&base), &inputs(), &font).unwrap();
        let mut retained = presented.tree;
        let max = retained.viewport_scroll_container().range.max.y;
        assert!(retained.set_viewport_scroll(layout::ScrollOffset { x: 0.0, y: max }));
        assert!(retained.set_scroll_offset(scroller, layout::ScrollOffset { x: 0.0, y: 400.0 }));

        // Rendered from the pre-scroll base at a taller window: less range.
        let mut taller = inputs();
        taller.size = PhysicalSize::new(800, 1200);
        let mut next = render_job(Arc::clone(&presented.frame), &taller, &font)
            .unwrap()
            .tree;
        assert_eq!(next.viewport_scroll().y, 0.0);
        assert!(next.adopt_scroll_state(retained.scroll_state()));
        let next_max = next.viewport_scroll_container().range.max.y;
        assert!(next_max < max);
        assert_eq!(next.viewport_scroll().y, next_max);
        assert_eq!(next.scroll_offset(scroller).y, 400.0);
        let same = next.scroll_state().clone();
        assert!(!next.adopt_scroll_state(&same));

        let other = render_job(frame("https://example.test/", html), &inputs(), &font).unwrap();
        let mut other_tree = other.tree;
        assert!(!other_tree.adopt_scroll_state(retained.scroll_state()));
        assert_eq!(other_tree.viewport_scroll().y, 0.0);
    }

    #[test]
    fn worker_renders_off_thread_and_wakes_the_ui() {
        let font = paint::load_bundled_font().unwrap();
        let wakes = Arc::new(AtomicUsize::new(0));
        let counter = Arc::clone(&wakes);
        let worker = RenderWorker::spawn(font, move || {
            counter.fetch_add(1, Ordering::SeqCst);
        })
        .unwrap();
        let base = frame("https://example.test/", "<p>worker</p>");
        worker
            .submit(RenderJob {
                generation: 7,
                frame: base,
                inputs: inputs(),
            })
            .map_err(|_| "worker exited")
            .unwrap();
        let deadline = Instant::now() + Duration::from_secs(30);
        let output = loop {
            if let Some(output) = worker.try_recv() {
                break output;
            }
            assert!(Instant::now() < deadline, "render worker timed out");
            thread::sleep(Duration::from_millis(5));
        };
        assert_eq!(output.generation, 7);
        let rendered = output.result.unwrap();
        assert!(rendered.list.iter().any(|item| matches!(item,
            DisplayItem::Text { text, .. } if text == "worker")));
        assert_eq!(wakes.load(Ordering::SeqCst), 1);
    }
}

use std::{
    error::Error,
    io,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
        mpsc::{self, Receiver, TryRecvError},
    },
    thread,
};

use url::Url;

use super::{Frame, History, LoadOptions, load_page};

pub enum LoadEvent {
    Progress(Arc<Frame>),
    Finished,
}

pub struct Navigation {
    pub history: History,
    options: LoadOptions,
    receiver: Option<Receiver<LoadEvent>>,
    cancelled: Arc<AtomicBool>,
    font_loading: bool,
}

impl Navigation {
    pub fn new(url: Url, options: LoadOptions) -> Self {
        Self {
            history: History::new(url),
            options,
            receiver: None,
            cancelled: Arc::new(AtomicBool::new(false)),
            font_loading: false,
        }
    }

    pub fn start_load(&mut self) -> Result<(), Box<dyn Error>> {
        self.font_loading = false;
        self.cancelled.store(true, Ordering::Relaxed);
        self.receiver = None;
        let cancelled = Arc::new(AtomicBool::new(false));
        let worker_cancelled = Arc::clone(&cancelled);
        let (sender, receiver) = mpsc::sync_channel(1);
        let url = self.history.current().url.clone();
        let options = self.options;
        thread::Builder::new()
            .name("page-loader".into())
            .spawn(move || {
                let mut final_url = url.clone();
                let result = load_page(
                    &url,
                    options,
                    |frame| {
                        final_url = frame.url.clone();
                        sender
                            .send(LoadEvent::Progress(Arc::new(frame.clone())))
                            .map_err(|_| {
                                io::Error::new(io::ErrorKind::BrokenPipe, "page receiver closed")
                                    .into()
                            })
                    },
                    || worker_cancelled.load(Ordering::Relaxed),
                );
                if worker_cancelled.load(Ordering::Relaxed) {
                    return;
                }
                if let Err(error) = result {
                    eprintln!("page load failed for {url}: {error}");
                    if let Some(error) = error.downcast_ref::<crate::net::FetchError>() {
                        final_url = error.url.clone();
                    }
                    let frame = Frame::error_page(final_url, error.to_string());
                    if sender.send(LoadEvent::Progress(Arc::new(frame))).is_err() {
                        return;
                    }
                }
                // A closed receiver means this navigation was replaced or the window closed.
                let _ = sender.send(LoadEvent::Finished);
            })?;
        self.cancelled = cancelled;
        self.receiver = Some(receiver);
        Ok(())
    }

    /// Load newly active fonts without refetching HTML/CSS/images. The same
    /// navigation cancellation/channel lifetime owns this resource worker.
    pub fn start_font_load(&mut self, frame: Arc<Frame>) -> Result<(), Box<dyn Error>> {
        self.cancelled.store(true, Ordering::Relaxed);
        self.receiver = None;
        let cancelled = Arc::new(AtomicBool::new(false));
        let worker_cancelled = Arc::clone(&cancelled);
        let (sender, receiver) = mpsc::sync_channel(1);
        thread::Builder::new()
            .name("font-loader".into())
            .spawn(move || {
                let mut frame = (*frame).clone();
                let result = frame.load_pending_web_fonts(
                    |frame| {
                        sender
                            .send(LoadEvent::Progress(Arc::new(frame.clone())))
                            .map_err(|_| {
                                io::Error::new(io::ErrorKind::BrokenPipe, "font receiver closed")
                                    .into()
                            })
                    },
                    || worker_cancelled.load(Ordering::Relaxed),
                );
                if worker_cancelled.load(Ordering::Relaxed) {
                    return;
                }
                if let Err(error) = result {
                    eprintln!("font resource phase failed: {error}");
                }
                let _ = sender.send(LoadEvent::Finished);
            })?;
        self.cancelled = cancelled;
        self.receiver = Some(receiver);
        self.font_loading = true;
        Ok(())
    }

    pub fn is_font_loading(&self) -> bool {
        self.font_loading
    }

    pub fn try_recv(&mut self) -> Result<LoadEvent, TryRecvError> {
        let event = self
            .receiver
            .as_ref()
            .ok_or(TryRecvError::Disconnected)?
            .try_recv()?;
        if let LoadEvent::Progress(frame) = &event {
            self.history.current_mut().url = frame.url.clone();
        }
        if matches!(event, LoadEvent::Finished) {
            self.font_loading = false;
        }
        Ok(event)
    }
}

impl Drop for Navigation {
    fn drop(&mut self) {
        self.cancelled.store(true, Ordering::Relaxed);
    }
}

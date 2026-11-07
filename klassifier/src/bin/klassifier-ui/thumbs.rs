//! Loads thumbnails off the UI thread. Each photo can have two files: the stand-in that
//! `klassifier.exe --thumbs` writes while classifying, and Lightroom's rendering (with the
//! photo's edits), which can arrive much later. The best one present is loaded, and photos
//! without Lightroom's yet are watched so the better one replaces the stand-in.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Condvar, Mutex};
use std::time::Duration;

use anyhow::Result;
use slint::{Rgba8Pixel, SharedPixelBuffer};

/// How often watched photos are checked for a better file.
const WATCH_INTERVAL: Duration = Duration::from_millis(500);

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Quality {
    Missing,
    Standin,
    Rendered,
}

#[derive(Debug, Clone)]
pub struct Paths {
    pub standin: PathBuf,
    pub rendered: PathBuf,
}

impl Paths {
    fn best(&self) -> Quality {
        if is_file(&self.rendered) {
            Quality::Rendered
        } else if is_file(&self.standin) {
            Quality::Standin
        } else {
            Quality::Missing
        }
    }
}

fn is_file(path: &Path) -> bool {
    !path.as_os_str().is_empty() && path.is_file()
}

/// What an image is for: a grid cell, or the side panel's larger picture of a photo.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Slot {
    Cell(usize),
    Panel(usize),
}

impl Slot {
    pub fn index(self) -> usize {
        match self {
            Slot::Cell(i) | Slot::Panel(i) => i,
        }
    }
}

pub struct Loaded {
    pub slot: Slot,
    pub quality: Quality,
    /// None when no file could be loaded.
    pub buffer: Option<SharedPixelBuffer<Rgba8Pixel>>,
}

struct Job {
    slot: Slot,
    paths: Paths,
    /// Longest edge to decode to, in physical pixels.
    edge: u32,
}

struct Shared {
    /// Served newest first, so the rows on screen load before ones scrolled past.
    jobs: Mutex<Vec<Job>>,
    ready: Condvar,
    /// Images shown with less than Lightroom's thumbnail: slot -> (what they have, job to rerun).
    watched: Mutex<HashMap<Slot, (Quality, Job)>>,
}

pub struct ThumbLoader {
    shared: Arc<Shared>,
}

impl ThumbLoader {
    /// Starts the worker threads. `on_loaded` is called on a worker thread for each finished job.
    pub fn new(on_loaded: impl Fn(Loaded) + Send + Sync + 'static) -> Self {
        let shared = Arc::new(Shared { jobs: Mutex::new(Vec::new()), ready: Condvar::new(), watched: Mutex::new(HashMap::new()) });
        let on_loaded = Arc::new(on_loaded);
        let workers = std::thread::available_parallelism().map(|n| n.get()).unwrap_or(4).clamp(2, 4);
        for _ in 0..workers {
            let shared = shared.clone();
            let on_loaded = on_loaded.clone();
            std::thread::spawn(move || loop {
                let job = {
                    let mut jobs = shared.jobs.lock().unwrap();
                    loop {
                        if let Some(job) = jobs.pop() {
                            break job;
                        }
                        jobs = shared.ready.wait(jobs).unwrap();
                    }
                };
                on_loaded(load(&job));
            });
        }
        let watcher = shared.clone();
        std::thread::spawn(move || loop {
            std::thread::sleep(WATCH_INTERVAL);
            let better: Vec<Job> = {
                let mut watched = watcher.watched.lock().unwrap();
                let ready: Vec<Slot> =
                    watched.iter().filter(|(_, (have, job))| job.paths.best() > *have).map(|(i, _)| *i).collect();
                ready.into_iter().filter_map(|i| watched.remove(&i).map(|(_, job)| job)).collect()
            };
            if !better.is_empty() {
                watcher.jobs.lock().unwrap().extend(better);
                watcher.ready.notify_all();
            }
        });
        ThumbLoader { shared }
    }

    pub fn request(&self, slot: Slot, paths: Paths, edge: u32) {
        self.shared.jobs.lock().unwrap().push(Job { slot, paths, edge });
        self.shared.ready.notify_one();
    }

    /// Reloads `slot` once a better file than `have` appears.
    pub fn watch(&self, slot: Slot, paths: Paths, have: Quality, edge: u32) {
        self.shared.watched.lock().unwrap().insert(slot, (have, Job { slot, paths, edge }));
    }

    pub fn unwatch(&self, slot: Slot) {
        self.shared.watched.lock().unwrap().remove(&slot);
    }
}

fn load(job: &Job) -> Loaded {
    for (quality, path) in [(Quality::Rendered, &job.paths.rendered), (Quality::Standin, &job.paths.standin)] {
        if is_file(path) {
            if let Ok(buffer) = decode(path, job.edge) {
                return Loaded { slot: job.slot, quality, buffer: Some(buffer) };
            }
        }
    }
    Loaded { slot: job.slot, quality: Quality::Missing, buffer: None }
}

fn decode(path: &Path, edge: u32) -> Result<SharedPixelBuffer<Rgba8Pixel>> {
    let image = image::open(path)?;
    let rgba = if image.width().max(image.height()) > edge { image.thumbnail(edge, edge) } else { image }.to_rgba8();
    Ok(SharedPixelBuffer::clone_from_slice(rgba.as_raw(), rgba.width(), rgba.height()))
}

/// Decodes `paths` on the calling thread (for `--screenshot`, which has no event loop).
pub fn load_now(slot: Slot, paths: Paths, edge: u32) -> Loaded {
    load(&Job { slot, paths, edge })
}

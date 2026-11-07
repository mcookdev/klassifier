//! The photo grid's model: one row per grid line, built on demand from the items, so the
//! ListView only asks for the lines on screen. Thumbnails load lazily and are cached.

use std::cell::{Cell as StdCell, RefCell};
use std::collections::{HashMap, HashSet, VecDeque};
use std::rc::Rc;

use crate::ui_model::{grid_rows, Item};
use slint::{ComponentHandle, Image, Model, ModelNotify, ModelRc, ModelTracker, VecModel};

use crate::thumbs::{self, Loaded, Paths, Quality, ThumbLoader};
use crate::{Cell, GridRow, Metrics, ReviewWindow};

/// Decoded thumbnails kept; about 100-250 KB each.
const CACHE_CAP: usize = 400;

pub struct GridModel {
    items: Rc<RefCell<Vec<Item>>>,
    columns: StdCell<usize>,
    /// Longest thumbnail edge to decode to, in physical pixels.
    edge: StdCell<u32>,
    thumb_size: f32,
    cache: RefCell<HashMap<usize, (Image, Quality)>>,
    order: RefCell<VecDeque<usize>>,
    pending: RefCell<HashSet<usize>>,
    /// None for `--screenshot`: thumbnails are then decoded synchronously.
    loader: Option<ThumbLoader>,
    notify: ModelNotify,
}

impl GridModel {
    pub fn new(items: Rc<RefCell<Vec<Item>>>, ui: &ReviewWindow, threaded: bool) -> Rc<Self> {
        let loader = threaded.then(|| {
            let weak = ui.as_weak();
            ThumbLoader::new(move |loaded: Loaded| {
                let _ = weak.upgrade_in_event_loop(move |ui| {
                    if let Some(model) = ui.get_rows().as_any().downcast_ref::<GridModel>() {
                        model.loaded(loaded);
                    }
                });
            })
        });
        let thumb_size = ui.global::<Metrics>().get_thumb_size();
        Rc::new(GridModel {
            items,
            columns: StdCell::new(1),
            edge: StdCell::new(thumb_size.round() as u32),
            thumb_size,
            cache: RefCell::new(HashMap::new()),
            order: RefCell::new(VecDeque::new()),
            pending: RefCell::new(HashSet::new()),
            loader,
            notify: ModelNotify::default(),
        })
    }

    pub fn set_columns(&self, columns: usize) {
        let columns = columns.max(1);
        if columns != self.columns.get() {
            self.columns.set(columns);
            self.notify.reset();
        }
    }

    /// Decode thumbnails for this display scale from now on.
    pub fn set_scale(&self, scale: f32) {
        self.edge.set((self.thumb_size * scale.max(1.0)).round() as u32);
    }

    fn paths(&self, index: usize) -> Paths {
        let items = self.items.borrow();
        Paths { standin: items[index].standin.clone(), rendered: items[index].rendered.clone() }
    }

    fn loaded(&self, loaded: Loaded) {
        let index = loaded.index;
        self.pending.borrow_mut().remove(&index);
        if let Some(buffer) = loaded.buffer {
            self.cache.borrow_mut().insert(index, (Image::from_rgba8(buffer), loaded.quality));
            let mut order = self.order.borrow_mut();
            order.retain(|&i| i != index);
            order.push_back(index);
            while order.len() > CACHE_CAP {
                if let Some(old) = order.pop_front() {
                    self.cache.borrow_mut().remove(&old);
                    if let Some(loader) = &self.loader {
                        loader.unwatch(old);
                    }
                }
            }
        }
        if let Some(loader) = &self.loader {
            if loaded.quality < Quality::Rendered {
                loader.watch(index, self.paths(index), loaded.quality, self.edge.get());
            } else {
                loader.unwatch(index);
            }
        }
        self.notify.row_changed(index / self.columns.get());
    }

    /// Starts loading `index`'s thumbnail unless it is cached or on its way.
    fn ensure_thumb(&self, index: usize) {
        if self.cache.borrow().contains_key(&index) || !self.pending.borrow_mut().insert(index) {
            return;
        }
        match &self.loader {
            Some(loader) => loader.request(index, self.paths(index), self.edge.get()),
            None => {
                let loaded = thumbs::load_now(index, self.paths(index), self.edge.get());
                self.pending.borrow_mut().remove(&index);
                if let Some(buffer) = loaded.buffer {
                    self.cache.borrow_mut().insert(index, (Image::from_rgba8(buffer), loaded.quality));
                }
            }
        }
    }

    fn cell(&self, index: usize) -> Cell {
        self.ensure_thumb(index);
        let image = self.cache.borrow().get(&index).map(|(image, _)| image.clone());
        let items = self.items.borrow();
        let item = &items[index];
        Cell {
            index: index as i32,
            name: item.name.as_str().into(),
            copies: item.copy_note.as_str().into(),
            status: item.status().into(),
            edited: item.edited,
            loaded: image.is_some(),
            thumb: image.unwrap_or_default(),
        }
    }
}

impl Model for GridModel {
    type Data = GridRow;

    fn row_count(&self) -> usize {
        grid_rows(self.items.borrow().len(), self.columns.get())
    }

    fn row_data(&self, row: usize) -> Option<GridRow> {
        let count = self.items.borrow().len();
        let columns = self.columns.get();
        let first = row * columns;
        if first >= count {
            return None;
        }
        let cells: Vec<Cell> = (first..(first + columns).min(count)).map(|i| self.cell(i)).collect();
        Some(GridRow { cells: ModelRc::new(VecModel::from(cells)) })
    }

    fn model_tracker(&self) -> &dyn ModelTracker {
        &self.notify
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}

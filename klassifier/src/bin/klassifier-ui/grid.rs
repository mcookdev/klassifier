//! The review's view of the items: the photo grid's model (one row per grid line, built on
//! demand, so the ListView only asks for the lines on screen) and the side panel showing the
//! selected photo. Thumbnails load lazily and are cached.

use std::cell::{Cell as StdCell, RefCell};
use std::collections::{HashMap, HashSet, VecDeque};
use std::rc::Rc;

use crate::ui_model::{grid_rows, Item};
use slint::{ComponentHandle, Image, Model, ModelNotify, ModelRc, ModelTracker, VecModel};

use crate::thumbs::{self, Loaded, Paths, Quality, Slot, ThumbLoader};
use crate::{Cell, GridRow, Metrics, PanelLabel, ReviewWindow};

/// Decoded thumbnails kept; about 100-250 KB each.
const CACHE_CAP: usize = 400;

pub struct GridModel {
    items: Rc<RefCell<Vec<Item>>>,
    columns: StdCell<usize>,
    /// Longest edge to decode to, in physical pixels: grid thumbnails and the panel's picture.
    edge: StdCell<u32>,
    panel_edge: StdCell<u32>,
    thumb_size: f32,
    panel_size: f32,
    cache: RefCell<HashMap<usize, (Option<Image>, Quality)>>,
    order: RefCell<VecDeque<usize>>,
    pending: RefCell<HashSet<usize>>,
    /// The panel's picture: (photo, image, quality). One photo at a time.
    panel: RefCell<Option<(usize, Image, Quality)>>,
    /// None for `--screenshot`: images are then decoded synchronously.
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
                        model.loaded(&ui, loaded);
                    }
                });
            })
        });
        let metrics = ui.global::<Metrics>();
        let (thumb_size, panel_size) = (metrics.get_thumb_size(), metrics.get_panel_image_width());
        Rc::new(GridModel {
            items,
            columns: StdCell::new(1),
            edge: StdCell::new(thumb_size.round() as u32),
            panel_edge: StdCell::new(panel_size.round() as u32),
            thumb_size,
            panel_size,
            cache: RefCell::new(HashMap::new()),
            order: RefCell::new(VecDeque::new()),
            pending: RefCell::new(HashSet::new()),
            panel: RefCell::new(None),
            loader,
            notify: ModelNotify::default(),
        })
    }

    pub fn columns(&self) -> usize {
        self.columns.get()
    }

    pub fn set_columns(&self, columns: usize) {
        let columns = columns.max(1);
        if columns != self.columns.get() {
            self.columns.set(columns);
            self.notify.reset();
        }
    }

    /// Decode images for this display scale from now on.
    pub fn set_scale(&self, scale: f32) {
        let scale = scale.max(1.0);
        self.edge.set((self.thumb_size * scale).round() as u32);
        self.panel_edge.set((self.panel_size * scale).round() as u32);
    }

    /// Redraws `index`'s cell after its item changed.
    pub fn item_changed(&self, index: usize) {
        self.notify.row_changed(index / self.columns.get());
    }

    fn paths(&self, index: usize) -> Paths {
        let items = self.items.borrow();
        Paths { standin: items[index].standin.clone(), rendered: items[index].rendered.clone() }
    }

    fn loaded(&self, ui: &ReviewWindow, loaded: Loaded) {
        match loaded.slot {
            Slot::Cell(index) => self.cell_loaded(index, loaded),
            Slot::Panel(index) => self.panel_loaded(ui, index, loaded),
        }
    }

    fn cell_loaded(&self, index: usize, loaded: Loaded) {
        self.pending.borrow_mut().remove(&index);
        // Failures are cached too, so a missing or undecodable file is not reloaded on every redraw.
        self.cache.borrow_mut().insert(index, (loaded.buffer.map(Image::from_rgba8), loaded.quality));
        let mut order = self.order.borrow_mut();
        order.retain(|&i| i != index);
        order.push_back(index);
        while order.len() > CACHE_CAP {
            if let Some(old) = order.pop_front() {
                self.cache.borrow_mut().remove(&old);
                if let Some(loader) = &self.loader {
                    loader.unwatch(Slot::Cell(old));
                }
            }
        }
        drop(order);
        self.watch_if_better(Slot::Cell(index), loaded.quality, loaded.tried, self.edge.get());
        self.notify.row_changed(index / self.columns.get());
    }

    fn panel_loaded(&self, ui: &ReviewWindow, index: usize, loaded: Loaded) {
        if ui.get_selected() as usize != index {
            return; // the selection moved on
        }
        if let Some(buffer) = loaded.buffer {
            let image = Image::from_rgba8(buffer);
            ui.set_panel_image(image.clone());
            ui.set_panel_loaded(true);
            *self.panel.borrow_mut() = Some((index, image, loaded.quality));
        }
        self.watch_if_better(Slot::Panel(index), loaded.quality, loaded.tried, self.panel_edge.get());
    }

    /// Reloads `slot` when Lightroom's thumbnail appears, if it has less now. `tried` is the
    /// best file that existed at the last load, so one that failed to decode is not retried.
    fn watch_if_better(&self, slot: Slot, have: Quality, tried: Quality, edge: u32) {
        if let Some(loader) = &self.loader {
            if have < Quality::Rendered {
                loader.watch(slot, self.paths(slot.index()), have, tried, edge);
            } else {
                loader.unwatch(slot);
            }
        }
    }

    /// Starts loading `index`'s thumbnail unless it is cached or on its way.
    fn ensure_thumb(&self, index: usize) {
        if self.cache.borrow().contains_key(&index) || !self.pending.borrow_mut().insert(index) {
            return;
        }
        match &self.loader {
            Some(loader) => loader.request(Slot::Cell(index), self.paths(index), self.edge.get()),
            None => {
                let loaded = thumbs::load_now(Slot::Cell(index), self.paths(index), self.edge.get());
                self.pending.borrow_mut().remove(&index);
                self.cache.borrow_mut().insert(index, (loaded.buffer.map(Image::from_rgba8), loaded.quality));
            }
        }
    }

    fn cell(&self, index: usize) -> Cell {
        self.ensure_thumb(index);
        let image = self.cache.borrow().get(&index).and_then(|(image, _)| image.clone());
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

    /// Shows photo `index` in the side panel: its texts and labels now, and its picture,
    /// the grid's thumbnail until the larger one is loaded.
    pub fn show_panel(&self, ui: &ReviewWindow, index: usize) {
        self.update_panel_labels(ui, index);
        {
            let items = self.items.borrow();
            ui.set_panel_name(items[index].name.as_str().into());
            ui.set_panel_copies(items[index].copy_note.as_str().into());
        }
        let previous = self.panel.borrow().as_ref().map(|(i, _, _)| *i);
        if previous == Some(index) {
            return;
        }
        if let (Some(old), Some(loader)) = (previous, &self.loader) {
            loader.unwatch(Slot::Panel(old));
        }
        *self.panel.borrow_mut() = None;
        let interim = self.cache.borrow().get(&index).and_then(|(image, _)| image.clone());
        ui.set_panel_loaded(interim.is_some());
        ui.set_panel_image(interim.unwrap_or_default());
        let (slot, paths, edge) = (Slot::Panel(index), self.paths(index), self.panel_edge.get());
        match &self.loader {
            Some(loader) => loader.request(slot, paths, edge),
            None => self.panel_loaded(ui, index, thumbs::load_now(slot, paths, edge)),
        }
    }

    /// Refreshes the panel's status and ticks for photo `index`.
    pub fn update_panel_labels(&self, ui: &ReviewWindow, index: usize) {
        let items = self.items.borrow();
        let item = &items[index];
        ui.set_panel_status(item.status().into());
        ui.set_panel_edited(item.edited);
        let labels: Vec<PanelLabel> = item
            .labels
            .iter()
            .map(|label| PanelLabel {
                name: label.name.as_str().into(),
                keyword: label.keyword.as_str().into(),
                checked: item.keeps(label),
                suggested: label.suggested,
            })
            .collect();
        ui.set_panel_labels(ModelRc::new(VecModel::from(labels)));
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

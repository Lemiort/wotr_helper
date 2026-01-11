use egui::{ColorImage, TextureOptions};
use std::path::Path;

#[cfg(not(target_arch = "wasm32"))]
use rfd::FileDialog;

// A named rectangular region on a card (x,y,width,height in card pixel coords)
#[derive(serde::Serialize, serde::Deserialize, Clone, Debug)]
pub struct Region {
    pub name: String,
    pub x: usize,
    pub y: usize,
    pub width: usize,
    pub height: usize,
}

/// We derive Deserialize/Serialize so we can persist app state on shutdown.
#[derive(serde::Deserialize, serde::Serialize)]
#[serde(default)] // if we add new fields, give them default values when deserializing old state
pub struct TemplateApp {
    // Example stuff:
    label: String,

    #[serde(skip)] // This how you opt-out of serialization of a field
    value: f32,

    // Viewer state:
    index: usize, // persist selected index

    // Persist the last opened atlas path (optional)
    atlas_path: Option<String>,

    #[serde(skip)]
    atlas: Option<image::RgbaImage>,

    #[serde(skip)]
    atlas_size: [usize; 2],

    // Card dimensions are persisted so user can change them
    card_width: usize,
    card_height: usize,

    // Selected preset index into CARD_FORMATS or None for custom
    selected_preset: Option<usize>,

    #[serde(skip)]
    texture: Option<egui::TextureHandle>,

    #[serde(skip)]
    last_index: Option<usize>,

    #[serde(skip)]
    error: Option<String>,

    // Regions editor state:
    regions: Vec<Region>, // saved regions (coordinates in card pixels)

    #[serde(skip)]
    drag_start: Option<egui::Pos2>,

    #[serde(skip)]
    drag_current: Option<egui::Pos2>,

    #[serde(skip)]
    pending_region: Option<[usize; 4]>, // x,y,w,h in card pixels while naming

    #[serde(skip)]
    new_region_name: String,

    #[serde(skip)]
    selected_region: Option<usize>,

    #[serde(skip)]
    dragging: bool,

    #[serde(skip)]
    last_pointer_down: bool,

    #[serde(skip)]
    recent_events: std::collections::VecDeque<String>,

    #[serde(skip)]
    recent_events_paused: bool,

    #[serde(skip)]
    event_dump: Option<String>,

    #[serde(skip)]
    pointer_down_on_image: bool,

    /// Runtime toggle to show/hide the regions SidePanel on native builds
    show_regions_panel: bool,
}

const ATLAS_PATH: &str = "assets/light_cards.png"; // Default atlas path; use Open... to pick a different file

// Hardcoded card format presets: (label, width, height)
const CARD_FORMATS: &[(&str, usize, usize)] = &[
    ("Player cards (535×752)", 535, 752),
    ("Fortress (1380x912)", 1380, 912),
    ("Path (1380x912)", 1380, 912),
];

impl Default for TemplateApp {
    fn default() -> Self {
        Self {
            // Example stuff:
            label: "Hello World!".to_owned(),
            value: 2.7,
            // viewer defaults
            index: 0,
            atlas_path: Some(ATLAS_PATH.to_string()),
            atlas: None,
            atlas_size: [0, 0],
            // sensible default card sizes
            card_width: 535,
            card_height: 752,
            selected_preset: None,
            texture: None,
            last_index: None,
            error: None,
            // regions editor defaults
            regions: Vec::new(),
            drag_start: None,
            drag_current: None,
            pending_region: None,
            new_region_name: String::new(),
            selected_region: None,
            dragging: false,
            last_pointer_down: false,
            recent_events: std::collections::VecDeque::with_capacity(256),
            recent_events_paused: false,
            event_dump: None,
            pointer_down_on_image: false,
            show_regions_panel: false,
        }
    }
}

impl TemplateApp {
    /// Called once before the first frame.
    pub fn new(cc: &eframe::CreationContext<'_>) -> Self {
        // This is also where you can customize the look and feel of egui using
        // `cc.egui_ctx.set_visuals` and `cc.egui_ctx.set_fonts`.

        // Load previous app state (if any).
        // Note that you must enable the `persistence` feature for this to work.
        let mut this: Self = if let Some(storage) = cc.storage {
            eframe::get_value(storage, eframe::APP_KEY).unwrap_or_default()
        } else {
            Default::default()
        };

        // Try loading atlas file from assets path
        if let Err(e) = this.load_atlas(Path::new(ATLAS_PATH)) {
            this.error = Some(format!("Failed to load atlas '{}': {}", ATLAS_PATH, e));
        }

        // Ensure a preview texture exists for the current index
        this.ensure_texture(&cc.egui_ctx);

        // Set visuals to dark by default
        cc.egui_ctx.set_visuals(egui::Visuals::dark());

        this
    }

    fn load_atlas(&mut self, path: &Path) -> Result<(), String> {
        let img = image::open(path).map_err(|e| e.to_string())?.to_rgba8();
        let (w, h) = img.dimensions();
        self.atlas = Some(img);
        self.atlas_size = [w as usize, h as usize];
        self.atlas_path = Some(path.to_string_lossy().to_string());
        // Invalidate any existing texture preview; caller should call ensure_texture after
        self.texture = None;
        self.last_index = None;
        Ok(())
    }

    /// Load atlas image from raw bytes (used by the web file picker)
    fn load_atlas_bytes(&mut self, bytes: &[u8]) -> Result<(), String> {
        let img = image::load_from_memory(bytes).map_err(|e| e.to_string())?.to_rgba8();
        let (w, h) = img.dimensions();
        self.atlas = Some(img);
        self.atlas_size = [w as usize, h as usize];
        // no real path when loading from a blob; set a friendly label
        self.atlas_path = Some("(selected)".to_owned());
        // Invalidate texture so it will be recreated
        self.texture = None;
        self.last_index = None;
        Ok(())
    }

    fn cols(&self) -> usize {
        if self.atlas_size[0] == 0 { return 0; }
        self.atlas_size[0] / self.card_width
    }

    fn rows(&self) -> usize {
        if self.atlas_size[1] == 0 { return 0; }
        self.atlas_size[1] / self.card_height
    }

    fn max_index(&self) -> usize {
        let c = self.cols();
        let r = self.rows();
        if c == 0 || r == 0 { 0 } else { c * r - 1 }
    }

    fn make_card_image(&self, index: usize) -> Option<ColorImage> {
        let atlas = self.atlas.as_ref()?;
        let cols = self.cols();
        if cols == 0 { return None; }
        let col = index % cols;
        let row = index / cols;
        if row * self.card_height + self.card_height > self.atlas_size[1] || col * self.card_width + self.card_width > self.atlas_size[0] {
            return None;
        }

        let mut pixels = vec![0u8; self.card_width * self.card_height * 4];
        for y in 0..self.card_height {
            for x in 0..self.card_width {
                let sx = (col * self.card_width + x) as u32;
                let sy = (row * self.card_height + y) as u32;
                let p = atlas.get_pixel(sx, sy);
                let off = (y * self.card_width + x) * 4;
                pixels[off..off + 4].copy_from_slice(&p.0);
            }
        }
        Some(ColorImage::from_rgba_unmultiplied([self.card_width, self.card_height], &pixels))
    }

    fn ensure_texture(&mut self, ctx: &egui::Context) {
        if self.last_index == Some(self.index) { return; }
        self.texture = None;
        self.last_index = None;

        if let Some(img) = self.make_card_image(self.index) {
            let tex = ctx.load_texture(
                "card_preview",
                img,
                TextureOptions::NEAREST,
            );
            self.texture = Some(tex);
            self.last_index = Some(self.index);
        }
    }
}

impl eframe::App for TemplateApp {
    /// Called by the framework to save state before shutdown.
    fn save(&mut self, storage: &mut dyn eframe::Storage) {
        eframe::set_value(storage, eframe::APP_KEY, self);
    }

    /// Called each time the UI needs repainting, which may be many times per second.
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        // Put your widgets into a `SidePanel`, `TopBottomPanel`, `CentralPanel`, `Window` or `Area`.
        // For inspiration and more examples, go to https://emilk.github.io/egui

        egui::TopBottomPanel::top("top_panel").show(ctx, |ui| {
            // The top panel is often a good place for a menu bar:

            egui::MenuBar::new().ui(ui, |ui| {
                // NOTE: no File->Quit on web pages!
                let is_web = cfg!(target_arch = "wasm32");
                if !is_web {
                    ui.menu_button("File", |ui| {
                        if ui.button("Quit").clicked() {
                            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                        }
                    });
                    ui.add_space(16.0);
                }

                egui::widgets::global_theme_preference_buttons(ui);
            });
        });

        #[cfg(not(target_arch = "wasm32"))]
        {
            if self.show_regions_panel {
                egui::SidePanel::right("regions_panel").resizable(true).default_width(260.0).show(ctx, |ui| {
                ui.heading("Regions");
                ui.separator();

                let mut to_delete: Option<usize> = None;

                if let Some([px, py, pw, ph]) = self.pending_region {
                    ui.label("New region pending:");
                    ui.horizontal(|ui| {
                        ui.label(format!("{}×{} @ {},{}", pw, ph, px, py));
                        if ui.button("Add").clicked() {
                            self.regions.push(Region { name: self.new_region_name.clone(), x: px, y: py, width: pw, height: ph });
                            self.selected_region = Some(self.regions.len()-1);
                            self.pending_region = None;
                            self.new_region_name.clear();
                        }
                        if ui.button("Cancel").clicked() {
                            self.pending_region = None;
                            self.new_region_name.clear();
                        }
                    });
                    ui.horizontal(|ui| {
                        ui.label("Name:");
                        ui.add(egui::TextEdit::singleline(&mut self.new_region_name));
                    });
                    ui.separator();
                } else {
                    ui.label("No pending region.");
                    ui.separator();
                }

                ui.label("Saved regions:");
                egui::ScrollArea::vertical().show(ui, |ui| {
                    for (i, r) in self.regions.iter().enumerate() {
                        ui.horizontal(|ui| {
                            let selected = self.selected_region == Some(i);
                            if ui.selectable_label(selected, &r.name).clicked() {
                                self.selected_region = Some(i);
                            }
                            ui.label(format!("{}x{} @ {},{}", r.width, r.height, r.x, r.y));
                            if ui.small_button("Delete").clicked() {
                                to_delete = Some(i);
                            }
                        });
                    }
                });

                if let Some(i) = to_delete {
                    if i < self.regions.len() {
                        self.regions.remove(i);
                        if self.selected_region == Some(i) { self.selected_region = None; }
                    }
                }

                ui.add_space(8.0);
                ui.horizontal(|ui| {
                    if ui.button("Clear All").clicked() {
                        self.regions.clear();
                        self.selected_region = None;
                    }
                    if ui.button("Save...").clicked() {
                        if let Some(path) = FileDialog::new().add_filter("JSON", &["json"]).save_file() {
                            // New format: include the card/image size alongside regions
                            #[derive(serde::Serialize)]
                            struct RegionsFile<'a> {
                                image_size: [usize; 2],
                                regions: &'a [Region],
                            }
                            let file = RegionsFile { image_size: [self.card_width, self.card_height], regions: &self.regions };
                            if let Ok(s) = serde_json::to_string_pretty(&file) {
                                let _ = std::fs::write(path, s);
                            }
                        }
                    }
                    if ui.button("Load...").clicked() {
                        if let Some(path) = FileDialog::new().add_filter("JSON", &["json"]).pick_file() {
                            match std::fs::read_to_string(&path) {
                                Ok(s) => {
                                    // Try new format first (object with image_size + regions), otherwise fall back to old Vec<Region>
                                    #[derive(serde::Deserialize)]
                                    struct RegionsFile {
                                        image_size: [usize; 2],
                                        regions: Vec<Region>,
                                    }

                                    if let Ok(f) = serde_json::from_str::<RegionsFile>(&s) {
                                        self.regions = f.regions;
                                        self.selected_region = None;
                                        // Update card size to match saved file
                                        self.card_width = f.image_size[0].max(1);
                                        self.card_height = f.image_size[1].max(1);
                                        self.selected_preset = None;
                                        self.texture = None; // invalidate preview so it will be recreated
                                        self.last_index = None;
                                    } else if let Ok(v) = serde_json::from_str::<Vec<Region>>(&s) {
                                        // Old format
                                        self.regions = v;
                                        self.selected_region = None;
                                    } else {
                                        self.error = Some("Failed to parse regions file: unknown format".to_owned());
                                    }
                                }
                                Err(e) => { self.error = Some(format!("Failed to read regions file: {}", e)); },
                            }
                        }
                    }
                });
            });
            }
        }





        egui::CentralPanel::default().show(ctx, |ui| {
            // The central panel — Atlas Viewer
            ui.heading("Atlas Viewer");
            egui::warn_if_debug_build(ui);
            ui.separator();

            // --- Atlas viewer UI ---
            ui.label("Atlas Card Preview:");

            // Path / Open / Reload
            ui.horizontal(|ui| {
                ui.label("Atlas:");
                ui.label(self.atlas_path.as_deref().unwrap_or("(none)"));
                if ui.button("Open...").clicked() {
                    #[cfg(not(target_arch = "wasm32"))]
                    {
                        if let Some(path) = FileDialog::new().add_filter("Image", &["png", "jpg", "jpeg"]).pick_file() {
                            match self.load_atlas(&path) {
                                Ok(()) => self.error = None,
                                Err(e) => self.error = Some(e),
                            }
                        }
                    }

                    #[cfg(target_arch = "wasm32")]
                    {
                        crate::file_picker::open_image_picker();
                    }
                }
                if ui.button("Reload").clicked() {
                    if let Some(p) = self.atlas_path.clone() {
                        if let Err(e) = self.load_atlas(Path::new(&p)) {
                            self.error = Some(e);
                        } else {
                            self.error = None;
                        }
                    }
                }
            });

            // Card size controls + presets
            ui.horizontal(|ui| {
                ui.label("Format:");
                let selected_text = self
                    .selected_preset
                    .and_then(|i| CARD_FORMATS.get(i).map(|(n,_,_)| *n))
                    .unwrap_or("Custom");

                egui::ComboBox::from_id_salt("card_format").selected_text(selected_text).show_ui(ui, |ui| {
                    for (i, (name, w, h)) in CARD_FORMATS.iter().enumerate() {
                        if ui.selectable_label(self.selected_preset == Some(i), *name).clicked() {
                            self.selected_preset = Some(i);
                            self.card_width = *w;
                            self.card_height = *h;
                            self.texture = None;
                            self.last_index = None;
                            if self.index > self.max_index() { self.index = self.max_index(); }
                        }
                    }
                    if ui.selectable_label(self.selected_preset.is_none(), "Custom").clicked() {
                        self.selected_preset = None;
                    }
                });

                ui.separator();

                ui.label("Card width:");
                let mut w = self.card_width as i64;
                ui.add(egui::DragValue::new(&mut w).range(1..=4096));
                ui.label("Card height:");
                let mut h = self.card_height as i64;
                ui.add(egui::DragValue::new(&mut h).range(1..=4096));

                let changed = (w as usize != self.card_width) || (h as usize != self.card_height);
                self.card_width = w.max(1) as usize;
                self.card_height = h.max(1) as usize;
                if changed {
                    // If user manually changes size, treat as custom
                    self.selected_preset = None;
                    self.texture = None;
                    self.last_index = None;
                    // clamp index
                    if self.index > self.max_index() { self.index = self.max_index(); }
                }
            });

            ui.horizontal(|ui| {
                ui.label("Card index:");
                let mut idx = self.index as i64;
                ui.add(egui::DragValue::new(&mut idx).range(0..=self.max_index() as i64));
                if ui.button("Prev").clicked() {
                    idx = (idx - 1).max(0);
                }
                if ui.button("Next").clicked() {
                    let max = self.max_index() as i64;
                    idx = (idx + 1).min(max);
                }
                let max = self.max_index() as i64;
                idx = idx.clamp(0, max);
                self.index = idx as usize;

                ui.separator();
                ui.label(format!("Atlas: {}x{} | cols: {} rows: {} | max index: {}", self.atlas_size[0], self.atlas_size[1], self.cols(), self.rows(), self.max_index()));
            });

            // Show/hide Regions panel (native only)
            #[cfg(not(target_arch = "wasm32"))]
            ui.checkbox(&mut self.show_regions_panel, "Show regions panel");

            if let Some(err) = &self.error {
                ui.colored_label(egui::Color32::RED, err);
                ui.label("Place your atlas image and use Open... to pick it.");
            } else {
                // Ensure texture exists / is updated if index changed
                self.ensure_texture(ctx);

                if let Some(tex) = &self.texture {
                    ui.vertical_centered(|ui| {
                        // Fit the preview into available space while preserving aspect ratio
                        let avail = ui.available_size();
                        let cw = self.card_width as f32;
                        let ch = self.card_height as f32;
                        // Reserve some space so UI controls remain visible. Allow scaling up to 4x.
                        let max_w = (avail.x - 20.0).max(10.0);
                        let max_h = ((avail.y * 1.0) - 20.0).max(10.0);
                        let scale_x = max_w / cw;
                        let scale_y = max_h / ch;
                        let mut scale = scale_x.min(scale_y);
                        scale = scale.clamp(0.1, 4.0);
                        let desired_size = egui::vec2(cw * scale, ch * scale);

                        // Show image and capture response for mouse interactions
                        let img_widget = egui::Image::new((tex.id(), desired_size));
                        let resp = ui.add(img_widget.sense(egui::Sense::click_and_drag()));
                        let img_rect = resp.rect;

                        // Minimal debug: show hovered+clicked. Disabled on wasm builds.
                        if self.show_regions_panel {
                            egui::TopBottomPanel::bottom("debug_panel").show(ctx, |ui| {
                                ui.horizontal_wrapped(|ui| {
                                    ui.label(format!("hovered: {}", resp.hovered()));
                                    ui.separator();
                                    ui.label(format!("contains_pointer: {}", resp.contains_pointer()));
                                    ui.separator();
                                    ui.label(format!("pointer_down_on: {}", resp.is_pointer_button_down_on()));
                                    ui.separator();
                                    ui.label(format!("interact_pos: {:?}", resp.interact_pointer_pos()));
                                    ui.separator();
                                    ui.label(format!("drag_started: {}", resp.drag_started_by(egui::PointerButton::Primary)));
                                    ui.separator();
                                    ui.label(format!("dragged: {}", resp.dragged_by(egui::PointerButton::Primary)));
                                    ui.separator();
                                    ui.label(format!("drag_stopped: {}", resp.drag_stopped_by(egui::PointerButton::Primary)));
                                    ui.separator();
                                    ui.label(format!("clicked: {}", resp.clicked_by(egui::PointerButton::Primary)));
                                });
                            });
                        }

                        #[cfg(not(target_arch = "wasm32"))]
                        {
                            // Additional fallback: process raw pointer events to detect presses/drags/releases when Response misses them
                            const DRAG_THRESHOLD: f32 = 4.0;
                            let events = ctx.input(|i| i.events.clone());
                            for ev in events.iter() {
                                match ev {
                                    egui::Event::PointerButton { pos, button, pressed, .. } => {
                                        if *button == egui::PointerButton::Primary {
                                            if *pressed {
                                                if img_rect.contains(*pos) {
                                                    self.pointer_down_on_image = true;
                                                    self.drag_start = Some(*pos);
                                                    self.drag_current = Some(*pos);
                                                    self.dragging = false;
                                                } else {
                                                    self.pointer_down_on_image = false;
                                                }
                                            } else {
                                                // release
                                                if self.pointer_down_on_image || self.dragging {
                                                    let end = *pos;
                                                    if self.dragging {
                                                        if let Some(start) = self.drag_start {
                                                            let local_start = start - img_rect.min;
                                                            let local_end = end - img_rect.min;
                                                            let sx = local_start.x.clamp(0.0, img_rect.width());
                                                            let sy = local_start.y.clamp(0.0, img_rect.height());
                                                            let ex = local_end.x.clamp(0.0, img_rect.width());
                                                            let ey = local_end.y.clamp(0.0, img_rect.height());
                                                            let lx = sx.min(ex);
                                                            let ly = sy.min(ey);
                                                            let lw = (sx - ex).abs();
                                                            let lh = (sy - ey).abs();
                                                            let scale_ui_to_px = 1.0 / scale;
                                                            let px = (lx * scale_ui_to_px).round().max(0.0) as usize;
                                                            let py = (ly * scale_ui_to_px).round().max(0.0) as usize;
                                                            let pw = (lw * scale_ui_to_px).round().max(1.0) as usize;
                                                            let ph = (lh * scale_ui_to_px).round().max(1.0) as usize;
                                                            #[cfg(not(target_arch = "wasm32"))]
                                                            {
                                                                self.pending_region = Some([px, py, pw, ph]);
                                                                self.new_region_name = format!("region{}", self.regions.len() + 1);
                                                            }
                                                        }
                                                    } else {
                                                        // click
                                                        if img_rect.contains(end) {
                                                            let local = end - img_rect.min;
                                                            let scale_ui_to_px = 1.0 / scale;
                                                            let px = (local.x * scale_ui_to_px).floor().max(0.0) as usize;
                                                            let py = (local.y * scale_ui_to_px).floor().max(0.0) as usize;
                                                            let mut found: Option<usize> = None;
                                                            for (i, r) in self.regions.iter().enumerate() {
                                                                if px >= r.x && px < r.x + r.width && py >= r.y && py < r.y + r.height {
                                                                    found = Some(i);
                                                                    break;
                                                                }
                                                            }
                                                            self.selected_region = found;
                                                        } else {
                                                            self.selected_region = None;
                                                        }
                                                    }
                                                }
                                                self.pointer_down_on_image = false;
                                                self.drag_start = None;
                                                self.drag_current = None;
                                                self.dragging = false;
                                            }
                                        }
                                    }
                                    egui::Event::PointerMoved(pos) => {
                                        if self.pointer_down_on_image {
                                            if let Some(start) = self.drag_start {
                                                let dist = ((*pos) - start).length();
                                                if !self.dragging && dist > DRAG_THRESHOLD {
                                                    self.dragging = true;
                                                }
                                                if self.dragging {
                                                    self.drag_current = Some(*pos);
                                                    // update live pending region
                                                    let local_start = start - img_rect.min;
                                                    let local_pos = (*pos) - img_rect.min;
                                                    let sx = local_start.x.clamp(0.0, img_rect.width());
                                                    let sy = local_start.y.clamp(0.0, img_rect.height());
                                                    let ex = local_pos.x.clamp(0.0, img_rect.width());
                                                    let ey = local_pos.y.clamp(0.0, img_rect.height());
                                                    let lx = sx.min(ex);
                                                    let ly = sy.min(ey);
                                                    let lw = (sx - ex).abs();
                                                    let lh = (sy - ey).abs();
                                                    let scale_ui_to_px = 1.0 / scale;
                                                    let px = (lx * scale_ui_to_px).round().max(0.0) as usize;
                                                    let py = (ly * scale_ui_to_px).round().max(0.0) as usize;
                                                    let pw = (lw * scale_ui_to_px).round().max(1.0) as usize;
                                                    let ph = (lh * scale_ui_to_px).round().max(1.0) as usize;
                                                    #[cfg(not(target_arch = "wasm32"))]
                                                    {
                                                        self.pending_region = Some([px, py, pw, ph]);
                                                        if self.new_region_name.is_empty() {
                                                            self.new_region_name = format!("region{}", self.regions.len() + 1);
                                                        }
                                                    }
                                                }
                                            }
                                        }
                                    }
                                    _ => {}
                                }
                            }
                        }

                        /* old input handling disabled: */ if false {
                        // Enhanced drag handling with a small movement threshold:
                        // - Quick click (press+release without moving) is treated as selection
                        // - Click+drag (movement > threshold) creates a pending region on release
                        const DRAG_THRESHOLD: f32 = 4.0;


                        // Prefer explicit PointerButton events to detect presses/releases reliably
                        let events = ctx.input(|i| i.events.clone());
                        let mut released_event = false;
                        for ev in events.iter() {
                            match ev {
                                egui::Event::PointerButton { button, pressed, .. } => {
                                    if *button == egui::PointerButton::Primary {
                                        if !*pressed { released_event = true; }
                                    }
                                }
                                _ => {}
                            }
                        }

                        // Use hover_pos and interact_pos as before, but combine event-derived flags
                        let hover_pos = ctx.input(|i| i.pointer.hover_pos());
                        let pos_opt = ctx.input(|i| i.pointer.interact_pos()).or(hover_pos);
                        let down = ctx.input(|i| i.pointer.any_down());
                        let released = released_event || ctx.input(|i| i.pointer.any_released());

                        // Start potential drag when pointer pressed while hovering the image
                        // legacy press handling removed (using Response::drag_started_by)
                        // if needed, use resp.drag_started_by/resp.clicked_by/resp.interact_pointer_pos() instead.

                        // Update while pointer is down
                        if down {
                            if let (Some(start), Some(pos)) = (self.drag_start, pos_opt.or(hover_pos)) {
                                self.drag_current = Some(pos);
                                let dist = (pos - start).length();
                                if dist > DRAG_THRESHOLD {
                                    self.dragging = true;
                                }

                                // While dragging, update a live pending region so user can Add even if release isn't observed
                                if self.dragging {
                                    // Convert screen coords to local image coords
                                    let local_start = start - img_rect.min;
                                    let local_pos = pos - img_rect.min;

                                    // Clamp to image rect
                                    let sx = local_start.x.clamp(0.0, img_rect.width());
                                    let sy = local_start.y.clamp(0.0, img_rect.height());
                                    let ex = local_pos.x.clamp(0.0, img_rect.width());
                                    let ey = local_pos.y.clamp(0.0, img_rect.height());

                                    let lx = sx.min(ex);
                                    let ly = sy.min(ey);
                                    let lw = (sx - ex).abs();
                                    let lh = (sy - ey).abs();

                                    // Convert to card pixel coords
                                    let scale_ui_to_px = 1.0 / scale;
                                    let px = (lx * scale_ui_to_px).round().max(0.0) as usize;
                                    let py = (ly * scale_ui_to_px).round().max(0.0) as usize;
                                    let pw = (lw * scale_ui_to_px).round().max(1.0) as usize;
                                    let ph = (lh * scale_ui_to_px).round().max(1.0) as usize;

                                    #[cfg(not(target_arch = "wasm32"))]
                                    {
                                        self.pending_region = Some([px, py, pw, ph]);
                                        if self.new_region_name.is_empty() {
                                            self.new_region_name = format!("region{}", self.regions.len() + 1);
                                        }
                                    }
                                }
                            }
                        }

                        // On release: if we were dragging, create a pending region; otherwise selection logic handles clicks
                        if released && self.drag_start.is_some() {
                            let start = self.drag_start.unwrap();
                            let end = pos_opt.or(self.drag_current).or(hover_pos).unwrap_or(start);

                            if self.dragging {
                                // Convert screen coords to local image coords
                                let local_start = start - img_rect.min;
                                let local_end = end - img_rect.min;

                                // Clamp to image rect
                                let sx = local_start.x.clamp(0.0, img_rect.width());
                                let sy = local_start.y.clamp(0.0, img_rect.height());
                                let ex = local_end.x.clamp(0.0, img_rect.width());
                                let ey = local_end.y.clamp(0.0, img_rect.height());

                                let lx = sx.min(ex);
                                let ly = sy.min(ey);
                                let lw = (sx - ex).abs();
                                let lh = (sy - ey).abs();

                                // Convert to card pixel coords
                                let scale_ui_to_px = 1.0 / scale; // since desired_size = card_size * scale
                                let px = (lx * scale_ui_to_px).round().max(0.0) as usize;
                                let py = (ly * scale_ui_to_px).round().max(0.0) as usize;
                                let pw = (lw * scale_ui_to_px).round().max(1.0) as usize;
                                let ph = (lh * scale_ui_to_px).round().max(1.0) as usize;

                                #[cfg(not(target_arch = "wasm32"))]
                                {
                                    self.pending_region = Some([px, py, pw, ph]);
                                    self.new_region_name = format!("region{}", self.regions.len() + 1);
                                }
                            }

                            self.drag_start = None;
                            self.drag_current = None;
                            self.dragging = false;
                        }

                        // Also handle pointer-up that occurred outside of widget (e.g. released while cursor moved off image)
                        let current_down = down;
                        if self.dragging && self.last_pointer_down && !current_down {
                            // Treat similar to release while dragging
                            if let Some(start) = self.drag_start {
                                let end = self.drag_current.or(pos_opt).or(ctx.input(|i| i.pointer.hover_pos())).unwrap_or(start);

                                // Convert screen coords to local image coords
                                let local_start = start - img_rect.min;
                                let local_end = end - img_rect.min;

                                // Clamp to image rect
                                let sx = local_start.x.clamp(0.0, img_rect.width());
                                let sy = local_start.y.clamp(0.0, img_rect.height());
                                let ex = local_end.x.clamp(0.0, img_rect.width());
                                let ey = local_end.y.clamp(0.0, img_rect.height());

                                let lx = sx.min(ex);
                                let ly = sy.min(ey);
                                let lw = (sx - ex).abs();
                                let lh = (sy - ey).abs();

                                // Convert to card pixel coords
                                let scale_ui_to_px = 1.0 / scale; // since desired_size = card_size * scale
                                let px = (lx * scale_ui_to_px).round().max(0.0) as usize;
                                let py = (ly * scale_ui_to_px).round().max(0.0) as usize;
                                let pw = (lw * scale_ui_to_px).round().max(1.0) as usize;
                                let ph = (lh * scale_ui_to_px).round().max(1.0) as usize;

                                #[cfg(not(target_arch = "wasm32"))]
                                {
                                    self.pending_region = Some([px, py, pw, ph]);
                                    self.new_region_name = format!("region{}", self.regions.len() + 1);
                                }
                            }

                            self.drag_start = None;
                            self.drag_current = None;
                            self.dragging = false;
                        }

                        // Update last pointer down state for next frame
                        self.last_pointer_down = current_down;

                        // Click (release while hovering) selects a region if released inside it; clicking outside clears selection.
                        // Do not run selection if a pending region was just created this frame.
                        // legacy click handling removed; use resp.clicked_by to detect clicks instead

                        }

                        // Paint overlays (existing regions and drag preview)
                        let painter = ui.painter();
                        // Draw existing regions
                        for (i, r) in self.regions.iter().enumerate() {
                            let x = img_rect.min.x + (r.x as f32) * scale;
                            let y = img_rect.min.y + (r.y as f32) * scale;
                            let w = (r.width as f32) * scale;
                            let h = (r.height as f32) * scale;
                            let rect = egui::Rect::from_min_size(egui::pos2(x, y), egui::vec2(w, h));
                            let color = if self.selected_region == Some(i) { egui::Color32::LIGHT_BLUE } else { egui::Color32::from_rgba_unmultiplied(200, 100, 100, 180) };
                            let stroke = egui::Stroke::new(2.0, color);
                            painter.line_segment([rect.left_top(), rect.right_top()], stroke);
                            painter.line_segment([rect.right_top(), rect.right_bottom()], stroke);
                            painter.line_segment([rect.right_bottom(), rect.left_bottom()], stroke);
                            painter.line_segment([rect.left_bottom(), rect.left_top()], stroke);
                            if self.selected_region == Some(i) {
                                painter.rect_filled(rect.expand(2.0), 2.0, egui::Color32::from_rgba_unmultiplied(40, 100, 160, 48));
                            }
                        }

                        // Draw drag preview if dragging
                        if let (Some(start), Some(cur)) = (self.drag_start, self.drag_current) {
                            let local_start = start - img_rect.min;
                            let local_cur = cur - img_rect.min;
                            let lx = local_start.x.min(local_cur.x).clamp(0.0, img_rect.width());
                            let ly = local_start.y.min(local_cur.y).clamp(0.0, img_rect.height());
                            let lw = (local_start.x - local_cur.x).abs().clamp(1.0, img_rect.width());
                            let lh = (local_start.y - local_cur.y).abs().clamp(1.0, img_rect.height());
                            let rect = egui::Rect::from_min_size(img_rect.min + egui::vec2(lx, ly), egui::vec2(lw, lh));
                            let stroke = egui::Stroke::new(2.0, egui::Color32::YELLOW);
                            painter.line_segment([rect.left_top(), rect.right_top()], stroke);
                            painter.line_segment([rect.right_top(), rect.right_bottom()], stroke);
                            painter.line_segment([rect.right_bottom(), rect.left_bottom()], stroke);
                            painter.line_segment([rect.left_bottom(), rect.left_top()], stroke);
                        }

                        // Draw pending region (after release, before naming)
                        if let Some([px, py, pw, ph]) = self.pending_region {
                            let x = img_rect.min.x + (px as f32) * scale;
                            let y = img_rect.min.y + (py as f32) * scale;
                            let w = (pw as f32) * scale;
                            let h = (ph as f32) * scale;
                            let rect = egui::Rect::from_min_size(egui::pos2(x, y), egui::vec2(w, h));
                            let stroke = egui::Stroke::new(2.0, egui::Color32::from_rgba_unmultiplied(255, 200, 0, 200));
                            painter.line_segment([rect.left_top(), rect.right_top()], stroke);
                            painter.line_segment([rect.right_top(), rect.right_bottom()], stroke);
                            painter.line_segment([rect.right_bottom(), rect.left_bottom()], stroke);
                            painter.line_segment([rect.left_bottom(), rect.left_top()], stroke);
                            painter.rect_filled(rect, 0.0, egui::Color32::from_rgba_unmultiplied(255, 200, 0, 40));
                        }

                        // Debug moved to SidePanel (right) for visibility
                    });


                } else {
                    ui.label("No preview available for this index (out of range or atlas missing).");
                }
            }
        });

        // On web builds, check if the user picked a file (async callback writes bytes into the picker buffer)
        #[cfg(target_arch = "wasm32")]
        {
            if let Some((bytes, filename)) = crate::file_picker::take_selected_image_bytes() {
                match self.load_atlas_bytes(&bytes) {
                    Ok(()) => {
                        self.error = None;
                        self.atlas_path = Some(filename);
                    }
                    Err(e) => self.error = Some(e),
                }
            }
        }
    }
}


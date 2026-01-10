use egui::{ColorImage, TextureOptions};
use std::path::Path;

#[cfg(not(target_arch = "wasm32"))]
use rfd::FileDialog;

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

        egui::CentralPanel::default().show(ctx, |ui| {
            // The central panel — Atlas Viewer
            ui.heading("Atlas Viewer");
            ui.separator();

            // --- Atlas viewer UI ---
            ui.label("Atlas Card Preview:");

            // Path / Open / Reload
            ui.horizontal(|ui| {
                ui.label("Atlas:");
                ui.label(self.atlas_path.as_deref().unwrap_or("(none)"));
                if ui.button("Open...").clicked() {
                    #[cfg(not(target_arch = "wasm32"))]
                    if let Some(path) = FileDialog::new().add_filter("Image", &["png", "jpg", "jpeg"]).pick_file() {
                        match self.load_atlas(&path) {
                            Ok(()) => self.error = None,
                            Err(e) => self.error = Some(e),
                        }
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
                        let max_h = (avail.y - 20.0).max(10.0);
                        let scale_x = max_w / cw;
                        let scale_y = max_h / ch;
                        let mut scale = scale_x.min(scale_y);
                        scale = scale.clamp(0.1, 4.0);
                        let desired_size = egui::vec2(cw * scale, ch * scale);
                        ui.add(egui::Image::new((tex.id(), desired_size)));
                    });
                } else {
                    ui.label("No preview available for this index (out of range or atlas missing).");
                }
            }

            ui.separator();

            ui.add(egui::github_link_file!(
                "https://github.com/emilk/eframe_template/blob/main/",
                "Source code."
            ));

            ui.with_layout(egui::Layout::bottom_up(egui::Align::LEFT), |ui| {
                powered_by_egui_and_eframe(ui);
                egui::warn_if_debug_build(ui);
            });
        });
    }
}

fn powered_by_egui_and_eframe(ui: &mut egui::Ui) {
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 0.0;
        ui.label("Powered by ");
        ui.hyperlink_to("egui", "https://github.com/emilk/egui");
        ui.label(" and ");
        ui.hyperlink_to(
            "eframe",
            "https://github.com/emilk/egui/tree/master/crates/eframe",
        );
        ui.label(".");
    });
}

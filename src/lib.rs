#![warn(clippy::all, rust_2018_idioms)]

mod app;
pub use app::TemplateApp;

use eframe::NativeOptions;

#[cfg(target_os = "android")]
use egui_winit::winit;

impl TemplateApp {
    /// Run the app with provided NativeOptions (used by Android entrypoint).
    pub fn run(options: NativeOptions) -> Result<(), eframe::Error> {
        eframe::run_native(
            "wotr_helper",
            options,
            Box::new(|cc| Ok(Box::new(TemplateApp::new(cc)))),
        )
    }
}

#[cfg(target_os = "android")]
#[allow(unsafe_code)]
#[unsafe(no_mangle)]
pub extern "C" fn android_main(app: winit::platform::android::activity::AndroidApp) {
    use eframe::Renderer;

    unsafe {
        std::env::set_var("RUST_BACKTRACE", "full");
    }
    android_logger::init_once(
        android_logger::Config::default().with_max_level(log::LevelFilter::Info),
    );

    let options = NativeOptions {
        android_app: Some(app),
        renderer: Renderer::Wgpu,
        ..Default::default()
    };

    TemplateApp::run(options).unwrap();
}

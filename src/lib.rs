pub mod android_bridge;
pub mod app;
pub mod fs_ops;
pub mod icons;
pub mod model;
pub mod prefs;
pub mod theme;
pub mod trash;
pub mod zip_ops;

#[cfg(target_os = "android")]
mod android_entry {
    use crate::android_bridge::PlatformBridge;
    use crate::app::TabManagerApp;
    use winit::platform::android::activity::AndroidApp;

    pub fn run(app: AndroidApp) {
        android_logger::init_once(
            android_logger::Config::default()
                .with_max_level(log::LevelFilter::Info)
                .with_tag("TabManager"),
        );

        let bridge = PlatformBridge::from_android_app(&app);
        let data_dir = app
            .internal_data_path()
            .unwrap_or_else(|| std::path::PathBuf::from("/data/local/tmp"));

        let options = eframe::NativeOptions {
            android_app: Some(app),
            depth_buffer: 0,
            stencil_buffer: 0,
            multisampling: 1,
            persistence_path: Some(data_dir.join("eframe.ron")),
            ..Default::default()
        };

        let result = eframe::run_native(
            "TabManager",
            options,
            Box::new(move |cc| {
                crate::theme::install_fonts(&cc.egui_ctx);
                Ok(Box::new(TabManagerApp::new(bridge, data_dir)))
            }),
        );

        if let Err(e) = result {
            log::error!("TabManager failed to start: {e}");
        }

        // Exit cleanly without running destructors while Android system threads still use them
        // (same pattern as SolidCraft Android).
        unsafe { libc::_exit(0) }
    }
}

#[cfg(target_os = "android")]
#[unsafe(no_mangle)]
fn android_main(app: winit::platform::android::activity::AndroidApp) {
    android_entry::run(app);
}

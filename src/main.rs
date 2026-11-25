#[cfg(not(target_os = "android"))]
fn main() -> eframe::Result<()> {
    use tabmanager::android_bridge::PlatformBridge;
    use tabmanager::app::TabManagerApp;

    let bridge = PlatformBridge::desktop();
    let config_dir = std::env::var("HOME")
        .map(|h| std::path::PathBuf::from(h).join(".config/tabmanager"))
        .unwrap_or_else(|_| std::path::PathBuf::from(".tabmanager"));

    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("TabManager — High-End Rust File Manager")
            .with_inner_size([1440.0, 900.0]),
        ..Default::default()
    };

    eframe::run_native(
        "TabManager",
        options,
        Box::new(move |_cc| Ok(Box::new(TabManagerApp::new(bridge, config_dir)))),
    )
}

#[cfg(target_os = "android")]
fn main() {}

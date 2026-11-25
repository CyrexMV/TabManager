# TabManager

Ein moderner, ultraschneller **High-End-Dateimanager komplett in Rust** (`eframe` + `egui` + `wgpu`), speziell optimiert für das **Samsung Galaxy Tab S12 Ultra** (14,6" AMOLED, 120 Hz, Dual-Pane & Multi-Tab).

## Features

- **100 % Rust Kern & UI**: Native Performance via `libtabmanager.so` auf `NativeActivity` + `wgpu` (Vulkan / GLES) + 120 Hz Display-Modus.
- **Dual-Pane Split-View & Multi-Tabs**: Zwei unabhängige Dateifenster nebeneinander mit Browser-Tabs (`+` / `×`) und 1-Tap-Direkttransfer (*→ Kopieren* / *→ Verschieben*).
- **Vollständige Dateiverwaltung**: Kopieren, Ausschneiden, Verschieben, Einfügen, Umbenennen, Neuer Ordner, Neue Datei, Versteckte Dateien (`.dot`), Mehrfachauswahl.
- **ZIP-Engine (Pure Rust)**:
  - Dateien & ganze Ordner rekursiv in `.zip` komprimieren (mit Fortschritt & Abbruch).
  - `.zip`-Archive inspizieren (Inhalt vorab anzeigen) und sicher entpacken (*Hier* oder *In Unterordner*, inkl. Zip-Slip-Schutz).
- **Papierkorb (`.TabManagerTrash`)**: Sicheres Löschen mit Wiederherstellung an den Ursprungspfad oder endgültigem Löschen.
- **Integrierte Viewer**:
  - **Bildbetrachter** (natives Rust-Decoding mit Zoom)
  - **Text-, Code- & JSON-Editor** (direkt bearbeiten & speichern)
  - **ZIP-Inspektor**
  - **Externe Apps & Teilen** via Android `FileProvider`
- **Übersicht & Inspektor**: Live-Speicherbalken (Interner Speicher, SD-Karte / USB-OTG), Kategorien (Bilder, Videos, Audio, Dokumente, Archive, APKs), Favoriten, Tiefensuche, Ordnergrößen-Berechnung und **SHA-256-Prüfsumme**.
- **AMOLED Dark & Light Theme**: Tiefes Obsidian-AMOLED-Design, das auf dem Dynamic AMOLED 2X Display gestochen scharf aussieht.

## APK selbst bauen

Voraussetzungen:
- Rust + `rustup target add aarch64-linux-android`
- `cargo install cargo-ndk`
- Android SDK + NDK `29.0.14206865` + JDK 17/21

```bash
cd android
./gradlew assembleRelease
```

Die fertige APK liegt danach unter:
`android/app/build/outputs/apk/release/TabManager-1.0.0-release.apk`

package com.cyrexmv.tabmanager;

import android.app.NativeActivity;
import android.content.ActivityNotFoundException;
import android.content.Context;
import android.content.Intent;
import android.net.Uri;
import android.os.Build;
import android.os.Bundle;
import android.os.Environment;
import android.os.storage.StorageManager;
import android.os.storage.StorageVolume;
import android.provider.Settings;
import android.system.ErrnoException;
import android.system.Os;
import android.view.Display;
import android.view.View;
import android.view.WindowManager;
import android.view.inputmethod.InputMethodManager;
import android.webkit.MimeTypeMap;
import android.window.OnBackInvokedDispatcher;

import androidx.core.content.FileProvider;

import java.io.File;
import java.util.ArrayList;
import java.util.HashSet;
import java.util.Set;
import java.util.concurrent.atomic.AtomicBoolean;

/**
 * Thin NativeActivity wrapper around libtabmanager.so (100% Rust UI & file engine).
 * Provides only Android OS bridges that NativeActivity cannot perform directly in Rust:
 * - 120 Hz display mode on Galaxy Tab S12 Ultra
 * - All Files Access (MANAGE_EXTERNAL_STORAGE) check & settings intent
 * - FileProvider ACTION_VIEW / ACTION_SEND intents
 * - Back gesture forwarding & soft keyboard control
 * - Storage volume discovery (Internal + SD / USB OTG)
 */
public class TabManagerActivity extends NativeActivity {

    private final AtomicBoolean backPressed = new AtomicBoolean(false);
    private volatile int insetTop = 0;
    private volatile int insetBottom = 0;
    private volatile int insetLeft = 0;
    private volatile int insetRight = 0;

    @Override
    protected void onCreate(Bundle savedInstanceState) {
        try {
            Os.setenv("TMPDIR", getCacheDir().getAbsolutePath(), true);
        } catch (ErrnoException ignored) {
        }
        super.onCreate(savedInstanceState);

        requestHighRefreshRate();

        if (Build.VERSION.SDK_INT >= 30) {
            getWindow().getDecorView().setOnApplyWindowInsetsListener((v, insets) -> {
                android.graphics.Insets bars = insets.getInsets(
                        android.view.WindowInsets.Type.systemBars()
                                | android.view.WindowInsets.Type.displayCutout()
                                | android.view.WindowInsets.Type.captionBar()
                );
                insetTop = bars.top;
                insetBottom = bars.bottom;
                insetLeft = bars.left;
                insetRight = bars.right;
                return v.onApplyWindowInsets(insets);
            });
            getWindow().getDecorView().requestApplyInsets();
        }

        if (Build.VERSION.SDK_INT >= 33) {
            getOnBackInvokedDispatcher().registerOnBackInvokedCallback(
                    OnBackInvokedDispatcher.PRIORITY_DEFAULT,
                    () -> backPressed.set(true)
            );
        }
    }

    public String getSystemInsetsEncoded() {
        return insetTop + "," + insetBottom + "," + insetLeft + "," + insetRight;
    }

    @Override
    @SuppressWarnings("deprecation")
    public void onBackPressed() {
        backPressed.set(true);
    }

    private void requestHighRefreshRate() {
        try {
            Display display = Build.VERSION.SDK_INT >= 30
                    ? getDisplay()
                    : getWindowManager().getDefaultDisplay();
            if (display == null) return;
            Display.Mode best = display.getMode();
            for (Display.Mode mode : display.getSupportedModes()) {
                if (mode.getPhysicalWidth() == best.getPhysicalWidth()
                        && mode.getPhysicalHeight() == best.getPhysicalHeight()
                        && mode.getRefreshRate() > best.getRefreshRate()) {
                    best = mode;
                }
            }
            WindowManager.LayoutParams params = getWindow().getAttributes();
            params.preferredDisplayModeId = best.getModeId();
            getWindow().setAttributes(params);
        } catch (Exception ignored) {
        }
    }

    // ---- JNI-called methods from Rust (src/android_bridge.rs) ----

    public boolean takeBackPressed() {
        return backPressed.getAndSet(false);
    }

    public void moveToBackground() {
        runOnUiThread(() -> moveTaskToBack(true));
    }

    public boolean hasAllFilesAccess() {
        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.R) {
            return Environment.isExternalStorageManager();
        }
        return true;
    }

    public void requestAllFilesAccess() {
        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.R) {
            runOnUiThread(() -> {
                try {
                    Intent intent = new Intent(Settings.ACTION_MANAGE_APP_ALL_FILES_ACCESS_PERMISSION);
                    intent.setData(Uri.parse("package:" + getPackageName()));
                    startActivity(intent);
                } catch (Exception e) {
                    Intent fallback = new Intent(Settings.ACTION_MANAGE_ALL_FILES_ACCESS_PERMISSION);
                    startActivity(fallback);
                }
            });
        }
    }

    public String openFileExternal(String path) {
        try {
            File file = new File(path);
            if (!file.exists()) {
                return "Datei existiert nicht: " + path;
            }
            Uri uri = FileProvider.getUriForFile(this, getPackageName() + ".fileprovider", file);
            String mime = getMimeType(file);
            Intent intent = new Intent(Intent.ACTION_VIEW);
            intent.setDataAndType(uri, mime);
            intent.addFlags(Intent.FLAG_GRANT_READ_URI_PERMISSION | Intent.FLAG_GRANT_WRITE_URI_PERMISSION);
            Intent chooser = Intent.createChooser(intent, "Öffnen mit…");
            chooser.addFlags(Intent.FLAG_ACTIVITY_NEW_TASK);
            startActivity(chooser);
            return "";
        } catch (ActivityNotFoundException e) {
            return "Keine App zum Öffnen dieser Datei gefunden.";
        } catch (Exception e) {
            return "Öffnen fehlgeschlagen: " + e.getMessage();
        }
    }

    public void shareFiles(String newlinePaths) {
        runOnUiThread(() -> {
            try {
                String[] parts = newlinePaths.split("\n");
                ArrayList<Uri> uris = new ArrayList<>();
                String singleMime = "*/*";
                for (String p : parts) {
                    if (p == null || p.trim().isEmpty()) continue;
                    File f = new File(p.trim());
                    if (f.exists() && f.isFile()) {
                        uris.add(FileProvider.getUriForFile(this, getPackageName() + ".fileprovider", f));
                        singleMime = getMimeType(f);
                    }
                }
                if (uris.isEmpty()) return;

                Intent intent;
                if (uris.size() == 1) {
                    intent = new Intent(Intent.ACTION_SEND);
                    intent.setType(singleMime);
                    intent.putExtra(Intent.EXTRA_STREAM, uris.get(0));
                } else {
                    intent = new Intent(Intent.ACTION_SEND_MULTIPLE);
                    intent.setType("*/*");
                    intent.putParcelableArrayListExtra(Intent.EXTRA_STREAM, uris);
                }
                intent.addFlags(Intent.FLAG_GRANT_READ_URI_PERMISSION);
                startActivity(Intent.createChooser(intent, "Teilen über…"));
            } catch (Exception ignored) {
            }
        });
    }

    public void showSoftKeyboard() {
        runOnUiThread(() -> {
            View view = getWindow().getDecorView();
            view.requestFocus();
            InputMethodManager imm = (InputMethodManager) getSystemService(Context.INPUT_METHOD_SERVICE);
            if (imm != null) {
                imm.showSoftInput(view, InputMethodManager.SHOW_IMPLICIT);
            }
        });
    }

    public void hideSoftKeyboard() {
        runOnUiThread(() -> {
            View view = getWindow().getDecorView();
            InputMethodManager imm = (InputMethodManager) getSystemService(Context.INPUT_METHOD_SERVICE);
            if (imm != null) {
                imm.hideSoftInputFromWindow(view.getWindowToken(), 0);
            }
        });
    }

    /**
     * Returns newline-separated entries formatted as: name|path|removable(0/1)
     */
    public String getStorageRootsEncoded() {
        StringBuilder sb = new StringBuilder();
        Set<String> seen = new HashSet<>();

        File primary = Environment.getExternalStorageDirectory();
        if (primary != null && primary.exists()) {
            seen.add(primary.getAbsolutePath());
            sb.append("Interner Speicher|").append(primary.getAbsolutePath()).append("|0\n");
        }

        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.R) {
            try {
                StorageManager sm = (StorageManager) getSystemService(Context.STORAGE_SERVICE);
                if (sm != null) {
                    for (StorageVolume vol : sm.getStorageVolumes()) {
                        File dir = vol.getDirectory();
                        if (dir == null || !dir.exists()) continue;
                        String path = dir.getAbsolutePath();
                        if (seen.contains(path)) continue;
                        seen.add(path);
                        String desc = vol.getDescription(this);
                        if (desc == null || desc.trim().isEmpty()) {
                            desc = vol.isRemovable() ? "SD-Karte / USB" : "Speicher";
                        }
                        desc = desc.replace("|", " ").replace("\n", " ");
                        sb.append(desc).append("|").append(path).append("|")
                                .append(vol.isRemovable() ? "1" : "0").append("\n");
                    }
                }
            } catch (Exception ignored) {
            }
        }
        return sb.toString();
    }

    private static String getMimeType(File file) {
        String name = file.getName();
        int dot = name.lastIndexOf('.');
        if (dot > 0 && dot < name.length() - 1) {
            String ext = name.substring(dot + 1).toLowerCase();
            if (ext.equals("apk")) return "application/vnd.android.package-archive";
            if (ext.equals("zip")) return "application/zip";
            String mime = MimeTypeMap.getSingleton().getMimeTypeFromExtension(ext);
            if (mime != null) return mime;
        }
        return "*/*";
    }
}

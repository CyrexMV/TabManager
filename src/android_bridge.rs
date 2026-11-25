use std::path::PathBuf;

/// Platform bridge: on Android uses JNI calls into `TabManagerActivity` (a thin `NativeActivity`
/// subclass); on desktop provides sensible Linux/OS fallbacks so the app compiles and runs everywhere.
#[derive(Clone, Default)]
pub struct PlatformBridge {
    #[cfg(target_os = "android")]
    inner: Option<AndroidBridgeInner>,
}

#[cfg(target_os = "android")]
#[derive(Clone)]
struct AndroidBridgeInner {
    vm: std::sync::Arc<jni::JavaVM>,
    activity: std::sync::Arc<jni::objects::GlobalRef>,
}

impl PlatformBridge {
    #[cfg(target_os = "android")]
    pub fn from_android_app(app: &winit::platform::android::activity::AndroidApp) -> Self {
        let inner = unsafe {
            let vm_ptr = app.vm_as_ptr() as *mut jni::sys::JavaVM;
            let activity_ptr = app.activity_as_ptr() as jni::sys::jobject;
            if vm_ptr.is_null() || activity_ptr.is_null() {
                None
            } else {
                match jni::JavaVM::from_raw(vm_ptr) {
                    Ok(vm) => {
                        let global = vm
                            .attach_current_thread()
                            .ok()
                            .and_then(|env| {
                                let obj = jni::objects::JObject::from_raw(activity_ptr);
                                env.new_global_ref(obj).ok()
                            });
                        global.map(|activity| AndroidBridgeInner {
                            vm: std::sync::Arc::new(vm),
                            activity: std::sync::Arc::new(activity),
                        })
                    }
                    Err(_) => None,
                }
            }
        };
        Self { inner }
    }

    #[cfg(not(target_os = "android"))]
    pub fn desktop() -> Self {
        Self {}
    }

    pub fn has_all_files_access(&self) -> bool {
        #[cfg(target_os = "android")]
        {
            if let Some(inner) = &self.inner {
                if let Ok(mut env) = inner.vm.attach_current_thread() {
                    if let Ok(val) =
                        env.call_method(inner.activity.as_obj(), "hasAllFilesAccess", "()Z", &[])
                    {
                        if let Ok(b) = val.z() {
                            return b;
                        }
                    }
                }
            }
            false
        }
        #[cfg(not(target_os = "android"))]
        {
            true
        }
    }

    pub fn request_all_files_access(&self) {
        #[cfg(target_os = "android")]
        if let Some(inner) = &self.inner {
            if let Ok(mut env) = inner.vm.attach_current_thread() {
                let _ = env.call_method(
                    inner.activity.as_obj(),
                    "requestAllFilesAccess",
                    "()V",
                    &[],
                );
            }
        }
    }

    pub fn open_file(&self, path: &std::path::Path) -> Result<(), String> {
        let path_str = path.to_string_lossy();
        #[cfg(target_os = "android")]
        {
            let Some(inner) = &self.inner else {
                return Err("Android-Bridge nicht verfügbar.".to_string());
            };
            let mut env = inner
                .vm
                .attach_current_thread()
                .map_err(|e| format!("JNI-Fehler: {e}"))?;
            let j_path = env
                .new_string(path_str.as_ref())
                .map_err(|e| format!("JNI-String-Fehler: {e}"))?;
            let res = env
                .call_method(
                    inner.activity.as_obj(),
                    "openFileExternal",
                    "(Ljava/lang/String;)Ljava/lang/String;",
                    &[jni::objects::JValue::Object(&j_path)],
                )
                .map_err(|e| format!("Öffnen fehlgeschlagen: {e}"))?;
            if let Ok(obj) = res.l() {
                if !obj.is_null() {
                    let jstr = jni::objects::JString::from(obj);
                    if let Ok(err_str) = env.get_string(&jstr) {
                        let s: String = err_str.into();
                        if !s.is_empty() {
                            return Err(s);
                        }
                    }
                }
            }
            Ok(())
        }
        #[cfg(not(target_os = "android"))]
        {
            let _ = path_str;
            #[cfg(target_os = "linux")]
            {
                let _ = std::process::Command::new("xdg-open").arg(path).spawn();
            }
            Ok(())
        }
    }

    pub fn share_files(&self, paths: &[PathBuf]) -> Result<(), String> {
        if paths.is_empty() {
            return Err("Keine Dateien ausgewählt.".to_string());
        }
        #[cfg(target_os = "android")]
        {
            let Some(inner) = &self.inner else {
                return Err("Android-Bridge nicht verfügbar.".to_string());
            };
            let mut env = inner
                .vm
                .attach_current_thread()
                .map_err(|e| format!("JNI-Fehler: {e}"))?;
            let joined = paths
                .iter()
                .map(|p| p.to_string_lossy().into_owned())
                .collect::<Vec<_>>()
                .join("\n");
            let j_paths = env
                .new_string(&joined)
                .map_err(|e| format!("JNI-String-Fehler: {e}"))?;
            let _ = env.call_method(
                inner.activity.as_obj(),
                "shareFiles",
                "(Ljava/lang/String;)V",
                &[jni::objects::JValue::Object(&j_paths)],
            );
            Ok(())
        }
        #[cfg(not(target_os = "android"))]
        {
            let _ = paths;
            Ok(())
        }
    }

    pub fn take_back_pressed(&self) -> bool {
        #[cfg(target_os = "android")]
        {
            if let Some(inner) = &self.inner {
                if let Ok(mut env) = inner.vm.attach_current_thread() {
                    if let Ok(val) =
                        env.call_method(inner.activity.as_obj(), "takeBackPressed", "()Z", &[])
                    {
                        if let Ok(b) = val.z() {
                            return b;
                        }
                    }
                }
            }
            false
        }
        #[cfg(not(target_os = "android"))]
        {
            false
        }
    }

    pub fn move_to_background(&self) {
        #[cfg(target_os = "android")]
        if let Some(inner) = &self.inner {
            if let Ok(mut env) = inner.vm.attach_current_thread() {
                let _ = env.call_method(inner.activity.as_obj(), "moveToBackground", "()V", &[]);
            }
        }
    }

    pub fn show_keyboard(&self) {
        #[cfg(target_os = "android")]
        if let Some(inner) = &self.inner {
            if let Ok(mut env) = inner.vm.attach_current_thread() {
                let _ = env.call_method(inner.activity.as_obj(), "showSoftKeyboard", "()V", &[]);
            }
        }
    }

    pub fn hide_keyboard(&self) {
        #[cfg(target_os = "android")]
        if let Some(inner) = &self.inner {
            if let Ok(mut env) = inner.vm.attach_current_thread() {
                let _ = env.call_method(inner.activity.as_obj(), "hideSoftKeyboard", "()V", &[]);
            }
        }
    }

    pub fn get_storage_roots(&self) -> Vec<(String, PathBuf, bool)> {
        #[cfg(target_os = "android")]
        {
            let mut roots = Vec::new();
            if let Some(inner) = &self.inner {
                if let Ok(mut env) = inner.vm.attach_current_thread() {
                    if let Ok(val) = env.call_method(
                        inner.activity.as_obj(),
                        "getStorageRootsEncoded",
                        "()Ljava/lang/String;",
                        &[],
                    ) {
                        if let Ok(obj) = val.l() {
                            if !obj.is_null() {
                                let jstr = jni::objects::JString::from(obj);
                                if let Ok(s) = env.get_string(&jstr) {
                                    let encoded: String = s.into();
                                    for line in encoded.lines() {
                                        let parts: Vec<&str> = line.splitn(3, '|').collect();
                                        if parts.len() == 3 {
                                            roots.push((
                                                parts[0].to_string(),
                                                PathBuf::from(parts[1]),
                                                parts[2] == "1",
                                            ));
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
            }
            if roots.is_empty() {
                roots.push((
                    "Interner Speicher".to_string(),
                    PathBuf::from("/storage/emulated/0"),
                    false,
                ));
            }
            roots
        }
        #[cfg(not(target_os = "android"))]
        {
            let home = std::env::var("HOME")
                .map(PathBuf::from)
                .unwrap_or_else(|_| PathBuf::from("/"));
            vec![("Home".to_string(), home, false)]
        }
    }

    pub fn system_insets_px(&self) -> (f32, f32, f32, f32) {
        #[cfg(target_os = "android")]
        {
            if let Some(inner) = &self.inner {
                if let Ok(mut env) = inner.vm.attach_current_thread() {
                    if let Ok(val) = env.call_method(
                        inner.activity.as_obj(),
                        "getSystemInsetsEncoded",
                        "()Ljava/lang/String;",
                        &[],
                    ) {
                        if let Ok(obj) = val.l() {
                            if !obj.is_null() {
                                let jstr = jni::objects::JString::from(obj);
                                if let Ok(s) = env.get_string(&jstr) {
                                    let encoded: String = s.into();
                                    let parts: Vec<f32> = encoded
                                        .split(',')
                                        .filter_map(|p| p.trim().parse::<f32>().ok())
                                        .collect();
                                    if parts.len() == 4 {
                                        return (parts[0], parts[1], parts[2], parts[3]);
                                    }
                                }
                            }
                        }
                    }
                }
            }
            (0.0, 0.0, 0.0, 0.0)
        }
        #[cfg(not(target_os = "android"))]
        {
            (0.0, 0.0, 0.0, 0.0)
        }
    }

    pub fn primary_storage_root(&self) -> PathBuf {
        self.get_storage_roots()
            .into_iter()
            .next()
            .map(|(_, p, _)| p)
            .unwrap_or_else(|| PathBuf::from("/storage/emulated/0"))
    }
}

// SPDX-FileCopyrightText: 2022  Emmanuele Bassi
// SPDX-License-Identifier: GPL-3.0-or-later

mod application;
mod audio;
mod archive_view;
mod circular_progress;
mod config;
mod vinyl_view;
mod cover_picture;
mod drag_overlay;
mod i18n;
mod lyrics_view;
mod playback_control;
mod playlist_view;
mod queue_row;
mod search;
mod song_cover;
mod song_details;
mod sort;
mod utils;
mod window;

use std::env;

use config::{APPLICATION_ID, GETTEXT_PACKAGE, LOCALEDIR, PKGDATADIR, PROFILE};
use gettextrs::{bind_textdomain_codeset, bindtextdomain, setlocale, textdomain, LocaleCategory};
use gtk::{gio, glib, prelude::*};
use log::{debug, error, LevelFilter};

use self::application::Application;

fn main() -> glib::ExitCode {
    let mut builder = pretty_env_logger::formatted_builder();
    if APPLICATION_ID.ends_with("Devel") {
        builder.filter(Some("amberol"), LevelFilter::Debug);
    } else {
        builder.filter(Some("amberol"), LevelFilter::Info);
    }
    builder.init();

    #[cfg(target_os = "windows")]
    {
        // Suppress harmless win32 dbus warning on Windows
        glib::log_set_handler(
            Some("GLib-GIO"),
            glib::LogLevels::LEVEL_WARNING,
            false,
            false,
            |domain, level, message: &str| {
                if message.contains("win32 session dbus") {
                    return;
                }
                glib::log_default_handler(domain, level, Some(message));
            },
        );

        // Load bundled custom fonts (Audiowide, Inter) for Pango, Cairo, and DirectWrite
        setup_windows_fonts();

        // Default to cairo renderer on Windows to avoid crashing on legacy GPU drivers (e.g. AMD Radeon R5)
        if env::var_os("GSK_RENDERER").is_none() {
            env::set_var("GSK_RENDERER", "cairo");
        }

        // Ensure executable directory is at the front of PATH for GStreamer plugin dependency resolution
        if let Ok(exe_path) = env::current_exe() {
            if let Some(exe_dir) = exe_path.parent() {
                if let Some(existing) = env::var_os("PATH") {
                    let mut paths = env::split_paths(&existing).collect::<Vec<_>>();
                    paths.insert(0, exe_dir.to_path_buf());
                    if let Ok(new_path) = env::join_paths(paths) {
                        env::set_var("PATH", new_path);
                    }
                }
            }
        }
    }

    // Auto-detect GSETTINGS_SCHEMA_DIR if not set (especially critical on Windows)
    if env::var("GSETTINGS_SCHEMA_DIR").is_err() {
        if let Ok(exe_path) = env::current_exe() {
            if let Some(exe_dir) = exe_path.parent() {
                if exe_dir.join("gschemas.compiled").exists() {
                    env::set_var("GSETTINGS_SCHEMA_DIR", exe_dir);
                } else if exe_dir.join("share").join("glib-2.0").join("schemas").join("gschemas.compiled").exists() {
                    env::set_var("GSETTINGS_SCHEMA_DIR", exe_dir.join("share").join("glib-2.0").join("schemas"));
                } else if exe_dir.join("data").join("gschemas.compiled").exists() {
                    env::set_var("GSETTINGS_SCHEMA_DIR", exe_dir.join("data"));
                } else if std::path::Path::new("data/gschemas.compiled").exists() {
                    env::set_var("GSETTINGS_SCHEMA_DIR", "data");
                }
            }
        }
    }

    #[cfg(target_os = "windows")]
    {
        // Auto-detect GStreamer plugins directory if bundled next to executable
        if env::var("GST_PLUGIN_SYSTEM_PATH_1_0").is_err() && env::var("GST_PLUGIN_PATH_1_0").is_err() {
            if let Ok(exe_path) = env::current_exe() {
                if let Some(exe_dir) = exe_path.parent() {
                    let gst_lib_path = exe_dir.join("lib").join("gstreamer-1.0");
                    if gst_lib_path.exists() {
                        env::set_var("GST_PLUGIN_SYSTEM_PATH_1_0", gst_lib_path);
                    }
                }
            }
        }
    }

    // Set up gettext translations
    debug!("Setting up locale data");
    setlocale(LocaleCategory::LcAll, "");

    #[cfg(not(target_os = "windows"))]
    {
        let _ = bindtextdomain(GETTEXT_PACKAGE, LOCALEDIR);
        let _ = bind_textdomain_codeset(GETTEXT_PACKAGE, "UTF-8");
        let _ = textdomain(GETTEXT_PACKAGE);
    }
    #[cfg(target_os = "windows")]
    {
        let _ = bindtextdomain(GETTEXT_PACKAGE, LOCALEDIR);
        let _ = bind_textdomain_codeset(GETTEXT_PACKAGE, "UTF-8");
        let _ = textdomain(GETTEXT_PACKAGE);
    }

    #[cfg(not(target_os = "windows"))]
    {
        debug!("Setting up pulseaudio environment");
        let app_id = APPLICATION_ID.trim_end_matches(".Devel");
        env::set_var("PULSE_PROP_application.icon_name", app_id);
        env::set_var("PULSE_PROP_application.metadata().name", "Amberol");
        env::set_var("PULSE_PROP_media.role", "music");
    }

    debug!("Loading resources");
    let resources = match env::current_exe() {
        Ok(path) => {
            let mut resource_path = path.clone();
            resource_path.pop();
            resource_path.push("amberol.gresource");
            if resource_path.exists() {
                gio::Resource::load(&resource_path)
                    .expect("Unable to find amberol.gresource in build dir")
            } else if let Some(parent) = path.parent() {
                let share_path = parent.join("share").join("amberol").join("amberol.gresource");
                let data_path = parent.join("data").join("amberol.gresource");
                let cwd_path = std::path::Path::new("data/amberol.gresource");
                if share_path.exists() {
                    gio::Resource::load(&share_path).expect("Unable to find amberol.gresource in share dir")
                } else if data_path.exists() {
                    gio::Resource::load(&data_path).expect("Unable to find amberol.gresource in data dir")
                } else if cwd_path.exists() {
                    gio::Resource::load(cwd_path).expect("Unable to find amberol.gresource in current dir")
                } else {
                    gio::Resource::load(PKGDATADIR.to_owned() + "/amberol.gresource")
                        .expect("Unable to find amberol.gresource")
                }
            } else {
                gio::Resource::load(PKGDATADIR.to_owned() + "/amberol.gresource")
                    .expect("Unable to find amberol.gresource")
            }
        }
        Err(err) => {
            error!("Unable to find the current path: {}", err);
            gio::Resource::load(PKGDATADIR.to_owned() + "/amberol.gresource")
                .expect("Unable to find amberol.gresource")
        }
    };
    gio::resources_register(&resources);

    debug!("Setting up application (profile: {})", &PROFILE);
    glib::set_application_name("Amberol");
    glib::set_program_name(Some("amberol"));

    gst::init().expect("Failed to initialize gstreamer");

    let ctx = glib::MainContext::default();
    let _guard = ctx.acquire().unwrap();

    Application::new().run()
}

#[cfg(target_os = "windows")]
fn setup_windows_fonts() {
    use std::os::windows::ffi::OsStrExt;

    extern "system" {
        fn AddFontResourceExW(
            name: *const u16,
            fl: u32,
            res: *mut std::ffi::c_void,
        ) -> i32;
        fn AddFontResourceW(name: *const u16) -> i32;
        fn LoadLibraryA(lpLibFileName: *const u8) -> *mut std::ffi::c_void;
        fn GetProcAddress(
            hModule: *mut std::ffi::c_void,
            lpProcName: *const u8,
        ) -> *mut std::ffi::c_void;
    }

    type FcBool = i32;
    type FcConfig = std::ffi::c_void;
    type FcChar8 = u8;

    let font_candidates = [
        ("Audiowide-Regular.ttf", "Audiowide (TrueType)"),
        ("Inter.ttf", "Inter (TrueType)"),
    ];

    let exe_dir = env::current_exe().ok().and_then(|p| p.parent().map(|p| p.to_path_buf()));
    let local_app_data = env::var_os("LOCALAPPDATA").map(std::path::PathBuf::from);
    let user_fonts_dir = local_app_data.map(|p| p.join("Microsoft").join("Windows").join("Fonts"));

    if let Some(ref dir) = user_fonts_dir {
        let _ = std::fs::create_dir_all(dir);

        // Ensure a valid fonts.conf exists to eliminate Fontconfig stderr warnings
        if env::var_os("FONTCONFIG_FILE").is_none() {
            let conf_path = if let Some(ref edir) = exe_dir {
                let p = edir.join("fonts").join("fonts.conf");
                if p.exists() { p } else { dir.join("fonts.conf") }
            } else {
                dir.join("fonts.conf")
            };
            if !conf_path.exists() {
                let conf_content = "<?xml version=\"1.0\"?>\n<!DOCTYPE fontconfig SYSTEM \"urn:fontconfig:fonts.dtd\">\n<fontconfig>\n  <dir>WINDOWSFONTDIR</dir>\n</fontconfig>\n";
                let _ = std::fs::write(&conf_path, conf_content);
            }
            if conf_path.exists() {
                env::set_var("FONTCONFIG_FILE", &conf_path);

                // Also set in UCRT environment table so libfontconfig reads it
                let ucrt = unsafe { LoadLibraryA(b"ucrtbase.dll\0".as_ptr()) };
                if !ucrt.is_null() {
                    let putenv_proc = unsafe { GetProcAddress(ucrt, b"_wputenv\0".as_ptr()) };
                    if !putenv_proc.is_null() {
                        let putenv_fn: unsafe extern "C" fn(*const u16) -> i32 = unsafe { std::mem::transmute(putenv_proc) };
                        let wide_env: Vec<u16> = format!("FONTCONFIG_FILE={}\0", conf_path.display()).encode_utf16().collect();
                        unsafe { let _ = putenv_fn(wide_env.as_ptr()); }
                    }
                }
            }
        }
    }

    let fc_dll = unsafe { LoadLibraryA(b"libfontconfig-1.dll\0".as_ptr()) };
    let fc_add_file: Option<unsafe extern "C" fn(*mut FcConfig, *const FcChar8) -> FcBool> = if !fc_dll.is_null() {
        let proc = unsafe { GetProcAddress(fc_dll, b"FcConfigAppFontAddFile\0".as_ptr()) };
        if !proc.is_null() {
            Some(unsafe { std::mem::transmute(proc) })
        } else {
            None
        }
    } else {
        None
    };

    // Load advapi32 for HKCU font registry
    type HKEY = *mut std::ffi::c_void;
    const HKEY_CURRENT_USER: HKEY = 0x80000001usize as HKEY;
    const KEY_SET_VALUE: u32 = 0x0002;
    const REG_SZ: u32 = 1;

    let advapi = unsafe { LoadLibraryA(b"advapi32.dll\0".as_ptr()) };
    let (reg_open, reg_set, reg_close) = if !advapi.is_null() {
        let ro = unsafe { GetProcAddress(advapi, b"RegOpenKeyExW\0".as_ptr()) };
        let rs = unsafe { GetProcAddress(advapi, b"RegSetValueExW\0".as_ptr()) };
        let rc = unsafe { GetProcAddress(advapi, b"RegCloseKey\0".as_ptr()) };
        (ro, rs, rc)
    } else {
        (std::ptr::null_mut(), std::ptr::null_mut(), std::ptr::null_mut())
    };

    for &(font_name, font_reg_title) in &font_candidates {
        let mut source_path = None;
        if let Some(ref dir) = exe_dir {
            let p = dir.join("fonts").join(font_name);
            if p.exists() {
                source_path = Some(p);
            }
        }
        if source_path.is_none() {
            let p = std::path::Path::new("src/assets/fonts").join(font_name);
            if p.exists() {
                source_path = Some(p);
            }
        }
        if source_path.is_none() {
            let p = std::path::Path::new("fonts").join(font_name);
            if p.exists() {
                source_path = Some(p);
            }
        }

        if let Some(src) = source_path {
            // Copy to user fonts directory if not already there
            let dest_path = if let Some(ref u_dir) = user_fonts_dir {
                let target = u_dir.join(font_name);
                if !target.exists() {
                    let _ = std::fs::copy(&src, &target);
                }
                if target.exists() {
                    target
                } else {
                    src.clone()
                }
            } else {
                src.clone()
            };

            // 1. Add to GDI / DirectWrite process font table
            let wide: Vec<u16> = dest_path.as_os_str().encode_wide().chain(Some(0)).collect();
            const FR_PRIVATE: u32 = 0x10;
            unsafe {
                let _ = AddFontResourceExW(wide.as_ptr(), FR_PRIVATE, std::ptr::null_mut());
                let _ = AddFontResourceW(wide.as_ptr());
            }

            // 2. Register in HKCU Registry so Windows font enumeration always finds it
            if !reg_open.is_null() && !reg_set.is_null() && !reg_close.is_null() {
                let open_fn: unsafe extern "system" fn(HKEY, *const u16, u32, u32, *mut HKEY) -> i32 = unsafe { std::mem::transmute(reg_open) };
                let set_fn: unsafe extern "system" fn(HKEY, *const u16, u32, u32, *const u8, u32) -> i32 = unsafe { std::mem::transmute(reg_set) };
                let close_fn: unsafe extern "system" fn(HKEY) -> i32 = unsafe { std::mem::transmute(reg_close) };

                let subkey: Vec<u16> = "Software\\Microsoft\\Windows NT\\CurrentVersion\\Fonts\0".encode_utf16().collect();
                let mut hkey: HKEY = std::ptr::null_mut();
                if unsafe { open_fn(HKEY_CURRENT_USER, subkey.as_ptr(), 0, KEY_SET_VALUE, &mut hkey) } == 0 {
                    let val_name: Vec<u16> = font_reg_title.encode_utf16().chain(Some(0)).collect();
                    let val_bytes: &[u8] = unsafe {
                        std::slice::from_raw_parts(wide.as_ptr() as *const u8, wide.len() * 2)
                    };
                    unsafe {
                        let _ = set_fn(hkey, val_name.as_ptr(), 0, REG_SZ, val_bytes.as_ptr(), val_bytes.len() as u32);
                        let _ = close_fn(hkey);
                    }
                }
            }

            // 3. Add to Fontconfig
            if let Some(add_file) = fc_add_file {
                let path_str = dest_path.to_string_lossy().replace('\\', "/");
                if let Ok(c_path) = std::ffi::CString::new(path_str.as_bytes()) {
                    unsafe {
                        let _ = add_file(std::ptr::null_mut(), c_path.as_ptr() as *const FcChar8);
                    }
                }
            }
        }
    }

    // 4. Notify running windows of font changes
    let user32 = unsafe { LoadLibraryA(b"user32.dll\0".as_ptr()) };
    if !user32.is_null() {
        let post_msg = unsafe { GetProcAddress(user32, b"PostMessageW\0".as_ptr()) };
        if !post_msg.is_null() {
            let post_fn: unsafe extern "system" fn(*mut std::ffi::c_void, u32, usize, usize) -> i32 = unsafe { std::mem::transmute(post_msg) };
            const HWND_BROADCAST: *mut std::ffi::c_void = 0xffffusize as *mut std::ffi::c_void;
            const WM_FONTCHANGE: u32 = 0x001D;
            unsafe {
                let _ = post_fn(HWND_BROADCAST, WM_FONTCHANGE, 0, 0);
            }
        }
    }
}

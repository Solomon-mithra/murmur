pub mod audio;
pub mod cleanup;
pub mod llm;
pub mod stt;
pub mod tap;

use std::sync::mpsc;
use std::time::{Duration, Instant};
use tap::Action;
use tauri::menu::{MenuBuilder, MenuItemBuilder};
use tauri::path::BaseDirectory;
use tauri::tray::TrayIconBuilder;
use tauri::{ActivationPolicy, AppHandle, Emitter, Manager, PhysicalPosition};

fn ui(app: &AppHandle, state: &str, text: &str) {
    if matches!(state, "listening" | "polishing") {
        place_pill(app); // follow Dock resizes / display changes
    }
    let _ = app.emit("state", (state, text));
}

/// Put `text` into the focused field via the clipboard, then restore the user's clipboard.
fn paste(clip: &mut arboard::Clipboard, text: &str, saved: Option<String>) {
    let _ = clip.set_text(text);
    std::thread::sleep(Duration::from_millis(40));
    tap::cmd(tap::KEY_V);
    std::thread::sleep(Duration::from_millis(250)); // let the target app read it
    if let Some(s) = saved {
        let _ = clip.set_text(s);
    }
}

/// Select-all + copy in the focused app, returning what was copied.
fn grab_field(clip: &mut arboard::Clipboard) -> String {
    let _ = clip.clear();
    tap::cmd(tap::KEY_A);
    std::thread::sleep(Duration::from_millis(30));
    tap::cmd(tap::KEY_C);
    let t = Instant::now();
    while t.elapsed() < Duration::from_millis(600) {
        if let Ok(s) = clip.get_text() {
            if !s.is_empty() {
                return s;
            }
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    String::new()
}

fn worker(app: AppHandle, rx: mpsc::Receiver<Action>) {
    let dir = app.path().resolve("resources", BaseDirectory::Resource).unwrap();
    ui(&app, "loading", "");
    if let Err(e) = stt::load(&dir.join("whistle.cact")) {
        return ui(&app, "error", &e.to_string());
    }
    let llm = match llm::Rephraser::load(&dir) {
        Ok(m) => m,
        Err(e) => return ui(&app, "error", &e.to_string()),
    };
    let mut clip = arboard::Clipboard::new().expect("clipboard");
    ui(&app, "idle", "");

    let mut rec: Option<(audio::Recording, Instant)> = None;
    while let Ok(action) = rx.recv() {
        match (action, rec.take()) {
            (Action::DictateStart, None) => {
                let a = app.clone();
                match audio::start(move |lvl| {
                    let _ = a.emit("level", lvl);
                }) {
                    Ok(r) => {
                        rec = Some((r, Instant::now()));
                        ui(&app, "listening", "");
                    }
                    Err(e) => ui(&app, "error", &e.to_string()),
                }
                continue;
            }
            (Action::DictateCancel, Some(_)) => ui(&app, "idle", ""),
            // A quick brush of the keys isn't a dictation.
            (Action::DictateStop, Some((_, t))) if t.elapsed() < Duration::from_millis(350) => ui(&app, "idle", ""),
            (Action::DictateStop, Some((r, _))) => {
                let pcm = r.finish();
                if audio::voiced_ms(&pcm) < 250 {
                    ui(&app, "error", "didn't hear anything");
                    continue;
                }
                ui(&app, "thinking", "");
                match stt::transcribe(&pcm).map(|t| cleanup::tidy(&t)) {
                    Ok(t) if !t.is_empty() => {
                        let saved = clip.get_text().ok();
                        paste(&mut clip, &t, saved);
                        ui(&app, "done", &t);
                    }
                    Ok(_) => ui(&app, "error", "didn't catch that"),
                    Err(e) => ui(&app, "error", &e.to_string()),
                }
            }
            (Action::Polish, None) => {
                ui(&app, "polishing", "");
                std::thread::sleep(Duration::from_millis(60));
                let saved = clip.get_text().ok();
                let original = grab_field(&mut clip);
                let result = if original.trim().is_empty() {
                    Err(anyhow::anyhow!("nothing to polish"))
                } else {
                    llm.rephrase(&original)
                };
                match result {
                    Ok(t) if t == original => {
                        saved.map(|s| clip.set_text(s));
                        ui(&app, "done", "already looks good");
                    }
                    Ok(t) => {
                        paste(&mut clip, &t, saved);
                        ui(&app, "done", &t);
                    }
                    Err(e) => {
                        saved.map(|s| clip.set_text(s));
                        ui(&app, "error", &e.to_string());
                    }
                }
            }
            (_, r) => {
                rec = r; // stray event for the current mode
                continue;
            }
        }
        // Chords pressed while we were busy are stale.
        while rx.try_recv().is_ok() {}
    }
}

/// Bottom-center of the primary display, resting on top of the Dock
/// (work_area excludes the Dock / Windows taskbar). The 10 px gap is the
/// pill's CSS `bottom`, so the window itself sits flush on the Dock's top edge.
fn place_pill(app: &AppHandle) {
    let Some(w) = app.get_webview_window("pill") else { return };
    if let Ok(Some(m)) = w.primary_monitor() {
        let (area, size) = (m.work_area(), w.outer_size().unwrap_or_default());
        let x = area.position.x + (area.size.width as i32 - size.width as i32) / 2;
        let y = area.position.y + area.size.height as i32 - size.height as i32;
        let _ = w.set_position(PhysicalPosition::new(x, y));
    }
}

/// Above every app, including full-screen ones.
///
/// Full-screen apps live in their own Space, and only a non-activating NSPanel
/// may join another app's full-screen Space without stealing focus. Tauri gives
/// us an NSWindow subclass, so we swap its class to an NSPanel subclass.
#[cfg(target_os = "macos")]
fn float_above_everything(w: &tauri::WebviewWindow) {
    use objc2::runtime::{AnyClass, AnyObject, Bool, ClassBuilder, Sel};
    use objc2::{msg_send, sel};

    extern "C-unwind" fn no(_: &AnyObject, _: Sel) -> Bool {
        Bool::NO
    }
    const NON_ACTIVATING_PANEL: usize = 1 << 7;
    const POP_UP_MENU_LEVEL: isize = 101;
    // canJoinAllSpaces | stationary | ignoresCycle | fullScreenAuxiliary
    const BEHAVIOR: usize = 1 << 0 | 1 << 4 | 1 << 6 | 1 << 8;

    let Ok(ns) = w.ns_window() else { return };
    let ns = unsafe { &*(ns as *mut AnyObject) };
    let (panel, window) = (AnyClass::get(c"NSPanel").unwrap(), AnyClass::get(c"NSWindow").unwrap());
    // Swapping is only memory-safe if NSPanel adds no storage over NSWindow.
    if panel.instance_size() == window.instance_size() {
        let cls = AnyClass::get(c"MurmurPanel").unwrap_or_else(|| {
            let mut b = ClassBuilder::new(c"MurmurPanel", panel).unwrap();
            b.add_ivar::<Bool>(c"focusable"); // mirrors tao's TaoWindow layout
            unsafe {
                b.add_method(sel!(canBecomeKeyWindow), no as extern "C-unwind" fn(_, _) -> _);
                b.add_method(sel!(canBecomeMainWindow), no as extern "C-unwind" fn(_, _) -> _);
            }
            b.register()
        });
        unsafe {
            AnyObject::set_class(ns, cls);
            let mask: usize = msg_send![ns, styleMask];
            let _: () = msg_send![ns, setStyleMask: mask | NON_ACTIVATING_PANEL];
            let _: () = msg_send![ns, setFloatingPanel: true];
            let _: () = msg_send![ns, setHidesOnDeactivate: false];
        }
    } else {
        eprintln!("murmur: NSPanel layout differs; overlay won't cover full-screen apps");
    }
    unsafe {
        let _: () = msg_send![ns, setLevel: POP_UP_MENU_LEVEL];
        let _: () = msg_send![ns, setCollectionBehavior: BEHAVIOR];
    }
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .setup(|app| {
            app.set_activation_policy(ActivationPolicy::Accessory);
            place_pill(app.handle());
            if let Some(w) = app.get_webview_window("pill") {
                #[cfg(target_os = "macos")]
                float_above_everything(&w);
                let _ = w.set_ignore_cursor_events(true);
                let _ = w.show();
            }

            let quit = MenuItemBuilder::with_id("quit", "Quit Murmur").build(app)?;
            let hint = MenuItemBuilder::new("Hold ⌃⇧ to dictate  ·  ⌃⌥ to rephrase").enabled(false).build(app)?;
            let menu = MenuBuilder::new(app).items(&[&hint, &quit]).build()?;
            TrayIconBuilder::new()
                .icon(app.default_window_icon().unwrap().clone())
                .menu(&menu)
                .on_menu_event(|app, e| {
                    if e.id() == "quit" {
                        app.exit(0)
                    }
                })
                .build(app)?;

            let (tx, rx) = mpsc::channel();
            let h = app.handle().clone();
            std::thread::spawn(move || worker(h, rx));

            let h = app.handle().clone();
            tap::request_access();
            std::thread::spawn(move || loop {
                let tx = tx.clone();
                // Needs Accessibility permission; keep retrying until the user grants it.
                let ready = h.clone();
                if tap::run(
                    move || ui(&ready, "idle", ""), // clears the "grant access" message
                    move |a| {
                        let _ = tx.send(a);
                    },
                )
                .is_err()
                {
                    if !tap::trusted() {
                        ui(&h, "access", "");
                    }
                    std::thread::sleep(Duration::from_secs(2));
                }
            });
            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}

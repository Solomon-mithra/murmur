//! Global modifier chords via a CGEventTap, plus synthetic keystrokes.
//!
//! Hold ⌃⇧  -> dictate while held, paste on release.
//! Tap  ⌃⌥  -> rephrase the focused field on release.
//! Pressing any other key during a chord means it was a shortcut
//! (⌃⇧Tab, ⌃⌥←, ...), so the chord cancels. Nothing is ever swallowed.
use core_foundation::base::TCFType;
use core_foundation::runloop::{kCFRunLoopCommonModes, CFRunLoop};
use core_graphics::event::{
    CGEvent, CGEventFlags, CGEventTap, CGEventTapLocation, CGEventTapOptions, CGEventTapPlacement, CGEventType,
    CallbackResult, EventField,
};
use core_graphics::event_source::{CGEventSource, CGEventSourceStateID};
use std::sync::atomic::{AtomicPtr, Ordering};
use std::sync::Mutex;

pub const KEY_A: u16 = 0;
pub const KEY_C: u16 = 8;
pub const KEY_V: u16 = 9;
const TAG: i64 = 0x6d75726d; // "murm": marks our own synthetic events

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Action {
    DictateStart,
    DictateStop,
    DictateCancel,
    Polish,
}

#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct Mods {
    pub ctrl: bool,
    pub shift: bool,
    pub opt: bool,
    pub cmd: bool,
}

impl Mods {
    fn none(self) -> bool {
        !(self.ctrl || self.shift || self.opt || self.cmd)
    }
    fn dictate(self) -> bool {
        self.ctrl && self.shift && !self.opt && !self.cmd
    }
    fn polish(self) -> bool {
        self.ctrl && self.opt && !self.shift && !self.cmd
    }
}

#[derive(Debug, Default, Clone, Copy, PartialEq)]
enum Mode {
    #[default]
    Idle,
    Dictating,
    PolishArmed,
    Spent, // chord finished or cancelled; wait for all modifiers up
}

#[derive(Default)]
pub struct Chords {
    mode: Mode,
}

impl Chords {
    pub fn flags(&mut self, m: Mods) -> Option<Action> {
        let (mode, action) = match self.mode {
            Mode::Idle if m.dictate() => (Mode::Dictating, Some(Action::DictateStart)),
            Mode::Idle if m.polish() => (Mode::PolishArmed, None),
            Mode::Dictating if m.dictate() => (Mode::Dictating, None),
            Mode::Dictating if m.opt || m.cmd => (Mode::Spent, Some(Action::DictateCancel)),
            Mode::Dictating => (Mode::Spent, Some(Action::DictateStop)),
            Mode::PolishArmed if m.polish() => (Mode::PolishArmed, None),
            Mode::PolishArmed if m.shift || m.cmd => (Mode::Spent, None),
            Mode::PolishArmed => (Mode::Spent, Some(Action::Polish)),
            Mode::Spent if m.none() => (Mode::Idle, None),
            mode => (mode, None),
        };
        self.mode = mode;
        action
    }

    /// Any non-modifier key went down.
    pub fn key(&mut self) -> Option<Action> {
        let was = std::mem::replace(&mut self.mode, Mode::Spent);
        match was {
            Mode::Dictating => Some(Action::DictateCancel),
            Mode::Idle => {
                self.mode = Mode::Idle;
                None
            }
            _ => None,
        }
    }
}

fn key_event(code: u16, down: bool, flags: CGEventFlags) -> Option<CGEvent> {
    let src = CGEventSource::new(CGEventSourceStateID::HIDSystemState).ok()?;
    let e = CGEvent::new_keyboard_event(src, code, down).ok()?;
    e.set_flags(flags);
    e.set_integer_value_field(EventField::EVENT_SOURCE_USER_DATA, TAG);
    Some(e)
}

/// Press Cmd+<key>.
pub fn cmd(code: u16) {
    for down in [true, false] {
        if let Some(e) = key_event(code, down, CGEventFlags::CGEventFlagCommand) {
            e.post(CGEventTapLocation::HID);
        }
        std::thread::sleep(std::time::Duration::from_millis(8));
    }
}

extern "C" {
    fn CGEventTapEnable(tap: *mut std::ffi::c_void, enable: bool);
    fn AXIsProcessTrusted() -> bool;
    fn AXIsProcessTrustedWithOptions(options: core_foundation::dictionary::CFDictionaryRef) -> bool;
}

static TAP_PORT: AtomicPtr<std::ffi::c_void> = AtomicPtr::new(std::ptr::null_mut());

pub fn trusted() -> bool {
    unsafe { AXIsProcessTrusted() }
}

/// Shows macOS's own "allow accessibility" dialog, which adds Murmur to the
/// list and links straight to the right Settings page on any macOS version.
pub fn request_access() -> bool {
    use core_foundation::{boolean::CFBoolean, dictionary::CFDictionary, string::CFString};
    let opts = CFDictionary::from_CFType_pairs(&[(CFString::new("AXTrustedCheckOptionPrompt"), CFBoolean::true_value())]);
    unsafe { AXIsProcessTrustedWithOptions(opts.as_concrete_TypeRef()) }
}

/// Installs the tap on the current thread and runs its run loop forever.
/// Returns Err if Accessibility permission is missing.
pub fn run(on_ready: impl FnOnce(), on_action: impl Fn(Action) + Send + 'static) -> Result<(), ()> {
    let state = Mutex::new(Chords::default());
    let tap = CGEventTap::new(
        CGEventTapLocation::HID,
        CGEventTapPlacement::HeadInsertEventTap,
        CGEventTapOptions::Default,
        vec![CGEventType::KeyDown, CGEventType::FlagsChanged],
        move |_, ty, ev| {
            if matches!(ty, CGEventType::TapDisabledByTimeout | CGEventType::TapDisabledByUserInput) {
                unsafe { CGEventTapEnable(TAP_PORT.load(Ordering::Relaxed), true) };
                return CallbackResult::Keep;
            }
            if ev.get_integer_value_field(EventField::EVENT_SOURCE_USER_DATA) == TAG {
                return CallbackResult::Keep;
            }
            let mut s = state.lock().unwrap();
            let action = if matches!(ty, CGEventType::FlagsChanged) {
                let f = ev.get_flags();
                s.flags(Mods {
                    ctrl: f.contains(CGEventFlags::CGEventFlagControl),
                    shift: f.contains(CGEventFlags::CGEventFlagShift),
                    opt: f.contains(CGEventFlags::CGEventFlagAlternate),
                    cmd: f.contains(CGEventFlags::CGEventFlagCommand),
                })
            } else {
                s.key()
            };
            if let Some(a) = action {
                on_action(a);
            }
            CallbackResult::Keep
        },
    )?;
    TAP_PORT.store(tap.mach_port().as_concrete_TypeRef() as *mut _, Ordering::Relaxed);
    let src = tap.mach_port().create_runloop_source(0).map_err(|_| ())?;
    CFRunLoop::get_current().add_source(&src, unsafe { kCFRunLoopCommonModes });
    tap.enable();
    on_ready();
    CFRunLoop::run_current();
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    const NONE: Mods = Mods { ctrl: false, shift: false, opt: false, cmd: false };
    const CTRL: Mods = Mods { ctrl: true, ..NONE };
    const CTRL_SHIFT: Mods = Mods { ctrl: true, shift: true, ..NONE };
    const CTRL_OPT: Mods = Mods { ctrl: true, opt: true, ..NONE };

    #[test]
    fn hold_ctrl_shift_dictates_until_release() {
        let mut c = Chords::default();
        assert_eq!(c.flags(CTRL), None);
        assert_eq!(c.flags(CTRL_SHIFT), Some(Action::DictateStart));
        assert_eq!(c.flags(CTRL), Some(Action::DictateStop));
        assert_eq!(c.flags(CTRL_OPT), None); // still spent until all mods up
        assert_eq!(c.flags(NONE), None);
        assert_eq!(c.flags(CTRL_SHIFT), Some(Action::DictateStart));
    }

    #[test]
    fn shortcut_key_cancels_dictation() {
        let mut c = Chords::default();
        c.flags(CTRL_SHIFT);
        assert_eq!(c.key(), Some(Action::DictateCancel)); // ⌃⇧Tab
        assert_eq!(c.flags(CTRL), None);
    }

    #[test]
    fn ctrl_opt_polishes_on_release_only_if_clean() {
        let mut c = Chords::default();
        assert_eq!(c.flags(CTRL_OPT), None);
        assert_eq!(c.flags(CTRL), Some(Action::Polish));
        c.flags(NONE);

        c.flags(CTRL_OPT);
        assert_eq!(c.key(), None); // ⌃⌥← : a shortcut, not a chord
        assert_eq!(c.flags(NONE), None);
    }

    #[test]
    fn plain_typing_untouched() {
        let mut c = Chords::default();
        assert_eq!(c.key(), None);
        assert_eq!(c.flags(CTRL_SHIFT), Some(Action::DictateStart));
    }
}

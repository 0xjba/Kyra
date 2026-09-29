//! Frosted window backdrop matching the design's `blur(44px) saturate(1.35)`.
//! Uses NSVisualEffectView for the behind-window sampling, then strips the system
//! tint layers so the CSS on #root owns the tint instead of macOS's gray material.

use std::sync::atomic::{AtomicUsize, Ordering};

use objc2::msg_send;
use objc2::runtime::{AnyClass, AnyObject, Bool};
use objc2_foundation::{NSRect, NSString};

const BLUR_RADIUS: f64 = 44.0;
const SATURATION: f64 = 1.35;
const CORNER_RADIUS: f64 = 26.0;

static EFFECT_VIEW: AtomicUsize = AtomicUsize::new(0);
static TRAFFIC_LIGHTS: std::sync::Mutex<Option<TrafficLights>> = std::sync::Mutex::new(None);

/// Visible geometry of the native window buttons, in window points.
#[derive(Clone, Copy, serde::Serialize)]
pub struct TrafficLights {
    /// X just past the zoom (green) dot's right edge.
    pub right: f64,
    /// Dot diameter.
    pub dot: f64,
    /// Space between two adjacent dots.
    pub gap: f64,
}

/// Must run on the main thread after the traffic-light inset has been applied.
pub unsafe fn measure_traffic_lights(ns_win: *mut AnyObject) {
    let close: *mut AnyObject = msg_send![&*ns_win, standardWindowButton: 0usize];
    let mini: *mut AnyObject = msg_send![&*ns_win, standardWindowButton: 1usize];
    let zoom: *mut AnyObject = msg_send![&*ns_win, standardWindowButton: 2usize];
    if close.is_null() || mini.is_null() || zoom.is_null() {
        return;
    }
    let c: NSRect = msg_send![&*close, frame];
    let m: NSRect = msg_send![&*mini, frame];
    let z: NSRect = msg_send![&*zoom, frame];
    // macOS 26 draws the dot edge-to-edge in its frame; earlier versions inset it by 1pt.
    let inset = if macos_major() >= 26 { 0.0 } else { 1.0 };
    let dot = c.size.width - 2.0 * inset;
    let pitch = m.origin.x - c.origin.x;
    if dot <= 0.0 || pitch <= dot {
        return;
    }
    let lights = TrafficLights {
        right: z.origin.x + z.size.width - inset,
        dot,
        gap: pitch - dot,
    };
    if let Ok(mut slot) = TRAFFIC_LIGHTS.lock() {
        *slot = Some(lights);
    }
}

fn macos_major() -> u32 {
    std::process::Command::new("sw_vers")
        .arg("-productVersion")
        .output()
        .ok()
        .and_then(|o| String::from_utf8(o.stdout).ok())
        .and_then(|v| v.trim().split('.').next().and_then(|m| m.parse().ok()))
        .unwrap_or(0)
}

#[tauri::command]
pub fn get_traffic_lights() -> Option<TrafficLights> {
    TRAFFIC_LIGHTS.lock().ok().and_then(|s| *s)
}

pub unsafe fn install(ns_win: *mut AnyObject) -> bool {
    let Some(cls) = AnyClass::get(c"NSVisualEffectView") else {
        return false;
    };
    let content: *mut AnyObject = msg_send![&*ns_win, contentView];
    if content.is_null() {
        return false;
    }
    let bounds: NSRect = msg_send![&*content, bounds];
    let view: *mut AnyObject = msg_send![cls, alloc];
    let view: *mut AnyObject = msg_send![view, initWithFrame: bounds];
    // NSViewWidthSizable | NSViewHeightSizable
    let _: () = msg_send![&*view, setAutoresizingMask: 18u64];
    // NSVisualEffectBlendingModeBehindWindow
    let _: () = msg_send![&*view, setBlendingMode: 0isize];
    // NSVisualEffectMaterialUnderWindowBackground
    let _: () = msg_send![&*view, setMaterial: 21isize];
    // NSVisualEffectStateActive: keep the frost when the window loses focus
    let _: () = msg_send![&*view, setState: 1isize];
    let _: () = msg_send![&*view, setWantsLayer: Bool::YES];
    // NSWindowBelow = -1: underneath the webview
    let _: () = msg_send![&*content, addSubview: view, positioned: -1isize, relativeTo: std::ptr::null_mut::<AnyObject>()];

    EFFECT_VIEW.store(view as usize, Ordering::SeqCst);
    retune();
    true
}

/// NSVisualEffectView rebuilds its layers on focus/appearance/resize changes,
/// so this must be re-run after those events.
pub fn retune() {
    let view = EFFECT_VIEW.load(Ordering::SeqCst) as *mut AnyObject;
    if view.is_null() {
        return;
    }
    unsafe {
        let _: () = msg_send![&*view, layoutSubtreeIfNeeded];
        let _: () = msg_send![&*view, displayIfNeeded];
        let layer: *mut AnyObject = msg_send![&*view, layer];
        if layer.is_null() {
            return;
        }
        let _: () = msg_send![&*layer, setCornerRadius: CORNER_RADIUS];
        let _: () = msg_send![&*layer, setMasksToBounds: Bool::YES];
        tune_layer(layer);
    }
}

unsafe fn tune_layer(layer: *mut AnyObject) {
    let sublayers: *mut AnyObject = msg_send![&*layer, sublayers];
    if sublayers.is_null() {
        return;
    }
    let count: usize = msg_send![&*sublayers, count];
    for i in 0..count {
        let sub: *mut AnyObject = msg_send![&*sublayers, objectAtIndex: i];
        let name = (*sub).class().name().to_string_lossy();
        if name.contains("Backdrop") {
            set_backdrop_filters(sub);
            let _: () = msg_send![&*sub, setHidden: Bool::NO];
        } else {
            let has_children: *mut AnyObject = msg_send![&*sub, sublayers];
            if has_children.is_null() {
                // Leaf tint/fill layer from the system material
                let _: () = msg_send![&*sub, setHidden: Bool::YES];
            } else {
                tune_layer(sub);
            }
        }
    }
}

unsafe fn set_backdrop_filters(backdrop: *mut AnyObject) {
    let Some(filter_cls) = AnyClass::get(c"CAFilter") else {
        return;
    };
    let Some(number_cls) = AnyClass::get(c"NSNumber") else {
        return;
    };
    let Some(array_cls) = AnyClass::get(c"NSArray") else {
        return;
    };

    let blur_type = NSString::from_str("gaussianBlur");
    let blur: *mut AnyObject = msg_send![filter_cls, filterWithType: &*blur_type];
    let radius: *mut AnyObject = msg_send![number_cls, numberWithDouble: BLUR_RADIUS];
    let _: () = msg_send![&*blur, setValue: radius, forKey: &*NSString::from_str("inputRadius")];
    let _: () = msg_send![&*blur, setValue: &*number_bool(number_cls, true), forKey: &*NSString::from_str("inputNormalizeEdges")];

    let sat_type = NSString::from_str("colorSaturate");
    let sat: *mut AnyObject = msg_send![filter_cls, filterWithType: &*sat_type];
    let amount: *mut AnyObject = msg_send![number_cls, numberWithDouble: SATURATION];
    let _: () = msg_send![&*sat, setValue: amount, forKey: &*NSString::from_str("inputAmount")];

    let filters: [*mut AnyObject; 2] = [blur, sat];
    let arr: *mut AnyObject = msg_send![array_cls, arrayWithObjects: filters.as_ptr(), count: 2usize];
    let _: () = msg_send![&*backdrop, setFilters: arr];
    // Full-resolution sampling so the radius maps 1:1 to points like CSS blur()
    let _: () = msg_send![&*backdrop, setScale: 1.0f64];
}

unsafe fn number_bool(number_cls: &AnyClass, v: bool) -> &'static AnyObject {
    let n: *mut AnyObject = msg_send![number_cls, numberWithBool: Bool::new(v)];
    &*n
}

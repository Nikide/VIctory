//! Presents a composed render target when the plane's own resource is smaller.
//!
//! The console composites at the title's own resolution and hands the display a
//! plane; the presenter here is a shortcut that copies the first plane's first
//! resource as-is. A title that composes into a larger target (GTA VI renders
//! its UI at 3840x2160 and presents a 1920x1080 plane) therefore shows black.
//! This keeps the largest target drawn since the last present so the presenter
//! can offer it instead. `VICTORY_COMPOSE_FALLBACK=0` turns it off.
use std::sync::Mutex;

use windows::Win32::Graphics::Direct3D12::ID3D12Resource;

use crate::object::This;

/// The guest handle is stored as an address: the pointer itself is not `Send`.
/// The counter is `crate::draw::drawn()` when the target was bound: a target
/// with no draw after that was never rendered into.
static LARGEST: Mutex<Option<(u32, usize, ID3D12Resource, u64)>> = Mutex::new(None);

pub(crate) fn enabled() -> bool {
    static ON: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *ON.get_or_init(|| std::env::var_os("VICTORY_COMPOSE_FALLBACK").map_or(true, |v| v != "0"))
}

/// Records a render target drawn this frame, keeping the widest.
pub(crate) fn note(guest: This, texture: &ID3D12Resource) {
    if !enabled() || guest.is_null() {
        return;
    }
    // SAFETY: a live resource.
    let width = unsafe { texture.GetDesc() }.Width as u32;
    let drawn = crate::draw::drawn();
    let mut slot = LARGEST.lock().unwrap_or_else(|e| e.into_inner());
    if slot.as_ref().map_or(true, |(w, _, _, _)| width > *w) {
        *slot = Some((width, guest as usize, texture.clone(), drawn));
    }
}

/// Takes the composed target if it is wider than the plane being presented.
/// The last composed target that was actually presented, so a frame the title
/// hands over empty keeps showing the previous picture instead of going black.
static LAST_GOOD: Mutex<Option<(u32, usize, ID3D12Resource)>> = Mutex::new(None);

pub(crate) fn remember_presented(guest: This, texture: &ID3D12Resource) {
    if !enabled() || guest.is_null() {
        return;
    }
    // SAFETY: a live resource.
    let width = unsafe { texture.GetDesc() }.Width as u32;
    let mut slot = LAST_GOOD.lock().unwrap_or_else(|e| e.into_inner());
    *slot = Some((width, guest as usize, texture.clone()));
}

pub(crate) fn last_presented() -> Option<(This, ID3D12Resource)> {
    let slot = LAST_GOOD.lock().unwrap_or_else(|e| e.into_inner());
    slot.as_ref().map(|(_, guest, texture)| (*guest as This, texture.clone()))
}

pub(crate) fn take_wider_than(width: u32) -> Option<(This, ID3D12Resource)> {
    let drawn = crate::draw::drawn();
    let mut slot = LARGEST.lock().unwrap_or_else(|e| e.into_inner());
    match slot.take() {
        // Only a target that received a draw since it was bound is worth
        // presenting: the title presents empty planes while it loads, and
        // showing an untouched target there flickers black.
        Some((w, guest, texture, bound_at)) if w > width && drawn > bound_at => {
            Some((guest as This, texture))
        }
        _ => None,
    }
}

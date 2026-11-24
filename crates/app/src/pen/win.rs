use super::PenEvent;
use super::Phase;
use super::ink::{Ink, Sample, contact, tilt_of};
use klad_core::rng::{FNV_OFFSET, fnv};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Mutex, MutexGuard, OnceLock};
use std::time::{Duration, Instant};
use windows_sys::Win32::Foundation::{HANDLE, HWND, LPARAM, LRESULT, POINT, RECT, WPARAM};
use windows_sys::Win32::Graphics::Gdi::ClientToScreen;
use windows_sys::Win32::System::Performance::QueryPerformanceFrequency;
use windows_sys::Win32::UI::Controls::{
    FEEDBACK_PEN_BARRELVISUALIZATION, FEEDBACK_PEN_DOUBLETAP, FEEDBACK_PEN_PRESSANDHOLD, FEEDBACK_PEN_RIGHTTAP,
    FEEDBACK_PEN_TAP, POINTER_DEVICE_INFO, SetWindowFeedbackSetting,
};
use windows_sys::Win32::UI::Input::Pointer::{
    GetPointerDevice, GetPointerDeviceRects, GetPointerInfo, GetPointerPenInfo, GetPointerPenInfoHistory,
    GetPointerType, POINTER_FLAG_PRIMARY, POINTER_INFO, POINTER_PEN_INFO,
};
use windows_sys::Win32::UI::Shell::{DefSubclassProc, RemoveWindowSubclass, SetWindowSubclass};
use windows_sys::Win32::UI::WindowsAndMessaging::{
    GetMessageExtraInfo, PEN_FLAG_ERASER, PEN_FLAG_INVERTED, PEN_MASK_PRESSURE, PEN_MASK_TILT_X, PEN_MASK_TILT_Y,
    PT_PEN, PT_TOUCH, WM_KILLFOCUS, WM_LBUTTONDOWN, WM_LBUTTONUP, WM_MOUSEMOVE, WM_NCDESTROY, WM_POINTERCAPTURECHANGED,
    WM_POINTERDOWN, WM_POINTERUP, WM_POINTERUPDATE,
};

static INK: Mutex<Ink> = Mutex::new(Ink::new());
static DEVICES: Mutex<Vec<(usize, u64)>> = Mutex::new(Vec::new());
static ATTACHED: AtomicBool = AtomicBool::new(false);
const SUBCLASS: usize = 0x6b6c_6164;
const FROM_POINTER: u32 = 0xff51_5700;
const PRESSURE_MAX: f32 = 1024.0;
const NEAR_PX: f64 = 2.0;

fn ink() -> MutexGuard<'static, Ink> {
    INK.lock().unwrap_or_else(|e| e.into_inner())
}

pub struct Pen;

impl Pen {
    pub fn install() -> Option<Self> {
        ATTACHED.load(Ordering::Relaxed).then(|| {
            ink().clear();
            Pen
        })
    }

    pub fn drain(&self, ctx: &egui::Context) -> Vec<PenEvent> {
        ink().take(ctx.pixels_per_point())
    }
}

pub fn attach(hwnd: isize) {
    let hwnd = hwnd as HWND;
    let off: i32 = 0;
    [
        FEEDBACK_PEN_BARRELVISUALIZATION,
        FEEDBACK_PEN_TAP,
        FEEDBACK_PEN_DOUBLETAP,
        FEEDBACK_PEN_PRESSANDHOLD,
        FEEDBACK_PEN_RIGHTTAP,
    ]
    .into_iter()
    .for_each(|feedback| unsafe {
        SetWindowFeedbackSetting(hwnd, feedback, 0, size_of::<i32>() as u32, (&raw const off).cast());
    });
    let hooked = unsafe { SetWindowSubclass(hwnd, Some(hook), SUBCLASS, 0) } != 0;
    ATTACHED.store(hooked, Ordering::Relaxed);
}

unsafe extern "system" fn hook(
    hwnd: HWND,
    msg: u32,
    wparam: WPARAM,
    lparam: LPARAM,
    _id: usize,
    _data: usize,
) -> LRESULT {
    let now = Instant::now();
    match msg {
        WM_POINTERDOWN | WM_POINTERUPDATE | WM_POINTERUP => pointer(hwnd, (wparam & 0xffff) as u32, now),
        WM_POINTERCAPTURECHANGED | WM_KILLFOCUS => ink().lost(now),
        WM_LBUTTONDOWN | WM_LBUTTONUP | WM_MOUSEMOVE if !from_pointer() => mouse(msg, lparam, now),
        WM_NCDESTROY => {
            unsafe { RemoveWindowSubclass(hwnd, Some(hook), SUBCLASS) };
            ATTACHED.store(false, Ordering::Relaxed);
        }
        _ => {}
    }
    unsafe { DefSubclassProc(hwnd, msg, wparam, lparam) }
}

fn from_pointer() -> bool {
    (unsafe { GetMessageExtraInfo() } as u32) & 0xffff_ff00 == FROM_POINTER
}

fn mouse(msg: u32, lparam: LPARAM, now: Instant) {
    let x = (lparam & 0xffff) as u16 as i16 as f32;
    let y = ((lparam >> 16) & 0xffff) as u16 as i16 as f32;
    let phase = match msg {
        WM_LBUTTONDOWN => Phase::Down,
        WM_LBUTTONUP => Phase::Up,
        _ => Phase::Move,
    };
    ink().mouse(phase, x, y, now);
}

fn pointer(hwnd: HWND, id: u32, now: Instant) {
    let mut kind = 0;
    if unsafe { GetPointerType(id, &mut kind) } == 0 {
        return;
    }
    let samples = match kind {
        PT_PEN => pen_samples(hwnd, id),
        PT_TOUCH => touch_sample(hwnd, id).into_iter().collect(),
        _ => Vec::new(),
    };
    let mut ink = ink();
    samples.into_iter().for_each(|s| ink.sample(s, now));
}

fn pen_samples(hwnd: HWND, id: u32) -> Vec<Sample> {
    let mut newest = POINTER_PEN_INFO::default();
    if unsafe { GetPointerPenInfo(id, &mut newest) } == 0 {
        return Vec::new();
    }
    let mut count = newest.pointerInfo.historyCount.max(1);
    let mut history = vec![POINTER_PEN_INFO::default(); count as usize];
    let read = unsafe { GetPointerPenInfoHistory(id, &mut count, history.as_mut_ptr()) } != 0;
    history.truncate(if read { count as usize } else { 0 });
    if history.is_empty() {
        history.push(newest);
    }
    let place = Place::of(hwnd, newest.pointerInfo.sourceDevice);
    let device = device_key(newest.pointerInfo.sourceDevice);
    history
        .iter()
        .rev()
        .map(|p| {
            let (x, y) = place.client(&p.pointerInfo);
            Sample {
                contact: contact(p.pointerInfo.pointerFlags),
                x,
                y,
                pressure: (p.penMask & PEN_MASK_PRESSURE != 0).then(|| p.pressure as f32 / PRESSURE_MAX),
                tilt: match p.penMask & (PEN_MASK_TILT_X | PEN_MASK_TILT_Y) {
                    0 => (0.0, 0.0),
                    _ => tilt_of(p.tiltX, p.tiltY),
                },
                eraser: p.penFlags & (PEN_FLAG_ERASER | PEN_FLAG_INVERTED) != 0,
                tablet: true,
                device,
                age: age(&newest.pointerInfo, &p.pointerInfo),
            }
        })
        .collect()
}

fn touch_sample(hwnd: HWND, id: u32) -> Option<Sample> {
    let mut info = POINTER_INFO::default();
    if unsafe { GetPointerInfo(id, &mut info) } == 0 || info.pointerFlags & POINTER_FLAG_PRIMARY == 0 {
        return None;
    }
    let (x, y) = Place::of(hwnd, info.sourceDevice).client(&info);
    Some(Sample {
        contact: contact(info.pointerFlags),
        x,
        y,
        pressure: None,
        tilt: (0.0, 0.0),
        eraser: false,
        tablet: false,
        device: 0,
        age: Duration::ZERO,
    })
}

struct Place {
    origin: POINT,
    rects: Option<(RECT, RECT)>,
}

impl Place {
    fn of(hwnd: HWND, device: HANDLE) -> Self {
        let mut origin = POINT { x: 0, y: 0 };
        unsafe { ClientToScreen(hwnd, &mut origin) };
        let mut tablet = RECT::default();
        let mut display = RECT::default();
        let rects = (unsafe { GetPointerDeviceRects(device, &mut tablet, &mut display) } != 0)
            .then_some((tablet, display))
            .filter(|(t, _)| t.right > t.left && t.bottom > t.top);
        Self { origin, rects }
    }

    fn client(&self, p: &POINTER_INFO) -> (f32, f32) {
        let pixel = (p.ptPixelLocation.x as f64, p.ptPixelLocation.y as f64);
        let precise = self.rects.map(|(t, d)| {
            (
                d.left as f64
                    + (p.ptHimetricLocation.x - t.left) as f64 * (d.right - d.left) as f64 / (t.right - t.left) as f64,
                d.top as f64
                    + (p.ptHimetricLocation.y - t.top) as f64 * (d.bottom - d.top) as f64 / (t.bottom - t.top) as f64,
            )
        });
        let near = |fine: f64, coarse: f64| if (fine - coarse).abs() < NEAR_PX { fine } else { coarse };
        let (x, y) = precise.map_or(pixel, |(x, y)| (near(x, pixel.0), near(y, pixel.1)));
        ((x - self.origin.x as f64) as f32, (y - self.origin.y as f64) as f32)
    }
}

fn frequency() -> u64 {
    static FREQUENCY: OnceLock<u64> = OnceLock::new();
    *FREQUENCY.get_or_init(|| {
        let mut f = 0i64;
        match unsafe { QueryPerformanceFrequency(&mut f) } {
            0 => 0,
            _ => f.max(0) as u64,
        }
    })
}

fn age(newest: &POINTER_INFO, p: &POINTER_INFO) -> Duration {
    let freq = frequency();
    let micros = match (newest.PerformanceCount, freq) {
        (0, _) | (_, 0) => u64::from(newest.dwTime.wrapping_sub(p.dwTime)).saturating_mul(1000),
        (count, freq) => count.saturating_sub(p.PerformanceCount).saturating_mul(1_000_000) / freq,
    };
    Duration::from_micros(micros.min(1_000_000))
}

fn device_key(device: HANDLE) -> u64 {
    let handle = device as usize;
    let mut known = DEVICES.lock().unwrap_or_else(|e| e.into_inner());
    if let Some((_, key)) = known.iter().find(|(h, _)| *h == handle) {
        return *key;
    }
    let mut info = POINTER_DEVICE_INFO::default();
    let name: Vec<u8> = match unsafe { GetPointerDevice(device, &mut info) } {
        0 => Vec::new(),
        _ => info
            .productString
            .iter()
            .take_while(|c| **c != 0)
            .flat_map(|c| c.to_le_bytes())
            .collect(),
    };
    let key = if name.is_empty() { 0 } else { fnv(FNV_OFFSET, &name) };
    known.push((handle, key));
    key
}

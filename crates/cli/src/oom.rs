use std::alloc::{GlobalAlloc, Layout, System};
use std::fs::File;
use std::io::Write;
use std::sync::OnceLock;

pub struct Guard;

static NOTE: OnceLock<File> = OnceLock::new();

pub fn arm() {
    let Some(path) = klad_core::memory::note_path() else {
        return;
    };
    if let Some(dir) = path.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    if let Ok(file) = std::fs::OpenOptions::new().create(true).append(true).open(&path) {
        let _ = NOTE.set(file);
    }
}

fn digits(mut n: usize, out: &mut [u8; 24]) -> &[u8] {
    let mut at = out.len();
    loop {
        at -= 1;
        out[at] = b'0' + (n % 10) as u8;
        n /= 10;
        if n == 0 || at == 0 {
            break;
        }
    }
    &out[at..]
}

#[cold]
fn ran_out(size: usize) -> ! {
    let mut buf = [0u8; 24];
    let mb = digits(size / 1_048_576 + 1, &mut buf);
    let say = |w: &mut dyn Write| {
        let _ = w.write_all(b"klad ran out of memory: it asked the system for ");
        let _ = w.write_all(mb);
        let _ = w.write_all(b" MB more and got none. Everything up to the last stroke is saved in the drawing. Make the canvas smaller, close other drawings and apps, or open it on a machine with more memory.\n");
    };
    say(&mut std::io::stderr());
    if let Some(mut file) = NOTE.get() {
        say(&mut file);
        let _ = file.flush();
    }
    std::process::abort()
}

unsafe impl GlobalAlloc for Guard {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        let p = unsafe { System.alloc(layout) };
        if p.is_null() {
            ran_out(layout.size());
        }
        p
    }

    unsafe fn alloc_zeroed(&self, layout: Layout) -> *mut u8 {
        let p = unsafe { System.alloc_zeroed(layout) };
        if p.is_null() {
            ran_out(layout.size());
        }
        p
    }

    unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, new_size: usize) -> *mut u8 {
        let p = unsafe { System.realloc(ptr, layout, new_size) };
        if p.is_null() {
            ran_out(new_size);
        }
        p
    }

    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        unsafe { System.dealloc(ptr, layout) }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sizes_are_written_without_allocating_and_the_note_is_read_once() {
        let mut buf = [0u8; 24];
        assert_eq!(digits(0, &mut buf), b"0");
        let mut buf = [0u8; 24];
        assert_eq!(digits(1234567, &mut buf), b"1234567");
    }
}

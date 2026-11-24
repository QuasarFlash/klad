use std::path::PathBuf;
use std::process::{Command, Stdio};

pub(crate) fn refresh(drawings: Vec<PathBuf>) {
    if cfg!(all(any(target_os = "macos", windows), not(test))) && !drawings.is_empty() {
        spawn(drawings);
    }
}

fn spawn(drawings: Vec<PathBuf>) {
    let Some(klad) = crate::start::draw_binary() else {
        return;
    };
    let mut command = match cfg!(target_os = "macos") {
        true => {
            let mut nice = Command::new("/usr/bin/nice");
            nice.args(["-n", "19"]).arg(klad);
            nice
        }
        false => Command::new(klad),
    };
    command
        .args(["preview", "--changed"])
        .args(drawings)
        .env("RAYON_NUM_THREADS", "2")
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    #[cfg(unix)]
    std::os::unix::process::CommandExt::process_group(&mut command, 0);
    #[cfg(windows)]
    {
        const NO_WINDOW: u32 = 0x0800_0000;
        const IDLE_PRIORITY: u32 = 0x0000_0040;
        std::os::windows::process::CommandExt::creation_flags(&mut command, NO_WINDOW | IDLE_PRIORITY);
    }
    if let Ok(mut child) = command.spawn() {
        std::thread::spawn(move || child.wait());
    }
}

use std::path::PathBuf;
use std::process::{Command, Stdio};

pub(crate) fn refresh(drawings: Vec<PathBuf>) {
    if cfg!(all(target_os = "macos", not(test))) && !drawings.is_empty() {
        spawn(drawings);
    }
}

fn spawn(drawings: Vec<PathBuf>) {
    let Some(klad) = crate::start::draw_binary() else {
        return;
    };
    let mut command = Command::new("/usr/bin/nice");
    command
        .args(["-n", "19"])
        .arg(klad)
        .args(["preview", "--changed"])
        .args(drawings)
        .env("RAYON_NUM_THREADS", "2")
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    #[cfg(unix)]
    std::os::unix::process::CommandExt::process_group(&mut command, 0);
    if let Ok(mut child) = command.spawn() {
        std::thread::spawn(move || child.wait());
    }
}

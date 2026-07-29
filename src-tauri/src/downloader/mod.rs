pub mod bundled_tool;
pub mod filename;
pub mod gallery_dl;
pub mod gallery_dl_binary;
pub mod queue;
pub mod retry;
pub mod scheduler;

pub mod ytdlp;
pub mod ytdlp_binary;

/// Sets `creationflags(0x08000000)` (`CREATE_NO_WINDOW`) on Windows so child
/// processes (yt-dlp, ffmpeg, gallery-dl, …) don't pop up a console window.
/// No-op on other platforms.
#[cfg(windows)]
pub fn hide_cmd_window(cmd: &mut tokio::process::Command) {
    use std::os::windows::process::CommandExt;
    cmd.as_std_mut().creation_flags(0x08000000);
}

#[cfg(not(windows))]
pub fn hide_cmd_window(_cmd: &mut tokio::process::Command) {}

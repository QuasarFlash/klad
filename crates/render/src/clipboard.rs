use crate::pixelise::{Picture, decode};
use anyhow::{Result, anyhow};

pub const STAND_IN: &str = "KLAD_CLIPBOARD";

fn stand_in() -> Option<std::path::PathBuf> {
    std::env::var_os(STAND_IN).map(std::path::PathBuf::from)
}

pub fn read() -> Result<Picture> {
    if let Some(file) = stand_in() {
        let bytes = std::fs::read(&file).map_err(|_| anyhow!("the clipboard holds no picture"))?;
        return decode(&bytes);
    }
    let mut board = arboard::Clipboard::new().map_err(|e| anyhow!("the clipboard cannot be read: {e}"))?;
    let image = board
        .get_image()
        .map_err(|_| anyhow!("the clipboard holds no picture: copy one in another app first"))?;
    Ok(Picture {
        width: image.width as u32,
        height: image.height as u32,
        rgba: image.bytes.into_owned(),
    })
}

pub fn png(picture: &Picture) -> Vec<u8> {
    crate::pngwrite::write(picture.width, picture.height, 8, &picture.rgba)
}

pub fn write(picture: &Picture) -> Result<()> {
    if let Some(file) = stand_in() {
        return std::fs::write(&file, png(picture)).map_err(|e| anyhow!("writing {}: {e}", file.display()));
    }
    let mut board = arboard::Clipboard::new().map_err(|e| anyhow!("the clipboard cannot be written: {e}"))?;
    board
        .set_image(arboard::ImageData {
            width: picture.width as usize,
            height: picture.height as usize,
            bytes: std::borrow::Cow::Borrowed(&picture.rgba),
        })
        .map_err(|e| anyhow!("the picture did not reach the clipboard: {e}"))
}

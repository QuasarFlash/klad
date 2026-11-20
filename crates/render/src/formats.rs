use anyhow::{Result, anyhow, bail};
use skia_safe::{AlphaType, ColorSpace, ColorType, Image, ImageInfo, Pixmap};
use std::path::Path;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Format {
    Png,
    Jpeg,
    Webp,
    Tiff,
    Pdf,
    Mp4,
}

pub const NAMES: &str = "png, jpeg, webp, tiff, pdf or mp4";

impl Format {
    pub fn parse(text: &str) -> Result<Self> {
        match text.trim().to_ascii_lowercase().as_str() {
            "png" => Ok(Format::Png),
            "jpeg" | "jpg" => Ok(Format::Jpeg),
            "webp" => Ok(Format::Webp),
            "tiff" | "tif" => Ok(Format::Tiff),
            "pdf" => Ok(Format::Pdf),
            "mp4" | "video" => Ok(Format::Mp4),
            other => bail!("{other:?} is not a format klad writes: {NAMES}"),
        }
    }

    pub fn of_path(path: &Path) -> Option<Self> {
        path.extension()
            .and_then(|e| e.to_str())
            .and_then(|e| Format::parse(e).ok())
            .filter(|f| *f != Format::Png || path.extension().is_some_and(|e| e == "png"))
    }

    pub fn extension(self) -> &'static str {
        match self {
            Format::Png => "png",
            Format::Jpeg => "jpg",
            Format::Webp => "webp",
            Format::Tiff => "tif",
            Format::Pdf => "pdf",
            Format::Mp4 => "mp4",
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            Format::Png => "PNG",
            Format::Jpeg => "JPEG",
            Format::Webp => "WebP",
            Format::Tiff => "TIFF",
            Format::Pdf => "PDF",
            Format::Mp4 => "MP4",
        }
    }

    pub fn deep(self) -> bool {
        matches!(self, Format::Png | Format::Tiff)
    }

    pub fn transparent(self) -> bool {
        matches!(self, Format::Png | Format::Webp | Format::Tiff | Format::Pdf)
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Quality {
    pub quality: f32,
    pub lossless: bool,
    pub matte: [u8; 3],
}

impl Default for Quality {
    fn default() -> Self {
        Self {
            quality: 90.0,
            lossless: false,
            matte: [255, 255, 255],
        }
    }
}

pub fn matte_of(hex: Option<&str>) -> Result<[u8; 3]> {
    match hex {
        None => Ok([255, 255, 255]),
        Some(text) => {
            let [r, g, b, _] = klad_core::color::parse_hex(text)?;
            Ok([r, g, b].map(|c| (c * 255.0).round().clamp(0.0, 255.0) as u8))
        }
    }
}

pub fn flattened(rgba: &[u8], matte: [u8; 3]) -> Vec<u8> {
    rgba.as_chunks::<4>()
        .0
        .iter()
        .flat_map(|px| {
            let a = px[3] as f32 / 255.0;
            let mix = |c: u8, m: u8| (c as f32 * a + m as f32 * (1.0 - a)).round().clamp(0.0, 255.0) as u8;
            [mix(px[0], matte[0]), mix(px[1], matte[1]), mix(px[2], matte[2]), 255]
        })
        .collect()
}

pub fn jpeg(width: u32, height: u32, rgba: &[u8], q: Quality) -> Result<Vec<u8>> {
    let mut opaque = flattened(rgba, q.matte);
    let info = ImageInfo::new(
        (width as i32, height as i32),
        ColorType::RGBA8888,
        AlphaType::Opaque,
        ColorSpace::new_srgb(),
    );
    let pixmap =
        Pixmap::new(&info, &mut opaque, width as usize * 4).ok_or_else(|| anyhow!("cannot wrap the pixels"))?;
    let options = skia_safe::jpeg_encoder::Options {
        quality: q.quality.round().clamp(1.0, 100.0) as u32,
        ..Default::default()
    };
    skia_safe::jpeg_encoder::encode_pixmap(&pixmap, &options)
        .map(|d| d.as_bytes().to_vec())
        .ok_or_else(|| anyhow!("the JPEG encoder refused the picture"))
}

pub fn webp(width: u32, height: u32, rgba: &[u8], q: Quality) -> Result<Vec<u8>> {
    if width > 16383 || height > 16383 {
        bail!("a WebP is at most 16383 px a side; this one is {width}x{height}");
    }
    let encoder = webp::Encoder::from_rgba(rgba, width, height);
    let memory = match q.lossless {
        true => encoder.encode_lossless(),
        false => encoder.encode(q.quality.clamp(0.0, 100.0)),
    };
    Ok(memory.to_vec())
}

pub struct Still {
    pub rgba: Vec<u8>,
    pub ms: u32,
}

pub fn webp_animated(width: u32, height: u32, frames: &[Still], q: Quality, looping: bool) -> Result<Vec<u8>> {
    if width > 16383 || height > 16383 {
        bail!("a WebP is at most 16383 px a side; this one is {width}x{height}");
    }
    let mut config = webp::WebPConfig::new().map_err(|_| anyhow!("the WebP encoder did not start"))?;
    config.lossless = q.lossless as i32;
    config.quality = match q.lossless {
        true => 75.0,
        false => q.quality.clamp(0.0, 100.0),
    };
    let mut encoder = webp::AnimEncoder::new(width, height, &config);
    encoder.set_loop_count(if looping { 0 } else { 1 });
    let mut at = 0i32;
    frames.iter().for_each(|f| {
        encoder.add_frame(webp::AnimFrame::from_rgba(&f.rgba, width, height, at));
        at += f.ms.max(1) as i32;
    });
    if let Some(last) = frames.last() {
        encoder.add_frame(webp::AnimFrame::from_rgba(&last.rgba, width, height, at));
    }
    encoder
        .try_encode()
        .map(|m| m.to_vec())
        .map_err(|e| anyhow!("the WebP encoder failed: {e:?}"))
}

pub fn tiff8(width: u32, height: u32, rgba: &[u8]) -> Result<Vec<u8>> {
    let mut out = std::io::Cursor::new(Vec::new());
    tiff::encoder::TiffEncoder::new(&mut out)?
        .with_compression(tiff::encoder::Compression::Deflate(
            tiff::encoder::DeflateLevel::Balanced,
        ))
        .write_image::<tiff::encoder::colortype::RGBA8>(width, height, rgba)?;
    Ok(out.into_inner())
}

pub fn tiff16(width: u32, height: u32, rgba: &[u16]) -> Result<Vec<u8>> {
    let mut out = std::io::Cursor::new(Vec::new());
    tiff::encoder::TiffEncoder::new(&mut out)?
        .with_compression(tiff::encoder::Compression::Deflate(
            tiff::encoder::DeflateLevel::Balanced,
        ))
        .write_image::<tiff::encoder::colortype::RGBA16>(width, height, rgba)?;
    Ok(out.into_inner())
}

pub fn deep_rgba(image: &Image) -> Result<Vec<u16>> {
    Ok(crate::export::f16_pixels(image, AlphaType::Unpremul)?
        .iter()
        .map(|v| (v.to_f32().clamp(0.0, 1.0) * 65535.0).round() as u16)
        .collect())
}

pub fn encode(image: &Image, format: Format, deep: bool, q: Quality) -> Result<Vec<u8>> {
    let (w, h) = (image.width() as u32, image.height() as u32);
    match (format, deep) {
        (Format::Png, false) => crate::export::png8(image),
        (Format::Png, true) => crate::export::png16(image),
        (Format::Tiff, true) => tiff16(w, h, &deep_rgba(image)?),
        (Format::Tiff, false) => tiff8(w, h, &crate::export::dither8(image)?),
        (Format::Jpeg, _) => jpeg(w, h, &crate::export::dither8(image)?, q),
        (Format::Webp, _) => webp(w, h, &crate::export::dither8(image)?, q),
        (Format::Pdf | Format::Mp4, _) => bail!("a {} is not one picture", format.name()),
    }
}

pub type Picture = Image;

pub fn alone(
    width: u32,
    height: u32,
    nodes: &[klad_core::doc::Node],
    rasters: &[klad_brush::Raster],
    scale: f32,
) -> Result<Image> {
    let (w, h) = (
        ((width as f32 * scale).round() as i32).max(1),
        ((height as f32 * scale).round() as i32).max(1),
    );
    let mut surface = skia_safe::surfaces::raster(&crate::f16_info(w as u32, h as u32), None, None)
        .ok_or_else(|| anyhow!("cannot allocate {w}x{h}"))?;
    let canvas = surface.canvas();
    canvas.clear(skia_safe::Color4f::new(0.0, 0.0, 0.0, 0.0));
    canvas.scale((scale, scale));
    let sampling = skia_safe::SamplingOptions::new(skia_safe::FilterMode::Linear, skia_safe::MipmapMode::Linear);
    rasters.iter().try_for_each(|r| {
        canvas.draw_image_with_sampling_options(&crate::raster_image(r)?, (0, 0), sampling, None);
        Ok::<_, anyhow::Error>(())
    })?;
    crate::vector::draw_nodes(canvas, nodes)?;
    cut_to_drawn(&surface.image_snapshot())
}

pub fn cut_to_drawn(image: &Image) -> Result<Image> {
    let frame = crate::read_rgba8(image)?;
    let (w, h) = (frame.width as usize, frame.height as usize);
    let solid = |x: usize, y: usize| frame.rgba[(y * w + x) * 4 + 3] > 0;
    let rows: Vec<usize> = (0..h).filter(|y| (0..w).any(|x| solid(x, *y))).collect();
    let cols: Vec<usize> = (0..w).filter(|x| (0..h).any(|y| solid(*x, y))).collect();
    match (rows.first(), rows.last(), cols.first(), cols.last()) {
        (Some(top), Some(bottom), Some(left), Some(right)) => image
            .make_subset(
                None,
                skia_safe::IRect::from_ltrb(*left as i32, *top as i32, *right as i32 + 1, *bottom as i32 + 1),
                Default::default(),
            )
            .ok_or_else(|| anyhow!("cannot cut the picture")),
        _ => bail!("nothing is drawn in the selection"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn picture(w: u32, h: u32) -> Vec<u8> {
        (0..w * h)
            .flat_map(|i| {
                let (x, y) = (i % w, i / w);
                [
                    (x * 255 / w) as u8,
                    (y * 255 / h) as u8,
                    90,
                    if x < w / 2 { 255 } else { 0 },
                ]
            })
            .collect()
    }

    #[test]
    fn jpeg_webp_and_tiff_read_back_as_the_picture() {
        let (w, h) = (40u32, 24u32);
        let rgba = picture(w, h);
        let j = jpeg(w, h, &rgba, Quality::default()).unwrap();
        assert_eq!(&j[..3], &[0xFF, 0xD8, 0xFF]);
        let back = zune_jpeg::JpegDecoder::new(zune_jpeg::zune_core::bytestream::ZCursor::new(&j))
            .decode()
            .unwrap();
        let right = (10 * w as usize + 35) * 3;
        assert!(
            back[right] > 240 && back[right + 2] > 240,
            "transparency became the white matte"
        );
        let lossless = webp(
            w,
            h,
            &rgba,
            Quality {
                lossless: true,
                ..Quality::default()
            },
        )
        .unwrap();
        assert_eq!(&lossless[..4], b"RIFF");
        assert_eq!(&lossless[8..15], b"WEBPVP8");
        let decoded = webp::Decoder::new(&lossless).decode().unwrap();
        assert_eq!((decoded.width(), decoded.height()), (w, h));
        assert_eq!(&decoded[..4], &rgba[..4], "lossless keeps the pixels");
        let small = webp(
            w,
            h,
            &rgba,
            Quality {
                quality: 40.0,
                ..Quality::default()
            },
        )
        .unwrap();
        assert!(webp::Decoder::new(&small).decode().is_some());
        let frames = [
            Still {
                rgba: rgba.clone(),
                ms: 100,
            },
            Still {
                rgba: flattened(&rgba, [0, 0, 0]),
                ms: 200,
            },
        ];
        let anim = webp_animated(
            w,
            h,
            &frames,
            Quality {
                lossless: true,
                ..Quality::default()
            },
            true,
        )
        .unwrap();
        assert!(anim.windows(4).any(|c| c == b"ANIM") && anim.windows(4).filter(|c| *c == b"ANMF").count() >= 2);
        let t = tiff8(w, h, &rgba).unwrap();
        let mut d = tiff::decoder::Decoder::new(std::io::Cursor::new(&t)).unwrap();
        assert_eq!(d.dimensions().unwrap(), (w, h));
        match d.read_image().unwrap() {
            tiff::decoder::DecodingResult::U8(px) => assert_eq!(px, rgba),
            _ => panic!("an 8-bit TIFF"),
        }
        let deep: Vec<u16> = rgba.iter().map(|v| *v as u16 * 257).collect();
        let t16 = tiff16(w, h, &deep).unwrap();
        match tiff::decoder::Decoder::new(std::io::Cursor::new(&t16))
            .unwrap()
            .read_image()
            .unwrap()
        {
            tiff::decoder::DecodingResult::U16(px) => assert_eq!(px, deep),
            _ => panic!("a 16-bit TIFF"),
        }
        assert_eq!(Format::of_path(Path::new("a/b.JPG")), Some(Format::Jpeg));
        assert_eq!(Format::of_path(Path::new("b.mp4")), Some(Format::Mp4));
        assert_eq!(Format::of_path(Path::new("b")), None);
    }
}

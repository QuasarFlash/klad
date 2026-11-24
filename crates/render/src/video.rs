use anyhow::{Result, anyhow, bail};
use rav1e::prelude::*;

pub struct Shot {
    pub rgba: Vec<u8>,
    pub ms: u32,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Settings {
    pub quality: f32,
    pub matte: [u8; 3],
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            quality: 80.0,
            matte: [255, 255, 255],
        }
    }
}

fn even(v: u32) -> u32 {
    v + (v & 1)
}

fn planes(width: u32, height: u32, rgba: &[u8], matte: [u8; 3]) -> (Vec<u8>, Vec<u8>, Vec<u8>) {
    let (w, h) = (even(width) as usize, even(height) as usize);
    let rgb = |x: usize, y: usize| -> [f32; 3] {
        let (x, y) = (x.min(width as usize - 1), y.min(height as usize - 1));
        let i = (y * width as usize + x) * 4;
        let a = rgba[i + 3] as f32 / 255.0;
        [0, 1, 2].map(|c| (rgba[i + c] as f32 * a + matte[c] as f32 * (1.0 - a)) / 255.0)
    };
    let luma = |[r, g, b]: [f32; 3]| 0.2126 * r + 0.7152 * g + 0.0722 * b;
    let y_plane: Vec<u8> = (0..h)
        .flat_map(|y| (0..w).map(move |x| (16.0 + 219.0 * luma(rgb(x, y))).round().clamp(0.0, 255.0) as u8))
        .collect();
    let chroma = |pick: fn([f32; 3], f32) -> f32| -> Vec<u8> {
        (0..h / 2)
            .flat_map(|y| {
                (0..w / 2).map(move |x| {
                    let mean = [(0, 0), (1, 0), (0, 1), (1, 1)]
                        .iter()
                        .map(|(dx, dy)| {
                            let c = rgb(2 * x + dx, 2 * y + dy);
                            pick(c, luma(c))
                        })
                        .sum::<f32>()
                        / 4.0;
                    (128.0 + 224.0 * mean).round().clamp(0.0, 255.0) as u8
                })
            })
            .collect()
    };
    let u = chroma(|[_, _, b], y| (b - y) / 1.8556);
    let v = chroma(|[r, _, _], y| (r - y) / 1.5748);
    (y_plane, u, v)
}

fn obus(data: &[u8]) -> Vec<(u8, &[u8])> {
    let mut found = Vec::new();
    let mut at = 0;
    while at < data.len() {
        let header = data[at];
        let kind = (header >> 3) & 0x0F;
        let extension = header & 0x04 != 0;
        let sized = header & 0x02 != 0;
        let mut p = at + 1 + extension as usize;
        let size = match sized {
            true => {
                let (mut value, mut shift) = (0usize, 0);
                while p < data.len() {
                    let b = data[p];
                    value |= ((b & 0x7F) as usize) << shift;
                    p += 1;
                    shift += 7;
                    if b & 0x80 == 0 {
                        break;
                    }
                }
                value
            }
            false => data.len() - p,
        };
        let end = (p + size).min(data.len());
        found.push((kind, &data[at..end]));
        at = end;
    }
    found
}

const TEMPORAL_DELIMITER: u8 = 2;
const SEQUENCE_HEADER: u8 = 1;

struct Encoded {
    samples: Vec<(Vec<u8>, bool)>,
    config: Vec<u8>,
}

fn encode(width: u32, height: u32, shots: &[Shot], s: Settings) -> Result<Encoded> {
    let quantizer = (255.0 - s.quality.clamp(0.0, 100.0) * 2.0).round().clamp(10.0, 255.0) as usize;
    let enc = EncoderConfig {
        width: even(width) as usize,
        height: even(height) as usize,
        bit_depth: 8,
        chroma_sampling: ChromaSampling::Cs420,
        time_base: Rational::new(1, 1000),
        low_latency: true,
        quantizer,
        min_key_frame_interval: 12,
        max_key_frame_interval: 240,
        pixel_range: PixelRange::Limited,
        color_description: Some(ColorDescription {
            color_primaries: ColorPrimaries::BT709,
            transfer_characteristics: TransferCharacteristics::BT709,
            matrix_coefficients: MatrixCoefficients::BT709,
        }),
        speed_settings: SpeedSettings::from_preset(10),
        ..Default::default()
    };
    let cfg = Config::new().with_encoder_config(enc);
    let mut ctx: Context<u8> = cfg
        .new_context()
        .map_err(|e| anyhow!("the video encoder refused its settings: {e}"))?;
    let mut samples = Vec::new();
    let take = |ctx: &mut Context<u8>, samples: &mut Vec<(Vec<u8>, bool)>| -> Result<bool> {
        match ctx.receive_packet() {
            Ok(packet) => {
                samples.push((packet.data, packet.frame_type == FrameType::KEY));
                Ok(true)
            }
            Err(EncoderStatus::Encoded) | Err(EncoderStatus::NeedMoreData) => Ok(false),
            Err(EncoderStatus::LimitReached) => Ok(false),
            Err(e) => Err(anyhow!("the video encoder failed: {e:?}")),
        }
    };
    let w = even(width) as usize;
    for shot in shots {
        let (y, u, v) = planes(width, height, &shot.rgba, s.matte);
        let mut frame = ctx.new_frame();
        frame.planes[0].copy_from_raw_u8(&y, w, 1);
        frame.planes[1].copy_from_raw_u8(&u, w / 2, 1);
        frame.planes[2].copy_from_raw_u8(&v, w / 2, 1);
        ctx.send_frame(frame)
            .map_err(|e| anyhow!("the video encoder took no frame: {e:?}"))?;
        while take(&mut ctx, &mut samples)? {}
    }
    ctx.flush();
    loop {
        match ctx.receive_packet() {
            Ok(packet) => samples.push((packet.data, packet.frame_type == FrameType::KEY)),
            Err(EncoderStatus::LimitReached) => break,
            Err(EncoderStatus::Encoded) => {}
            Err(EncoderStatus::NeedMoreData) => break,
            Err(e) => bail!("the video encoder failed: {e:?}"),
        }
    }
    let samples: Vec<(Vec<u8>, bool)> = samples
        .into_iter()
        .map(|(data, key)| {
            let kept: Vec<u8> = obus(&data)
                .into_iter()
                .filter(|(kind, _)| *kind != TEMPORAL_DELIMITER)
                .flat_map(|(_, bytes)| bytes.to_vec())
                .collect();
            (kept, key)
        })
        .collect();
    let header = samples
        .iter()
        .find_map(|(data, _)| {
            obus(data)
                .into_iter()
                .find(|(k, _)| *k == SEQUENCE_HEADER)
                .map(|(_, b)| b.to_vec())
        })
        .unwrap_or_default();
    let mut config = ctx.container_sequence_header();
    config.extend(header);
    Ok(Encoded { samples, config })
}

fn bx(kind: &[u8; 4], body: &[u8]) -> Vec<u8> {
    let mut out = ((body.len() + 8) as u32).to_be_bytes().to_vec();
    out.extend_from_slice(kind);
    out.extend_from_slice(body);
    out
}

fn full(kind: &[u8; 4], version: u8, flags: u32, body: &[u8]) -> Vec<u8> {
    let mut b = vec![version];
    b.extend_from_slice(&flags.to_be_bytes()[1..]);
    b.extend_from_slice(body);
    bx(kind, &b)
}

fn be32(v: u32) -> [u8; 4] {
    v.to_be_bytes()
}

fn be16(v: u16) -> [u8; 2] {
    v.to_be_bytes()
}

const MATRIX: [u32; 9] = [0x0001_0000, 0, 0, 0, 0x0001_0000, 0, 0, 0, 0x4000_0000];

fn cat(parts: &[&[u8]]) -> Vec<u8> {
    parts.concat()
}

fn moov(width: u32, height: u32, enc: &Encoded, durations: &[u32], offset: u32) -> Vec<u8> {
    let total: u32 = durations.iter().sum();
    let matrix: Vec<u8> = MATRIX.iter().flat_map(|v| be32(*v)).collect();
    let mvhd = full(
        b"mvhd",
        0,
        0,
        &cat(&[
            &be32(0),
            &be32(0),
            &be32(1000),
            &be32(total),
            &be32(0x0001_0000),
            &be16(0x0100),
            &[0; 10],
            &matrix,
            &[0; 24],
            &be32(2),
        ]),
    );
    let tkhd = full(
        b"tkhd",
        0,
        3,
        &cat(&[
            &be32(0),
            &be32(0),
            &be32(1),
            &be32(0),
            &be32(total),
            &[0; 8],
            &be16(0),
            &be16(0),
            &be16(0),
            &be16(0),
            &matrix,
            &be32(width << 16),
            &be32(height << 16),
        ]),
    );
    let mdhd = full(
        b"mdhd",
        0,
        0,
        &cat(&[&be32(0), &be32(0), &be32(1000), &be32(total), &be16(0x55C4), &be16(0)]),
    );
    let hdlr = full(b"hdlr", 0, 0, &cat(&[&be32(0), b"vide", &[0; 12], b"klad\0"]));
    let vmhd = full(b"vmhd", 0, 1, &[0; 8]);
    let dinf = bx(
        b"dinf",
        &full(b"dref", 0, 0, &cat(&[&be32(1), &full(b"url ", 0, 1, &[])])),
    );
    let mut name = [0u8; 32];
    let label = b"AOM Coding";
    name[0] = label.len() as u8;
    name[1..=label.len()].copy_from_slice(label);
    let colr = bx(b"colr", &cat(&[b"nclx", &be16(1), &be16(1), &be16(1), &[0]]));
    let av01 = bx(
        b"av01",
        &cat(&[
            &[0; 6],
            &be16(1),
            &[0; 16],
            &be16(even(width) as u16),
            &be16(even(height) as u16),
            &be32(0x0048_0000),
            &be32(0x0048_0000),
            &be32(0),
            &be16(1),
            &name,
            &be16(0x0018),
            &(-1i16).to_be_bytes(),
            &bx(b"av1C", &enc.config),
            &colr,
        ]),
    );
    let stsd = full(b"stsd", 0, 0, &cat(&[&be32(1), &av01]));
    let runs = durations.iter().fold(Vec::<(u32, u32)>::new(), |mut runs, d| {
        match runs.last_mut() {
            Some((n, last)) if *last == *d => *n += 1,
            _ => runs.push((1, *d)),
        }
        runs
    });
    let stts = full(
        b"stts",
        0,
        0,
        &cat(&[
            &be32(runs.len() as u32),
            &runs
                .iter()
                .flat_map(|(n, d)| [be32(*n), be32(*d)].concat())
                .collect::<Vec<u8>>(),
        ]),
    );
    let keys: Vec<u32> = enc
        .samples
        .iter()
        .enumerate()
        .filter(|(_, (_, key))| *key)
        .map(|(i, _)| i as u32 + 1)
        .collect();
    let stss = full(
        b"stss",
        0,
        0,
        &cat(&[
            &be32(keys.len() as u32),
            &keys.iter().flat_map(|k| be32(*k)).collect::<Vec<u8>>(),
        ]),
    );
    let n = enc.samples.len() as u32;
    let stsc = full(b"stsc", 0, 0, &cat(&[&be32(1), &be32(1), &be32(n), &be32(1)]));
    let stsz = full(
        b"stsz",
        0,
        0,
        &cat(&[
            &be32(0),
            &be32(n),
            &enc.samples
                .iter()
                .flat_map(|(d, _)| be32(d.len() as u32))
                .collect::<Vec<u8>>(),
        ]),
    );
    let stco = full(b"stco", 0, 0, &cat(&[&be32(1), &be32(offset)]));
    let stbl = bx(b"stbl", &cat(&[&stsd, &stts, &stss, &stsc, &stsz, &stco]));
    let minf = bx(b"minf", &cat(&[&vmhd, &dinf, &stbl]));
    let mdia = bx(b"mdia", &cat(&[&mdhd, &hdlr, &minf]));
    let trak = bx(b"trak", &cat(&[&tkhd, &mdia]));
    bx(b"moov", &cat(&[&mvhd, &trak]))
}

pub fn mp4(width: u32, height: u32, shots: &[Shot], s: Settings) -> Result<Vec<u8>> {
    if shots.is_empty() {
        bail!("a video needs at least one frame");
    }
    if width > 8192 || height > 8192 {
        bail!("a video is at most 8192 px a side; this one is {width}x{height}");
    }
    let enc = encode(width, height, shots, s)?;
    if enc.samples.len() != shots.len() {
        bail!(
            "the video encoder gave {} frames for {}",
            enc.samples.len(),
            shots.len()
        );
    }
    let durations: Vec<u32> = shots.iter().map(|s| s.ms.max(1)).collect();
    let ftyp = bx(
        b"ftyp",
        &cat(&[b"isom", &be32(0x200), b"isom", b"iso6", b"av01", b"mp41"]),
    );
    let size = moov(width, height, &enc, &durations, 0).len();
    let offset = (ftyp.len() + size + 8) as u32;
    let moov = moov(width, height, &enc, &durations, offset);
    let data: Vec<u8> = enc.samples.iter().flat_map(|(d, _)| d.clone()).collect();
    Ok(cat(&[&ftyp, &moov, &bx(b"mdat", &data)]))
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    pub fn boxes(data: &[u8]) -> Vec<(String, &[u8])> {
        let mut at = 0;
        let mut found = Vec::new();
        while at + 8 <= data.len() {
            let size = u32::from_be_bytes(data[at..at + 4].try_into().unwrap()) as usize;
            let kind = String::from_utf8_lossy(&data[at + 4..at + 8]).to_string();
            found.push((kind, &data[at + 8..at + size]));
            at += size.max(8);
        }
        found
    }

    pub fn find<'a>(data: &'a [u8], path: &[&str]) -> Option<&'a [u8]> {
        let (first, rest) = path.split_first()?;
        let body = boxes(data).into_iter().find(|(k, _)| k == first)?.1;
        match rest.is_empty() {
            true => Some(body),
            false => find(body, rest),
        }
    }

    #[test]
    fn a_clip_becomes_an_av1_mp4_with_its_frame_times() {
        let (w, h) = (33u32, 20u32);
        let shot = |shade: u8| Shot {
            rgba: (0..w * h).flat_map(|i| [shade, (i % w * 7) as u8, 200, 255]).collect(),
            ms: 0,
        };
        let shots: Vec<Shot> = [(10, 83), (120, 83), (240, 250)]
            .into_iter()
            .map(|(s, ms)| Shot { ms, ..shot(s) })
            .collect();
        let file = mp4(w, h, &shots, Settings::default()).unwrap();
        let top: Vec<String> = boxes(&file).into_iter().map(|(k, _)| k).collect();
        assert_eq!(top, ["ftyp", "moov", "mdat"]);
        let stsz = find(&file, &["moov", "trak", "mdia", "minf", "stbl", "stsz"]).unwrap();
        assert_eq!(
            u32::from_be_bytes(stsz[8..12].try_into().unwrap()),
            3,
            "a sample per frame"
        );
        let mvhd = find(&file, &["moov", "mvhd"]).unwrap();
        assert_eq!(
            u32::from_be_bytes(mvhd[16..20].try_into().unwrap()),
            416,
            "the frames' times add up"
        );
        let stsd = find(&file, &["moov", "trak", "mdia", "minf", "stbl", "stsd"]).unwrap();
        assert_eq!(&stsd[12..16], b"av01");
        let stss = find(&file, &["moov", "trak", "mdia", "minf", "stbl", "stss"]).unwrap();
        assert_eq!(
            u32::from_be_bytes(stss[8..12].try_into().unwrap()),
            1,
            "the first frame is a key frame"
        );
        let mdat = find(&file, &["mdat"]).unwrap();
        assert!(obus(mdat).iter().all(|(k, _)| *k != TEMPORAL_DELIMITER));
        assert!(mp4(w, h, &[], Settings::default()).is_err());
    }
}

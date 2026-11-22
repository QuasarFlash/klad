const RT_ICON: u16 = 3;
const RT_GROUP_ICON: u16 = 14;
const RT_VERSION: u16 = 16;
const ENGLISH: u16 = 0x0409;
const UNICODE: u16 = 1200;

pub struct About<'a> {
    pub version: &'a str,
    pub description: &'a str,
    pub file: &'a str,
}

fn pad(out: &mut Vec<u8>) {
    while !out.len().is_multiple_of(4) {
        out.push(0);
    }
}

fn wide(text: &str) -> Vec<u8> {
    text.encode_utf16().chain([0]).flat_map(u16::to_le_bytes).collect()
}

fn entry(out: &mut Vec<u8>, kind: u16, id: u16, flags: u16, data: &[u8]) {
    let mut header = Vec::new();
    header.extend((data.len() as u32).to_le_bytes());
    header.extend(32u32.to_le_bytes());
    header.extend([0xFFFF, kind, 0xFFFF, id].iter().flat_map(|w: &u16| w.to_le_bytes()));
    header.extend(0u32.to_le_bytes());
    header.extend(flags.to_le_bytes());
    header.extend(if kind == 0 { 0u16 } else { ENGLISH }.to_le_bytes());
    header.extend(0u32.to_le_bytes());
    header.extend(0u32.to_le_bytes());
    out.extend(header);
    out.extend(data);
    pad(out);
}

fn node(key: &str, kind: u16, value: &[u8], value_length: u16, children: &[Vec<u8>]) -> Vec<u8> {
    let mut out = vec![0, 0];
    out.extend(value_length.to_le_bytes());
    out.extend(kind.to_le_bytes());
    out.extend(wide(key));
    pad(&mut out);
    out.extend(value);
    children.iter().for_each(|child| {
        pad(&mut out);
        out.extend(child);
    });
    let length = out.len() as u16;
    out[..2].copy_from_slice(&length.to_le_bytes());
    out
}

fn text(key: &str, value: &str) -> Vec<u8> {
    let words = value.encode_utf16().count() as u16 + 1;
    node(key, 1, &wide(value), words, &[])
}

fn numbers(version: &str) -> [u16; 4] {
    let mut parts = version
        .split(|c: char| !c.is_ascii_digit())
        .filter(|p| !p.is_empty())
        .map(|p| p.parse::<u16>().unwrap_or(0));
    [(); 4].map(|_| parts.next().unwrap_or(0))
}

fn version_info(about: &About) -> Vec<u8> {
    let [a, b, c, d] = numbers(about.version);
    let ms = (u32::from(a) << 16) | u32::from(b);
    let ls = (u32::from(c) << 16) | u32::from(d);
    let fixed: Vec<u8> = [
        0xFEEF_04BD,
        0x0001_0000,
        ms,
        ls,
        ms,
        ls,
        0x3F,
        0,
        0x0004_0004,
        1,
        0,
        0,
        0,
    ]
    .iter()
    .flat_map(|n: &u32| n.to_le_bytes())
    .collect();
    let strings = node(
        "040904B0",
        1,
        &[],
        0,
        &[
            text("CompanyName", "Indicode BV"),
            text("FileDescription", about.description),
            text("FileVersion", about.version),
            text("InternalName", about.file.trim_end_matches(".exe")),
            text("LegalCopyright", "Copyright Indicode BV"),
            text("OriginalFilename", about.file),
            text("ProductName", "klad"),
            text("ProductVersion", about.version),
        ],
    );
    let translation = (u32::from(UNICODE) << 16) | u32::from(ENGLISH);
    let var = node("Translation", 0, &translation.to_le_bytes(), 4, &[]);
    node(
        "VS_VERSION_INFO",
        0,
        &fixed,
        52,
        &[
            node("StringFileInfo", 1, &[], 0, &[strings]),
            node("VarFileInfo", 1, &[], 0, &[var]),
        ],
    )
}

pub fn resources(ico: &[u8], about: &About) -> Result<Vec<u8>, String> {
    let word = |at: usize| ico.get(at..at + 2).map(|b| u16::from_le_bytes([b[0], b[1]]));
    let long = |at: usize| {
        ico.get(at..at + 4)
            .map(|b| u32::from_le_bytes([b[0], b[1], b[2], b[3]]))
    };
    if word(0) != Some(0) || word(2) != Some(1) {
        return Err("not an .ico file".into());
    }
    let count = word(4).ok_or("an .ico file without a count")?;
    let mut out = Vec::new();
    entry(&mut out, 0, 0, 0, &[]);
    let mut group = Vec::new();
    group.extend([0u16, 1, count].iter().flat_map(|w| w.to_le_bytes()));
    (0..count).try_for_each(|i| {
        let at = 6 + 16 * usize::from(i);
        let head = ico.get(at..at + 8).ok_or("a cut .ico entry")?;
        let size = long(at + 8).ok_or("a cut .ico entry")? as usize;
        let offset = long(at + 12).ok_or("a cut .ico entry")? as usize;
        let image = ico.get(offset..offset + size).ok_or("an .ico image past the end")?;
        let id = i + 1;
        entry(&mut out, RT_ICON, id, 0x1010, image);
        group.extend(head);
        group.extend((size as u32).to_le_bytes());
        group.extend(id.to_le_bytes());
        Ok::<_, String>(())
    })?;
    entry(&mut out, RT_GROUP_ICON, 1, 0x1030, &group);
    entry(&mut out, RT_VERSION, 1, 0x0030, &version_info(about));
    Ok(out)
}

use super::{MAX_FONT_BYTES, OutlineError as Error, Unsupported};
use std::ops::Range;

pub(super) const CHECKSUM: u32 = 0xB1B0_AFBA;
const MAX_TABLES: usize = 128;
const MAX_NAMES: usize = 1024;
const ALIASES: [u16; 11] = [1, 2, 3, 4, 6, 16, 17, 18, 21, 22, 25];

#[derive(Clone, Debug)]
pub(super) struct Table {
    pub tag: [u8; 4],
    pub range: Range<usize>,
}

pub(super) fn u16_at(data: &[u8], at: usize) -> Result<u16, Error> {
    Ok(u16::from_be_bytes(
        data.get(at..at + 2)
            .ok_or(Error::Malformed("truncated u16"))?
            .try_into()
            .unwrap(),
    ))
}
pub(super) fn u32_at(data: &[u8], at: usize) -> Result<u32, Error> {
    Ok(u32::from_be_bytes(
        data.get(at..at + 4)
            .ok_or(Error::Malformed("truncated u32"))?
            .try_into()
            .unwrap(),
    ))
}
pub(super) fn sum(data: &[u8], head: bool) -> u32 {
    data.chunks(4).enumerate().fold(0u32, |sum, (i, chunk)| {
        if head && i == 2 {
            return sum;
        }
        let mut word = [0; 4];
        word[..chunk.len()].copy_from_slice(chunk);
        sum.wrapping_add(u32::from_be_bytes(word))
    })
}
pub(super) fn parse(data: &[u8]) -> Result<Vec<Table>, Error> {
    if data.len() > MAX_FONT_BYTES {
        return Err(Error::Limit("font bytes"));
    }
    match data.get(..4) {
        Some(b"ttcf") => return Err(Error::Unsupported(Unsupported::Collection)),
        Some(b"\0\x01\0\0" | b"OTTO") => (),
        _ => return Err(Error::Unsupported(Unsupported::Container)),
    }
    let count = usize::from(u16_at(data, 4)?);
    if count == 0 || count > MAX_TABLES {
        return Err(Error::Limit("table count"));
    }
    let dir_end = 12 + count * 16;
    if data.len() < dir_end {
        return Err(Error::Malformed("table directory"));
    }
    let pow2 = 1usize << count.ilog2();
    if u16_at(data, 6)? as usize != pow2 * 16
        || u16_at(data, 8)? as u32 != count.ilog2()
        || u16_at(data, 10)? as usize != count * 16 - pow2 * 16
    {
        return Err(Error::Malformed("SFNT search fields"));
    }
    let mut tables = Vec::with_capacity(count);
    let mut previous = None;
    for i in 0..count {
        let at = 12 + i * 16;
        let tag: [u8; 4] = data[at..at + 4].try_into().unwrap();
        if previous.is_some_and(|p| p >= tag) {
            return Err(Error::Malformed("duplicate or unordered tables"));
        }
        previous = Some(tag);
        let start = u32_at(data, at + 8)? as usize;
        let len = u32_at(data, at + 12)? as usize;
        let end = start
            .checked_add(len)
            .ok_or(Error::Malformed("table overflow"))?;
        let padded_end = end
            .checked_add(3)
            .ok_or(Error::Malformed("table overflow"))?
            & !3;
        if start < dir_end || !start.is_multiple_of(4) || padded_end > data.len() {
            return Err(Error::Malformed("table bounds or alignment"));
        }
        if data[end..padded_end].iter().any(|v| *v != 0) {
            return Err(Error::Malformed("nonzero table padding"));
        }
        tables.push(Table {
            tag,
            range: start..end,
        });
    }
    let mut ranges: Vec<_> = tables
        .iter()
        .filter(|t| !t.range.is_empty())
        .map(|t| t.range.clone())
        .collect();
    ranges.sort_unstable_by_key(|r| r.start);
    if ranges.windows(2).any(|w| w[0].end > w[1].start) {
        return Err(Error::Malformed("overlapping tables"));
    }
    // Check ranges before hashing. Rejected overlapping slices must not amplify
    // a bounded input into MAX_TABLES repeated whole-file checksum scans.
    for (i, table) in tables.iter().enumerate() {
        if sum(&data[table.range.clone()], table.tag == *b"head") != u32_at(data, 16 + i * 16)? {
            return Err(Error::Malformed("table checksum"));
        }
    }
    if !data.len().is_multiple_of(4) || sum(data, false) != CHECKSUM {
        return Err(Error::Malformed("SFNT checksum"));
    }
    Ok(tables)
}
pub(super) fn table<'a>(
    bytes: &'a [u8],
    tables: &[Table],
    tag: &[u8; 4],
) -> Result<&'a [u8], Error> {
    let table = tables
        .iter()
        .find(|t| &t.tag == tag)
        .ok_or(Error::Malformed("missing required table"))?;
    Ok(&bytes[table.range.clone()])
}

struct Name {
    key: [u16; 4],
    bytes: Vec<u8>,
}
fn names(data: &[u8], alias: &str, italic: bool) -> Result<Vec<u8>, Error> {
    if data.len() > u16::MAX as usize {
        return Err(Error::Limit("name table bytes"));
    }
    let format = u16_at(data, 0)?;
    if format > 1 {
        return Err(Error::Unsupported(Unsupported::NameFormat));
    }
    let count = usize::from(u16_at(data, 2)?);
    if count > MAX_NAMES {
        return Err(Error::Limit("name records"));
    }
    let storage = usize::from(u16_at(data, 4)?);
    let mut end = 6 + count * 12;
    let mut langs = Vec::new();
    let mut retained_bytes = 0usize;
    if format == 1 {
        let count = usize::from(u16_at(data, end)?);
        if count > MAX_NAMES {
            return Err(Error::Limit("language tags"));
        }
        end += 2;
        for i in 0..count {
            let len = usize::from(u16_at(data, end + i * 4)?);
            let off = usize::from(u16_at(data, end + i * 4 + 2)?);
            retained_bytes += len;
            if retained_bytes > u16::MAX as usize {
                return Err(Error::Limit("retained language bytes"));
            }
            langs.push(
                data.get(storage + off..storage + off + len)
                    .ok_or(Error::Malformed("language tag"))?
                    .to_vec(),
            );
        }
        end += count * 4;
    }
    if storage < end || storage > data.len() {
        return Err(Error::Malformed("name storage"));
    }
    let mut names = Vec::new();
    for i in 0..count {
        let at = 6 + i * 12;
        let key = [
            u16_at(data, at)?,
            u16_at(data, at + 2)?,
            u16_at(data, at + 4)?,
            u16_at(data, at + 6)?,
        ];
        let len = usize::from(u16_at(data, at + 8)?);
        let off = usize::from(u16_at(data, at + 10)?);
        let bytes = data
            .get(storage + off..storage + off + len)
            .ok_or(Error::Malformed("name string"))?;
        if key[2] >= 0x8000 && usize::from(key[2] - 0x8000) >= langs.len() {
            return Err(Error::Malformed("name language index"));
        }
        // Keep copyright, trademark, manufacturer, designer, license, URLs,
        // descriptions and all other non-identity metadata byte-for-byte.
        if !ALIASES.contains(&key[3]) {
            retained_bytes += bytes.len();
            if retained_bytes > u16::MAX as usize {
                return Err(Error::Limit("retained name bytes"));
            }
            names.push(Name {
                key,
                bytes: bytes.to_vec(),
            });
        }
    }
    for id in ALIASES {
        let value = if [2, 17, 22].contains(&id) {
            if italic { "Italic" } else { "Regular" }
        } else {
            alias
        };
        let bytes: Vec<_> = value.encode_utf16().flat_map(u16::to_be_bytes).collect();
        names.push(Name {
            key: [3, 1, 0x0409, id],
            bytes,
        });
    }
    names.sort_by_key(|n| n.key);
    if names.len() > MAX_NAMES {
        return Err(Error::Limit("rewritten name records"));
    }
    let storage = 6 + names.len() * 12 + if format == 1 { 2 + langs.len() * 4 } else { 0 };
    let mut out = vec![0; storage];
    out[..2].copy_from_slice(&format.to_be_bytes());
    out[2..4].copy_from_slice(&(names.len() as u16).to_be_bytes());
    out[4..6].copy_from_slice(&(storage as u16).to_be_bytes());
    for (i, name) in names.iter().enumerate() {
        let at = 6 + i * 12;
        for (j, part) in name.key.iter().enumerate() {
            out[at + j * 2..at + j * 2 + 2].copy_from_slice(&part.to_be_bytes());
        }
        append_name(&mut out, storage, at + 8, &name.bytes)?;
    }
    if format == 1 {
        let at = 6 + names.len() * 12;
        out[at..at + 2].copy_from_slice(&(langs.len() as u16).to_be_bytes());
        for (i, bytes) in langs.iter().enumerate() {
            append_name(&mut out, storage, at + 2 + i * 4, bytes)?;
        }
    }
    Ok(out)
}
fn append_name(out: &mut Vec<u8>, storage: usize, at: usize, bytes: &[u8]) -> Result<(), Error> {
    if out.len() + bytes.len() > u16::MAX as usize {
        return Err(Error::Limit("name table bytes"));
    }
    out[at..at + 2].copy_from_slice(&(bytes.len() as u16).to_be_bytes());
    let offset = (out.len() - storage) as u16;
    out[at + 2..at + 4].copy_from_slice(&offset.to_be_bytes());
    out.extend_from_slice(bytes);
    Ok(())
}
pub(super) fn rewrite(
    data: &[u8],
    tables: &[Table],
    alias: &str,
    italic: bool,
) -> Result<Vec<u8>, Error> {
    let name = names(table(data, tables, b"name")?, alias, italic)?;
    let mut out = data[..12 + tables.len() * 16].to_vec();
    let mut head = None;
    for (i, t) in tables.iter().enumerate() {
        let bytes = if t.tag == *b"name" {
            name.as_slice()
        } else {
            &data[t.range.clone()]
        };
        if out.len() + bytes.len() + 3 > MAX_FONT_BYTES {
            return Err(Error::Limit("rewritten font bytes"));
        }
        let at = 12 + i * 16;
        let start = out.len();
        out.extend_from_slice(bytes);
        out.resize((out.len() + 3) & !3, 0);
        if t.tag == *b"head" {
            if bytes.len() < 54 {
                return Err(Error::Malformed("head table"));
            }
            out[start + 8..start + 12].fill(0);
            head = Some(start);
        }
        out[at + 4..at + 8].copy_from_slice(&sum(bytes, t.tag == *b"head").to_be_bytes());
        out[at + 8..at + 12].copy_from_slice(&(start as u32).to_be_bytes());
        out[at + 12..at + 16].copy_from_slice(&(bytes.len() as u32).to_be_bytes());
    }
    let head = head.ok_or(Error::Malformed("missing head"))?;
    let adjustment = CHECKSUM.wrapping_sub(sum(&out, false));
    out[head + 8..head + 12].copy_from_slice(&adjustment.to_be_bytes());
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn format_one_language_metadata_survives_alias_rewrite() {
        let license = [0, b'L', 0, b'I', 0, b'C'];
        let lang = [0, b'e', 0, b'n'];
        let mut source = vec![0; 24];
        source[0..2].copy_from_slice(&1u16.to_be_bytes());
        source[2..4].copy_from_slice(&1u16.to_be_bytes());
        source[4..6].copy_from_slice(&24u16.to_be_bytes());
        for (i, n) in [3u16, 1, 0x8000, 13, 6, 0, 1, 4, 6].iter().enumerate() {
            source[6 + i * 2..8 + i * 2].copy_from_slice(&n.to_be_bytes());
        }
        source.extend_from_slice(&license);
        source.extend_from_slice(&lang);
        let rewritten = names(&source, "FlowOutlinePrivate", false).unwrap();
        assert_eq!(u16_at(&rewritten, 0).unwrap(), 1);
        let count = usize::from(u16_at(&rewritten, 2).unwrap());
        let storage = usize::from(u16_at(&rewritten, 4).unwrap());
        let lic = (0..count)
            .map(|i| 6 + i * 12)
            .find(|at| u16_at(&rewritten, at + 6).unwrap() == 13)
            .unwrap();
        assert_eq!(u16_at(&rewritten, lic + 4).unwrap(), 0x8000);
        let off = usize::from(u16_at(&rewritten, lic + 10).unwrap());
        assert_eq!(&rewritten[storage + off..storage + off + 6], license);
        let lang_at = 6 + count * 12;
        assert_eq!(u16_at(&rewritten, lang_at).unwrap(), 1);
        let off = usize::from(u16_at(&rewritten, lang_at + 4).unwrap());
        assert_eq!(&rewritten[storage + off..storage + off + 4], lang);
    }
    #[test]
    fn repeated_metadata_ranges_cannot_multiply_allocation_past_name_limit() {
        let mut source = vec![0; 6 + 1024 * 12];
        let storage = source.len() as u16;
        source[2..4].copy_from_slice(&1024u16.to_be_bytes());
        source[4..6].copy_from_slice(&storage.to_be_bytes());
        for i in 0..1024 {
            let at = 6 + i * 12;
            source[at + 6..at + 8].copy_from_slice(&13u16.to_be_bytes());
            source[at + 8..at + 10].copy_from_slice(&100u16.to_be_bytes());
        }
        source.extend_from_slice(&[0; 100]);
        assert_eq!(
            names(&source, "FlowOutlinePrivate", false),
            Err(Error::Limit("retained name bytes"))
        );
    }
}

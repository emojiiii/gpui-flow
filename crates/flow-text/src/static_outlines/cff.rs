//! Conservative non-CID Type2 subset. No subroutine or seac expansion is
//! admitted, so byte work bounds are meaningful before entering the parser.
use super::{OutlineError as Error, Unsupported, Work, sfnt::u16_at};
use std::ops::Range;

fn index(data: &[u8], at: &mut usize) -> Result<Vec<Range<usize>>, Error> {
    let count = usize::from(u16_at(data, *at)?);
    *at += 2;
    if count == 0 {
        return Ok(Vec::new());
    }
    let size = usize::from(*data.get(*at).ok_or(Error::Malformed("CFF index"))?);
    *at += 1;
    if !(1..=4).contains(&size) {
        return Err(Error::Malformed("CFF index offset size"));
    }
    let end = *at + (count + 1) * size;
    let mut previous = 1usize;
    let mut ranges = Vec::with_capacity(count);
    for i in 0..=count {
        let off = data
            .get(*at + i * size..*at + (i + 1) * size)
            .ok_or(Error::Malformed("CFF index offsets"))?
            .iter()
            .fold(0usize, |v, b| (v << 8) | usize::from(*b));
        if off < previous || (i == 0 && off != 1) || off > data.len().saturating_sub(end) + 1 {
            return Err(Error::Malformed("CFF index range"));
        }
        if i > 0 {
            ranges.push(end + previous - 1..end + off - 1);
        }
        previous = off;
    }
    *at = end + previous - 1;
    Ok(ranges)
}
fn number(data: &[u8], at: &mut usize, dict: bool) -> Result<Option<i32>, Error> {
    let b = *data.get(*at).ok_or(Error::Malformed("CFF number"))?;
    *at += 1;
    let n = match b {
        32..=246 => Some(i32::from(b) - 139),
        247..=254 => {
            let next = i32::from(*data.get(*at).ok_or(Error::Malformed("CFF number"))?);
            *at += 1;
            Some(if b < 251 {
                (i32::from(b) - 247) * 256 + next + 108
            } else {
                -(i32::from(b) - 251) * 256 - next - 108
            })
        }
        28 => {
            let n = u16_at(data, *at)? as i16;
            *at += 2;
            Some(i32::from(n))
        }
        29 if dict => {
            let bytes = data
                .get(*at..*at + 4)
                .ok_or(Error::Malformed("CFF integer"))?;
            *at += 4;
            Some(i32::from_be_bytes(bytes.try_into().unwrap()))
        }
        30 if dict => {
            loop {
                let b = *data.get(*at).ok_or(Error::Malformed("CFF real"))?;
                *at += 1;
                if b >> 4 == 15 || b & 15 == 15 {
                    break;
                }
            }
            None
        }
        255 if !dict => {
            if data.get(*at..*at + 4).is_none() {
                return Err(Error::Malformed("CFF fixed"));
            }
            *at += 4;
            None
        }
        _ => return Err(Error::Malformed("CFF numeric opcode")),
    };
    Ok(n)
}
pub(super) fn ranges(
    data: &[u8],
    absolute: usize,
    glyphs: u16,
) -> Result<Vec<Range<usize>>, Error> {
    if data.first() != Some(&1) || data.get(1) != Some(&0) {
        return Err(Error::Unsupported(Unsupported::CffFormat));
    }
    let mut at = usize::from(*data.get(2).ok_or(Error::Malformed("CFF header"))?);
    if at < 4 || !(1..=4).contains(data.get(3).ok_or(Error::Malformed("CFF header"))?) {
        return Err(Error::Malformed("CFF header"));
    }
    if index(data, &mut at)?.len() != 1 {
        return Err(Error::Unsupported(Unsupported::Collection));
    }
    let top = index(data, &mut at)?;
    if top.len() != 1 {
        return Err(Error::Unsupported(Unsupported::Collection));
    }
    let dict = &data[top[0].clone()];
    let mut at = 0;
    let mut operands = Vec::new();
    let mut charstrings = None;
    while at < dict.len() {
        let b = dict[at];
        if b >= 28 {
            if operands.len() >= 48 {
                return Err(Error::Limit("CFF dictionary operands"));
            }
            operands.push(number(dict, &mut at, true)?);
        } else {
            at += 1;
            let op = if b == 12 {
                let n = *dict
                    .get(at)
                    .ok_or(Error::Malformed("CFF escaped operator"))?;
                at += 1;
                1200 + u16::from(n)
            } else {
                u16::from(b)
            };
            if [1230, 1236, 1237].contains(&op) {
                return Err(Error::Unsupported(Unsupported::CffFormat));
            }
            if op == 1206 && operands.as_slice() != [Some(2)] {
                return Err(Error::Unsupported(Unsupported::CffFormat));
            }
            if op == 17 {
                if charstrings.is_some() || operands.len() != 1 {
                    return Err(Error::Malformed("CFF CharStrings offset"));
                }
                charstrings = Some(
                    usize::try_from(operands[0].ok_or(Error::Malformed("CFF CharStrings offset"))?)
                        .map_err(|_| Error::Malformed("CFF CharStrings offset"))?,
                );
            }
            operands.clear();
        }
    }
    if !operands.is_empty() {
        return Err(Error::Malformed("CFF trailing operands"));
    }
    let mut at = charstrings.ok_or(Error::Malformed("CFF CharStrings"))?;
    let ranges = index(data, &mut at)?;
    if ranges.len() != usize::from(glyphs) {
        return Err(Error::Malformed("CFF glyph count"));
    }
    Ok(ranges
        .into_iter()
        .map(|r| absolute + r.start..absolute + r.end)
        .collect())
}
pub(super) fn preflight(data: &[u8], work: &mut Work) -> Result<bool, Error> {
    work.charge(data.len())?;
    if data.len() > 65_536 {
        return Err(Error::Limit("CFF glyph bytes"));
    }
    let mut at = 0;
    let mut stack = 0;
    let mut stems = 0;
    let mut geometry = false;
    while at < data.len() {
        let b = data[at];
        if b == 28 || b >= 32 {
            number(data, &mut at, false)?;
            stack += 1;
            if stack > 48 {
                return Err(Error::Limit("CFF operands"));
            }
            continue;
        }
        at += 1;
        match b {
            1 | 3 | 18 | 23 => {
                stems += stack / 2;
            }
            19 | 20 => {
                stems += stack / 2;
                at += (stems as usize).div_ceil(8);
                if at > data.len() {
                    return Err(Error::Malformed("CFF hint mask"));
                }
            }
            4..=8 | 21 | 22 | 24..=27 | 30 | 31 => geometry = true,
            12 => {
                let escaped = *data
                    .get(at)
                    .ok_or(Error::Malformed("CFF escaped operator"))?;
                at += 1;
                if !(34..=37).contains(&escaped) {
                    return Err(Error::Unsupported(Unsupported::CffProgram));
                }
                geometry = true;
            }
            14 => {
                if stack > 1 {
                    return Err(Error::Unsupported(Unsupported::CffProgram));
                }
                if at != data.len() {
                    return Err(Error::Malformed("CFF trailing glyph data"));
                }
                return Ok(!geometry);
            }
            // Includes local/global subroutines, return and deprecated seac.
            _ => return Err(Error::Unsupported(Unsupported::CffProgram)),
        }
        stack = 0;
    }
    Err(Error::Malformed("CFF missing endchar"))
}

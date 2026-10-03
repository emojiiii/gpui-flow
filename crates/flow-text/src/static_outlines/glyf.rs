use super::{
    OutlineError as Error, Unsupported, Work,
    sfnt::{u16_at, u32_at},
};
use std::ops::Range;

pub(super) fn ranges(
    loca: &[u8],
    glyf: Range<usize>,
    format: u16,
    count: u16,
) -> Result<Vec<Range<usize>>, Error> {
    let stride = match format {
        0 => 2,
        1 => 4,
        _ => return Err(Error::Malformed("loca format")),
    };
    if loca.len() != (usize::from(count) + 1) * stride {
        return Err(Error::Malformed("loca length"));
    }
    let mut ranges = Vec::with_capacity(usize::from(count));
    let mut previous = 0;
    for i in 0..=usize::from(count) {
        let offset = if stride == 2 {
            usize::from(u16_at(loca, i * 2)?) * 2
        } else {
            u32_at(loca, i * 4)? as usize
        };
        if offset < previous || offset > glyf.len() {
            return Err(Error::Malformed("loca offset"));
        }
        if i != 0 {
            ranges.push(glyf.start + previous..glyf.start + offset);
        }
        previous = offset;
    }
    Ok(ranges)
}

// Validate each expanded component before invoking ttf-parser. Its depth bound
// alone does not bound a branching composite graph or reject a truncated tail.
pub(super) fn preflight(
    bytes: &[u8],
    ranges: &[Range<usize>],
    glyph: u16,
    work: &mut Work,
    ancestors: &mut Vec<u16>,
) -> Result<bool, Error> {
    work.charge(1)?;
    if ancestors.len() >= 16 || ancestors.contains(&glyph) {
        return Err(Error::Limit("composite cycle or depth"));
    }
    let data = &bytes[ranges
        .get(usize::from(glyph))
        .ok_or(Error::Malformed("component glyph id"))?
        .clone()];
    if data.is_empty() {
        return Ok(true);
    }
    if data.len() < 10 {
        return Err(Error::Malformed("glyph header"));
    }
    let count = u16_at(data, 0)? as i16;
    if count == 0 {
        // A zero-contour glyph may have instructions, but never geometry.
        if data.len() > 10 {
            let len = usize::from(u16_at(data, 10)?);
            if 12 + len > data.len() {
                return Err(Error::Malformed("empty glyph instructions"));
            }
        }
        return Ok(true);
    }
    if count > 0 {
        let count = count as usize;
        let mut previous = None;
        for i in 0..count {
            let endpoint = u16_at(data, 10 + i * 2)?;
            if previous.is_some_and(|p| endpoint <= p) {
                return Err(Error::Malformed("glyph contour endpoints"));
            }
            previous = Some(endpoint);
        }
        let points = usize::from(previous.unwrap()) + 1;
        // ttf-parser silently skips a one-point child in a composite. Reject
        // it here so another valid child cannot hide that missing geometry.
        if points == 1 {
            return Err(Error::InvalidOutline);
        }
        work.charge(points + count)?;
        let instructions = 10 + count * 2;
        let mut at = instructions + 2 + usize::from(u16_at(data, instructions)?);
        if at > data.len() {
            return Err(Error::Malformed("glyph instructions"));
        }
        let mut left = points;
        let mut coordinates = 0;
        while left > 0 {
            let flags = *data.get(at).ok_or(Error::Malformed("glyph flags"))?;
            at += 1;
            if flags & 0x80 != 0 {
                return Err(Error::Unsupported(Unsupported::GlyphFormat));
            }
            let repeats = if flags & 8 != 0 {
                let n = *data.get(at).ok_or(Error::Malformed("glyph flag repeats"))?;
                at += 1;
                usize::from(n) + 1
            } else {
                1
            };
            if repeats > left {
                return Err(Error::Malformed("glyph flag repeats"));
            }
            left -= repeats;
            let x = if flags & 2 != 0 {
                1
            } else if flags & 16 == 0 {
                2
            } else {
                0
            };
            let y = if flags & 4 != 0 {
                1
            } else if flags & 32 == 0 {
                2
            } else {
                0
            };
            coordinates += repeats * (x + y);
        }
        if at + coordinates > data.len() {
            return Err(Error::Malformed("glyph coordinates"));
        }
        // A one-point contour is not silently classified as whitespace.
        return Ok(false);
    }
    if count != -1 {
        return Err(Error::Malformed("composite contour count"));
    }
    ancestors.push(glyph);
    let mut at = 10;
    let mut empty = true;
    loop {
        work.charge(1)?;
        let flags = u16_at(data, at)?;
        let component = u16_at(data, at + 2)?;
        at += 4;
        // Point attachment, scaled offsets and grid rounding differ across
        // outline backends. Admit explicit XY offsets and unscaled transforms.
        if flags & 2 == 0 || flags & (4 | 0x0800 | 0xE010) != 0 {
            return Err(Error::Unsupported(Unsupported::CompositeTransform));
        }
        let transforms = [8, 64, 128].into_iter().filter(|m| flags & m != 0).count();
        if transforms > 1 {
            return Err(Error::Malformed("composite transform flags"));
        }
        at += if flags & 1 != 0 { 4 } else { 2 };
        at += if flags & 128 != 0 {
            8
        } else if flags & 64 != 0 {
            4
        } else if flags & 8 != 0 {
            2
        } else {
            0
        };
        if at > data.len() {
            return Err(Error::Malformed("composite transform"));
        }
        // A transform without an explicit unscaled-offset flag has historical
        // rasterizer-dependent semantics; reject that ambiguity.
        if transforms != 0 && flags & 0x1000 == 0 {
            return Err(Error::Unsupported(Unsupported::CompositeTransform));
        }
        empty &= preflight(bytes, ranges, component, work, ancestors)?;
        if flags & 32 == 0 {
            if flags & 256 != 0 {
                let len = usize::from(u16_at(data, at)?);
                if at + 2 + len > data.len() {
                    return Err(Error::Malformed("composite instructions"));
                }
            }
            break;
        }
        if flags & 256 != 0 {
            return Err(Error::Malformed("nonfinal composite instructions"));
        }
    }
    ancestors.pop();
    Ok(empty)
}

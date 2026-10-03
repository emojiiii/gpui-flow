//! One bounded local decode per process. The owner can kill even codec work.
use image::{AnimationDecoder, ImageDecoder, ImageFormat};
use std::{
    fs::File,
    io::{Cursor, Read, Write},
    path::PathBuf,
};

fn run() -> Result<(), Box<dyn std::error::Error>> {
    let mut args = std::env::args_os().skip(1);
    let path = PathBuf::from(args.next().ok_or("missing local image path")?);
    let mut number = || -> Result<u64, Box<dyn std::error::Error>> {
        Ok(args
            .next()
            .ok_or("missing limit")?
            .to_str()
            .ok_or("invalid limit")?
            .parse()?)
    };
    let input = number()?;
    let output = number()?;
    let frames_limit = number()?;
    let dimension = number()?;
    if args.next().is_some()
        || input == 0
        || input > 1024 * 1024 * 1024
        || output == 0
        || output > 256 * 1024 * 1024
        || !(1..=4096).contains(&frames_limit)
        || !(1..=16384).contains(&dimension)
    {
        return Err("invalid image worker request".into());
    }
    let file = File::open(path)?;
    if !file.metadata()?.is_file() || file.metadata()?.len() > input {
        return Err("input must be a bounded regular file".into());
    }
    let mut bytes = Vec::new();
    file.take(input + 1).read_to_end(&mut bytes)?;
    if bytes.len() as u64 > input {
        return Err("image input exceeded byte limit".into());
    }
    let format = image::guess_format(&bytes)?;
    let mut limits = image::Limits::default();
    limits.max_image_width = Some(dimension as u32);
    limits.max_image_height = Some(dimension as u32);
    limits.max_alloc = Some(output);
    let validate = |w: u32, h: u32| -> Result<(), Box<dyn std::error::Error>> {
        if w == 0
            || h == 0
            || u64::from(w) > dimension
            || u64::from(h) > dimension
            || u64::from(w) * u64::from(h) * 4 > output
        {
            return Err("image dimensions exceed admitted output".into());
        }
        Ok(())
    };
    let mut frames = Vec::new();
    let mut retained = 0u64;
    let mut append = |frame: image::Frame| -> Result<(), Box<dyn std::error::Error>> {
        let (w, h) = frame.buffer().dimensions();
        validate(w, h)?;
        retained = retained
            .checked_add(u64::from(w) * u64::from(h) * 4)
            .ok_or("decoded size overflow")?;
        if retained > output || frames.len() as u64 >= frames_limit {
            return Err("image animation exceeds admitted output".into());
        }
        frames.push(frame);
        Ok(())
    };
    match format {
        ImageFormat::Gif => {
            let mut d = image::codecs::gif::GifDecoder::new(Cursor::new(&bytes))?;
            d.set_limits(limits)?;
            let (w, h) = d.dimensions();
            validate(w, h)?;
            for frame in d.into_frames() {
                append(frame?)?;
            }
        }
        ImageFormat::WebP => {
            let mut d = image::codecs::webp::WebPDecoder::new(Cursor::new(&bytes))?;
            d.set_limits(limits)?;
            let (w, h) = d.dimensions();
            validate(w, h)?;
            if d.has_animation() {
                d.set_background_color(image::Rgba([0, 0, 0, 0]))?;
                for frame in d.into_frames() {
                    append(frame?)?;
                }
            } else {
                append(image::Frame::new(
                    image::DynamicImage::from_decoder(d)?.into_rgba8(),
                ))?;
            }
        }
        ImageFormat::Png | ImageFormat::Jpeg => {
            let mut reader = image::ImageReader::with_format(Cursor::new(&bytes), format);
            reader.limits(limits);
            let decoder = reader.into_decoder()?;
            let (w, h) = decoder.dimensions();
            validate(w, h)?;
            append(image::Frame::new(
                image::DynamicImage::from_decoder(decoder)?.into_rgba8(),
            ))?;
        }
        _ => return Err("owned image worker supports PNG/JPEG/GIF/WebP".into()),
    }
    if frames.is_empty() {
        return Err("image contains no frames".into());
    }
    let stdout = std::io::stdout();
    let mut out = stdout.lock();
    out.write_all(b"FWI1")?;
    out.write_all(&(frames.len() as u32).to_le_bytes())?;
    for frame in frames {
        let (n, d) = frame.delay().numer_denom_ms();
        for value in [frame.buffer().width(), frame.buffer().height(), n, d] {
            out.write_all(&value.to_le_bytes())?;
        }
        out.write_all(frame.buffer().as_raw())?;
    }
    out.flush()?;
    Ok(())
}
fn main() {
    if let Err(error) = run() {
        eprintln!("{error}");
        std::process::exit(1);
    }
}

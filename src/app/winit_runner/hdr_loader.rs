use half::f16;
use image::ImageReader;
use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, Receiver};
use std::thread;

pub(super) struct HdrTextureData {
    pub(super) pixels: Vec<u16>,
    pub(super) width: u32,
    pub(super) height: u32,
}

pub(super) enum HdrLoadMessage {
    Progress { value: f32, status: String },
    Done(HdrTextureData),
    Failed(String),
}

fn load_hdr_texture_data(path: &Path) -> Result<HdrTextureData, String> {
    let mut reader = ImageReader::open(path)
        .map_err(|e| format!("failed to open hdr file `{}`: {e}", path.display()))?;
    reader.no_limits();
    let img = reader
        .with_guessed_format()
        .map_err(|e| format!("failed to detect hdr format `{}`: {e}", path.display()))?
        .decode()
        .map_err(|e| format!("failed to decode hdr image `{}`: {e}", path.display()))?;
    let rgb = img.to_rgb32f();
    let width = rgb.width();
    let height = rgb.height();
    let mut pixels = Vec::with_capacity((width * height * 4) as usize);

    for pixel in rgb.pixels() {
        let [r, g, b] = pixel.0;
        pixels.push(f16::from_f32(r.max(0.0)).to_bits());
        pixels.push(f16::from_f32(g.max(0.0)).to_bits());
        pixels.push(f16::from_f32(b.max(0.0)).to_bits());
        pixels.push(f16::from_f32(1.0).to_bits());
    }

    Ok(HdrTextureData {
        pixels,
        width,
        height,
    })
}

fn downscale_hdr_rgba16f_with_progress<F: FnMut(f32)>(
    data: HdrTextureData,
    max_dim: u32,
    mut progress: F,
) -> HdrTextureData {
    if data.width <= max_dim && data.height <= max_dim {
        progress(1.0);
        return data;
    }
    let scale_x = data.width as f32 / max_dim as f32;
    let scale_y = data.height as f32 / max_dim as f32;
    let scale = scale_x.max(scale_y);
    let new_width = ((data.width as f32) / scale).floor().max(1.0) as u32;
    let new_height = ((data.height as f32) / scale).floor().max(1.0) as u32;
    let mut out = vec![0_u16; (new_width * new_height * 4) as usize];

    for y in 0..new_height {
        if y % 32 == 0 {
            progress(y as f32 / new_height as f32);
        }
        let src_y = (y as u64 * data.height as u64 / new_height as u64) as u32;
        for x in 0..new_width {
            let src_x = (x as u64 * data.width as u64 / new_width as u64) as u32;
            let src_i = ((src_y * data.width + src_x) * 4) as usize;
            let dst_i = ((y * new_width + x) * 4) as usize;
            out[dst_i..dst_i + 4].copy_from_slice(&data.pixels[src_i..src_i + 4]);
        }
    }

    progress(1.0);
    HdrTextureData {
        pixels: out,
        width: new_width,
        height: new_height,
    }
}

pub(super) fn spawn_hdr_loader(path: PathBuf, max_texture_dimension_2d: u32) -> Receiver<HdrLoadMessage> {
    let (tx, rx) = mpsc::channel();
    thread::spawn(move || {
        let _ = tx.send(HdrLoadMessage::Progress {
            value: 0.02,
            status: "Decoding HDR...".to_string(),
        });
        let loaded = match load_hdr_texture_data(&path) {
            Ok(data) => data,
            Err(err) => {
                let _ = tx.send(HdrLoadMessage::Failed(err));
                return;
            }
        };

        let _ = tx.send(HdrLoadMessage::Progress {
            value: 0.55,
            status: "Resizing for GPU limits...".to_string(),
        });
        let scaled = downscale_hdr_rgba16f_with_progress(loaded, max_texture_dimension_2d, |p| {
            let _ = tx.send(HdrLoadMessage::Progress {
                value: 0.55 + 0.40 * p.clamp(0.0, 1.0),
                status: "Resizing for GPU limits...".to_string(),
            });
        });

        let _ = tx.send(HdrLoadMessage::Progress {
            value: 1.0,
            status: "HDR ready, uploading...".to_string(),
        });
        let _ = tx.send(HdrLoadMessage::Done(scaled));
    });

    rx
}

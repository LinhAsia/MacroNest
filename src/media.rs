use anyhow::{Context, Result, bail};

use crate::model::RgbaColor;
use crate::overlay::UiCommand;

#[derive(Debug, Clone, Copy, Default)]
pub struct VideoMetadata {
    pub width: i32,
    pub height: i32,
    pub duration_ms: u64,
    pub fps: f64,
}

#[derive(Debug, Clone, Default)]
pub struct VideoPreviewFrame {
    pub width: usize,
    pub height: usize,
    pub rgba: Vec<u8>,
}

#[derive(Debug, Clone)]
pub struct VideoFrameRequest {
    pub preset_id: u32,
    pub path: String,
    pub start_ms: u64,
    pub max_width: i32,
    pub max_height: i32,
    pub is_playing: bool,
}

#[cfg(windows)]
use opencv::{
    core::{Mat, Size},
    imgproc,
    prelude::*,
    videoio::{
        CAP_ANY, CAP_FFMPEG, CAP_PROP_FPS, CAP_PROP_FRAME_COUNT, CAP_PROP_FRAME_HEIGHT,
        CAP_PROP_FRAME_WIDTH, CAP_PROP_POS_MSEC, VideoCapture,
    },
};

#[cfg(windows)]
pub fn load_video_metadata(path: &str) -> Result<VideoMetadata> {
    let capture = open_video_capture_with_backend(path)?;
    let width = capture.get(CAP_PROP_FRAME_WIDTH)?.round() as i32;
    let height = capture.get(CAP_PROP_FRAME_HEIGHT)?.round() as i32;
    let fps = capture.get(CAP_PROP_FPS)?;
    let frame_count = capture.get(CAP_PROP_FRAME_COUNT)?;
    let duration_ms =
        if fps.is_finite() && fps > 0.0 && frame_count.is_finite() && frame_count > 0.0 {
            ((frame_count / fps) * 1000.0).round().max(0.0) as u64
        } else {
            0
        };

    Ok(VideoMetadata {
        width,
        height,
        duration_ms,
        fps,
    })
}

#[cfg(not(windows))]
pub fn load_video_metadata(_path: &str) -> Result<VideoMetadata> {
    bail!("Video playback is only supported on Windows")
}

#[cfg(windows)]
pub fn open_video_capture(path: &str, start_ms: u64) -> Result<(VideoCapture, VideoMetadata)> {
    let mut capture = open_video_capture_with_backend(path)?;
    let metadata = VideoMetadata {
        width: capture.get(CAP_PROP_FRAME_WIDTH)?.round() as i32,
        height: capture.get(CAP_PROP_FRAME_HEIGHT)?.round() as i32,
        fps: capture.get(CAP_PROP_FPS)?,
        duration_ms: {
            let fps = capture.get(CAP_PROP_FPS)?;
            let frame_count = capture.get(CAP_PROP_FRAME_COUNT)?;
            if fps.is_finite() && fps > 0.0 && frame_count.is_finite() && frame_count > 0.0 {
                ((frame_count / fps) * 1000.0).round().max(0.0) as u64
            } else {
                0
            }
        },
    };
    if start_ms > 0 {
        let _ = capture.set(CAP_PROP_POS_MSEC, start_ms as f64);
    }
    Ok((capture, metadata))
}

#[cfg(not(windows))]
pub fn open_video_capture(_path: &str, _start_ms: u64) -> Result<((), VideoMetadata)> {
    bail!("Video playback is only supported on Windows")
}

#[cfg(windows)]
fn open_video_capture_with_backend(path: &str) -> Result<VideoCapture> {
    let mut last_error = None;
    for backend in [CAP_FFMPEG, CAP_ANY] {
        match VideoCapture::from_file(path, backend)
            .with_context(|| format!("Open video with backend {backend}: {path}"))
        {
            Ok(capture) => {
                if capture.is_opened()? {
                    return Ok(capture);
                }
                last_error = Some(anyhow::anyhow!(
                    "Video backend {backend} did not open the file"
                ));
            }
            Err(error) => last_error = Some(error),
        }
    }
    Err(last_error.unwrap_or_else(|| anyhow::anyhow!("Video could not be opened")))
}

#[cfg(windows)]
pub fn frame_to_premultiplied_bgra(
    frame: &Mat,
    chroma_key: Option<(RgbaColor, u8)>,
    resolution: &str,
) -> Result<(Vec<u8>, i32, i32)> {
    let source_size = frame.size()?;
    let native_w = source_size.width;
    let native_h = source_size.height;

    let max_dim = match resolution {
        "360p" => Some(360),
        "720p" => Some(720),
        "1080p" => Some(1080),
        _ => None,
    };

    let mut temp_frame = frame.try_clone()?;

    if let Some(limit_h) = max_dim {
        if native_h > limit_h {
            let scale = limit_h as f32 / native_h as f32;
            let temp_w = ((native_w as f32 * scale).round() as i32).max(1);
            let temp_h = limit_h;
            let mut downscaled = Mat::default();
            imgproc::resize(
                frame,
                &mut downscaled,
                Size::new(temp_w, temp_h),
                0.0,
                0.0,
                imgproc::INTER_LINEAR,
            )?;
            temp_frame = downscaled;
        }
    }

    // 1. Convert source frame to BGRA first at its native resolution
    let mut bgra = Mat::default();
    match temp_frame.channels() {
        4 => bgra = temp_frame.try_clone()?,
        3 => imgproc::cvt_color(&temp_frame, &mut bgra, imgproc::COLOR_BGR2BGRA, 0)?,
        1 => imgproc::cvt_color(&temp_frame, &mut bgra, imgproc::COLOR_GRAY2BGRA, 0)?,
        channels => bail!("Unsupported frame channels: {channels}"),
    }

    if !bgra.is_continuous() {
        bgra = bgra.try_clone()?;
    }

    // 2. Apply Chroma key & Alpha Premultiplication at native resolution (drastically reducing pixel count)
    let pixels = bgra.data_bytes_mut()?;
    let tolerance = chroma_key.map(|(_, tol)| tol as i32).unwrap_or(0);
    for chunk in pixels.chunks_exact_mut(4) {
        let b = chunk[0];
        let g = chunk[1];
        let r = chunk[2];
        let mut alpha = chunk[3];

        if let Some((key, _)) = chroma_key {
            let dr = (r as i32 - key.r as i32).abs();
            let dg = (g as i32 - key.g as i32).abs();
            let db = (b as i32 - key.b as i32).abs();
            if dr <= tolerance && dg <= tolerance && db <= tolerance {
                alpha = 0;
            }
        }

        chunk[3] = alpha;
        chunk[0] = ((b as u32 * alpha as u32) / 255) as u8;
        chunk[1] = ((g as u32 * alpha as u32) / 255) as u8;
        chunk[2] = ((r as u32 * alpha as u32) / 255) as u8;
    }

    let final_size = bgra.size()?;
    let final_pixels = bgra.data_bytes()?;
    Ok((final_pixels.to_vec(), final_size.width, final_size.height))
}

#[cfg(not(windows))]
pub fn frame_to_premultiplied_bgra(
    _frame: &(),
    _chroma_key: Option<(RgbaColor, u8)>,
    _resolution: &str,
) -> Result<(Vec<u8>, i32, i32)> {
    bail!("Video playback is only supported on Windows")
}

#[cfg(windows)]
pub fn decode_video_preview_frame(
    capture: &mut VideoCapture,
    start_ms: u64,
    max_width: i32,
    max_height: i32,
    is_playing: bool,
) -> Result<VideoPreviewFrame> {
    let mut frame = Mat::default();
    let current_pos = capture.get(CAP_PROP_POS_MSEC).unwrap_or(0.0).round().max(0.0) as u64;

    let diff = (start_ms as i64 - current_pos as i64).abs();
    let should_seek = if is_playing {
        diff >= 300
    } else {
        diff >= 30
    };

    if should_seek {
        if start_ms > 0 {
            let _ = capture.set(CAP_PROP_POS_MSEC, start_ms as f64);
        }
        if !capture.read(&mut frame)? || frame.empty() {
            bail!("Video preview frame could not be read");
        }
    } else {
        let fps = capture.get(CAP_PROP_FPS).unwrap_or(30.0);
        let frame_duration = if fps.is_finite() && fps > 0.0 {
            (1000.0 / fps) as u64
        } else {
            33
        };

        let mut temp_pos = current_pos;
        let mut read_count = 0;
        loop {
            let mut f = Mat::default();
            if !capture.read(&mut f)? || f.empty() {
                break;
            }
            frame = f;
            read_count += 1;
            temp_pos += frame_duration;
            if temp_pos >= start_ms {
                break;
            }
            if read_count >= 5 {
                break;
            }
        }
        if frame.empty() {
            bail!("Video preview frame could not be read");
        }
    }

    let source_size = frame.size()?;
    let src_w = source_size.width.max(1);
    let src_h = source_size.height.max(1);
    let scale = ((max_width.max(1) as f32 / src_w as f32)
        .min(max_height.max(1) as f32 / src_h as f32))
    .min(1.0);
    let dst_w = ((src_w as f32 * scale).round() as i32).max(1);
    let dst_h = ((src_h as f32 * scale).round() as i32).max(1);

    let mut resized = Mat::default();
    if dst_w != src_w || dst_h != src_h {
        imgproc::resize(
            &frame,
            &mut resized,
            Size::new(dst_w, dst_h),
            0.0,
            0.0,
            imgproc::INTER_LINEAR,
        )?;
    } else {
        resized = frame.try_clone()?;
    }

    let mut rgba = Mat::default();
    match resized.channels() {
        4 => imgproc::cvt_color(&resized, &mut rgba, imgproc::COLOR_BGRA2RGBA, 0)?,
        3 => imgproc::cvt_color(&resized, &mut rgba, imgproc::COLOR_BGR2RGBA, 0)?,
        1 => imgproc::cvt_color(&resized, &mut rgba, imgproc::COLOR_GRAY2RGBA, 0)?,
        channels => bail!("Unsupported preview frame channels: {channels}"),
    }
    if !rgba.is_continuous() {
        rgba = rgba.try_clone()?;
    }
    let bytes = rgba.data_bytes()?.to_vec();
    Ok(VideoPreviewFrame {
        width: dst_w as usize,
        height: dst_h as usize,
        rgba: bytes,
    })
}

#[cfg(windows)]
pub fn load_video_preview_frame(
    path: &str,
    start_ms: u64,
    max_width: i32,
    max_height: i32,
) -> Result<VideoPreviewFrame> {
    let (mut capture, _) = open_video_capture(path, start_ms)?;
    decode_video_preview_frame(&mut capture, 0, max_width, max_height, false)
}

#[cfg(not(windows))]
pub fn decode_video_preview_frame(
    _capture: &mut (),
    _start_ms: u64,
    _max_width: i32,
    _max_height: i32,
    _is_playing: bool,
) -> Result<VideoPreviewFrame> {
    bail!("Video playback is only supported on Windows")
}

#[cfg(not(windows))]
pub fn load_video_preview_frame(
    _path: &str,
    _start_ms: u64,
    _max_width: i32,
    _max_height: i32,
) -> Result<VideoPreviewFrame> {
    bail!("Video playback is only supported on Windows")
}

#[cfg(windows)]
pub fn start_video_preview_worker(
    ui_tx: crossbeam_channel::Sender<UiCommand>,
) -> crossbeam_channel::Sender<VideoFrameRequest> {
    let (tx, rx) = crossbeam_channel::unbounded::<VideoFrameRequest>();
    std::thread::spawn(move || {
        let mut cached_capture: Option<(String, VideoCapture)> = None;

        while let Ok(request) = rx.recv() {
            let mut requests = std::collections::HashMap::new();
            requests.insert(request.preset_id, request);
            while let Ok(next_req) = rx.try_recv() {
                requests.insert(next_req.preset_id, next_req);
            }

            for (_, req) in requests {
                let capture_ok = if let Some((ref cached_path, ref mut capture)) = cached_capture {
                    if cached_path == &req.path {
                        capture.is_opened().unwrap_or(false)
                    } else {
                        false
                    }
                } else {
                    false
                };

                if !capture_ok {
                    if let Ok(capture) = open_video_capture_with_backend(&req.path) {
                        cached_capture = Some((req.path.clone(), capture));
                    } else {
                        cached_capture = None;
                    }
                }

                if let Some((_, ref mut capture)) = cached_capture {
                    if let Ok(preview) = decode_video_preview_frame(
                        capture,
                        req.start_ms,
                        req.max_width,
                        req.max_height,
                        req.is_playing,
                    ) {
                        let _ = ui_tx.send(UiCommand::VideoFrameLoaded {
                            preset_id: req.preset_id,
                            path: req.path,
                            start_ms: req.start_ms,
                            max_width: req.max_width,
                            max_height: req.max_height,
                            width: preview.width,
                            height: preview.height,
                            rgba: preview.rgba,
                        });
                    }
                }
            }
        }
    });
    tx
}

#[cfg(not(windows))]
pub fn start_video_preview_worker(
    _ui_tx: crossbeam_channel::Sender<UiCommand>,
) -> crossbeam_channel::Sender<VideoFrameRequest> {
    let (tx, _) = crossbeam_channel::unbounded::<VideoFrameRequest>();
    tx
}

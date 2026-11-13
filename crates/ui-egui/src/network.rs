//! Network printing and the print camera: the windows and their background
//! work. Uploads and checks run as engine jobs on a thread; the camera fetches
//! a JPEG snapshot about once a second while its window is open.

use crate::app::SliceCraftApp;
use eframe::egui::{self, RichText};
use slicecraft_engine::{EngineError, Outcome};
use slicecraft_io::network::HostKind;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, mpsc};
use std::time::Duration;

const WARN: egui::Color32 = egui::Color32::from_rgb(0xff, 0x7a, 0x70);

/// Largest camera picture decoded, per side.
const MAX_CAMERA_SIDE: usize = 4096;

#[derive(Default)]
pub struct NetState {
    pub open: bool,
    pub camera_open: bool,
    /// The job name for uploads (empty: the document's name).
    pub name: String,
    job: Option<(&'static str, mpsc::Receiver<Result<Outcome, EngineError>>)>,
    /// The last answer: (ok, message).
    last: Option<(bool, String)>,
    feed: Option<Feed>,
    texture: Option<egui::TextureHandle>,
    camera_note: String,
}

struct Feed {
    stop: Arc<AtomicBool>,
    rx: mpsc::Receiver<Result<egui::ColorImage, String>>,
}

impl Drop for Feed {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
    }
}

/// Collect finished jobs and new camera pictures.
pub fn poll(app: &mut SliceCraftApp, ctx: &egui::Context) {
    if let Some((what, rx)) = &app.net.job {
        let what = *what;
        let done = match rx.try_recv() {
            Ok(r) => Some(r),
            Err(mpsc::TryRecvError::Empty) => None,
            Err(mpsc::TryRecvError::Disconnected) => {
                Some(Err(EngineError::Internal("the network job stopped without an answer")))
            }
        };
        if let Some(r) = done {
            app.net.job = None;
            match r {
                Ok(Outcome::Network { message }) => {
                    app.net.last = Some((true, message.clone()));
                    app.info(message);
                }
                Ok(_) => app.net.last = Some((true, format!("{what}: done"))),
                Err(e) => {
                    app.net.last = Some((false, e.to_string()));
                    app.error(format!("{what}: {e}"));
                }
            }
        }
    }
    if !app.net.camera_open {
        app.net.feed = None;
        return;
    }
    let mut latest = None;
    if let Some(feed) = &app.net.feed {
        while let Ok(r) = feed.rx.try_recv() {
            latest = Some(r);
        }
    }
    match latest {
        Some(Ok(img)) => {
            app.net.camera_note.clear();
            match &mut app.net.texture {
                Some(t) => t.set(img, egui::TextureOptions::LINEAR),
                None => {
                    app.net.texture =
                        Some(ctx.load_texture("camera", img, egui::TextureOptions::LINEAR));
                }
            }
        }
        Some(Err(e)) => app.net.camera_note = e,
        None => {}
    }
}

fn start_job(
    app: &mut SliceCraftApp,
    ctx: &egui::Context,
    what: &'static str,
    job: slicecraft_engine::NetworkJob,
) {
    let (tx, rx) = mpsc::channel();
    let wake = ctx.clone();
    let spawned = std::thread::Builder::new().name("network".into()).spawn(move || {
        let _ = tx.send(job.run());
        wake.request_repaint();
    });
    match spawned {
        Ok(_) => {
            app.net.job = Some((what, rx));
            app.net.last = None;
            app.info(format!("{what}…"));
        }
        Err(e) => app.error(format!("Cannot start {what}: {e}")),
    }
}

pub fn network_window(app: &mut SliceCraftApp, ctx: &egui::Context) {
    if !app.net.open {
        return;
    }
    let mut open = true;
    let mut action: Option<(&'static str, bool)> = None;
    let sliced = app.session.slice_result().is_some();
    let busy = app.net.job.as_ref().map(|(w, _)| *w);
    egui::Window::new(crate::i18n::tr("Network printer"))
        .open(&mut open)
        .default_width(440.0)
        .show(ctx, |ui| {
            let p = &mut app.prefs.network;
            egui::Grid::new("net").num_columns(2).spacing([10.0, 6.0]).show(ui, |ui| {
                ui.label(crate::i18n::tr("Host software"));
                egui::ComboBox::from_id_salt("net-kind").selected_text(p.kind.label()).show_ui(
                    ui,
                    |ui| {
                        for k in HostKind::ALL {
                            ui.selectable_value(&mut p.kind, k, k.label());
                        }
                    },
                );
                ui.end_row();
                ui.label(crate::i18n::tr("Address"));
                ui.add(
                    egui::TextEdit::singleline(&mut p.url)
                        .hint_text("http://printer.local")
                        .desired_width(260.0),
                );
                ui.end_row();
                ui.label(crate::i18n::tr("API key"));
                ui.add(
                    egui::TextEdit::singleline(&mut p.api_key)
                        .password(true)
                        .hint_text(match p.kind {
                            HostKind::OctoPrint => "from the host's settings",
                            HostKind::Moonraker => "only if the host asks for one",
                        })
                        .desired_width(260.0),
                );
                ui.end_row();
                ui.label(crate::i18n::tr("Job name"));
                ui.add(
                    egui::TextEdit::singleline(&mut app.net.name)
                        .hint_text(app.session.job_name())
                        .desired_width(260.0),
                );
                ui.end_row();
            });
            ui.weak(crate::i18n::tr(
                "Plain http:// on the local network. The key is kept in your preferences file.",
            ));
            ui.separator();
            ui.horizontal(|ui| {
                let free = busy.is_none();
                if ui
                    .add_enabled(
                        free && !p.url.trim().is_empty(),
                        egui::Button::new(crate::i18n::tr("Test connection")),
                    )
                    .clicked()
                {
                    action = Some(("Checking the printer", false));
                }
                let can = free && sliced && !p.url.trim().is_empty();
                if ui.add_enabled(can, egui::Button::new(crate::i18n::tr("Upload"))).clicked() {
                    action = Some(("Uploading", false));
                }
                if ui
                    .add_enabled(can, egui::Button::new(crate::i18n::tr("Upload and print")))
                    .clicked()
                {
                    action = Some(("Uploading", true));
                }
                if busy.is_some() {
                    ui.spinner();
                }
            });
            if !sliced {
                ui.weak(crate::i18n::tr("Slice the plate first to upload it."));
            }
            if let Some((ok, msg)) = &app.net.last {
                let text = RichText::new(msg);
                ui.label(if *ok { text } else { text.color(WARN) });
            }
        });
    app.net.open = open;
    let Some((what, start)) = action else { return };
    let printer = app.prefs.network.clone();
    app.persist();
    let job = if what == "Uploading" {
        let name = if app.net.name.trim().is_empty() {
            app.session.job_name()
        } else {
            app.net.name.clone()
        };
        app.session.upload_job(printer, name, start)
    } else {
        Some(app.session.check_job(printer))
    };
    match job {
        Some(job) => start_job(app, ctx, what, job),
        None => app.error("Slice the plate first."),
    }
}

pub fn camera_window(app: &mut SliceCraftApp, ctx: &egui::Context) {
    if !app.net.camera_open {
        return;
    }
    let mut open = true;
    let mut toggle = false;
    let running = app.net.feed.is_some();
    egui::Window::new(crate::i18n::tr("Print camera")).open(&mut open).default_width(520.0).show(
        ctx,
        |ui| {
            ui.horizontal(|ui| {
                ui.label(crate::i18n::tr("Snapshot address"));
                ui.add(
                    egui::TextEdit::singleline(&mut app.prefs.camera_url)
                        .hint_text("http://printer.local/webcam/?action=snapshot")
                        .desired_width(300.0),
                );
                if ui.button(if running { "Stop" } else { "Start" }).clicked() {
                    toggle = true;
                }
            });
            ui.weak(crate::i18n::tr("A JPEG snapshot address, refreshed about once a second."));
            if !app.net.camera_note.is_empty() {
                ui.label(RichText::new(&app.net.camera_note).color(WARN));
            }
            if let Some(t) = &app.net.texture {
                let w = ui.available_width();
                ui.add(
                    egui::Image::new(egui::load::SizedTexture::from_handle(t))
                        .max_width(w)
                        .maintain_aspect_ratio(true),
                );
            }
        },
    );
    app.net.camera_open = open;
    if !toggle {
        return;
    }
    if running {
        app.net.feed = None;
        return;
    }
    if let Err(e) = slicecraft_io::network::parse_url(&app.prefs.camera_url) {
        app.net.camera_note = e.to_string();
        return;
    }
    app.persist();
    let url = app.prefs.camera_url.clone();
    let stop = Arc::new(AtomicBool::new(false));
    let (tx, rx) = mpsc::channel();
    let flag = Arc::clone(&stop);
    let wake = ctx.clone();
    let spawned = std::thread::Builder::new().name("camera".into()).spawn(move || {
        while !flag.load(Ordering::Relaxed) {
            let r = slicecraft_io::network::fetch(&url, "")
                .map_err(|e| e.to_string())
                .and_then(|bytes| decode_jpeg(&bytes));
            let failed = r.is_err();
            if tx.send(r).is_err() {
                break;
            }
            wake.request_repaint();
            // a second between pictures, longer after a failure
            let waits = if failed { 50 } else { 10 };
            for _ in 0..waits {
                if flag.load(Ordering::Relaxed) {
                    break;
                }
                std::thread::sleep(Duration::from_millis(100));
            }
        }
    });
    match spawned {
        Ok(_) => {
            app.net.feed = Some(Feed { stop, rx });
            app.net.camera_note = "Connecting…".into();
        }
        Err(e) => app.net.camera_note = format!("Cannot start the camera: {e}"),
    }
}

/// Decode a JPEG into a picture, refusing huge or odd ones.
pub fn decode_jpeg(data: &[u8]) -> Result<egui::ColorImage, String> {
    use zune_jpeg::zune_core::bytestream::ZCursor;
    use zune_jpeg::zune_core::colorspace::ColorSpace;
    use zune_jpeg::zune_core::options::DecoderOptions;
    if !data.starts_with(&[0xFF, 0xD8]) {
        return Err("the camera address did not return a JPEG picture".into());
    }
    let options = DecoderOptions::default()
        .jpeg_set_out_colorspace(ColorSpace::RGBA)
        .set_max_width(MAX_CAMERA_SIDE)
        .set_max_height(MAX_CAMERA_SIDE);
    let mut d = zune_jpeg::JpegDecoder::new_with_options(ZCursor::new(data), options);
    d.decode_headers().map_err(|e| format!("the camera picture is damaged: {e:?}"))?;
    let info = d.info().ok_or("the camera picture has no size")?;
    let (w, h) = (usize::from(info.width), usize::from(info.height));
    let size = d.output_buffer_size().ok_or("the camera picture has no size")?;
    if w == 0 || h == 0 || size != w * h * 4 {
        return Err("the camera picture has an unexpected layout".into());
    }
    let mut out = vec![0u8; size];
    d.decode_into(&mut out).map_err(|e| format!("the camera picture is damaged: {e:?}"))?;
    Ok(egui::ColorImage::from_rgba_unmultiplied([w, h], &out))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn junk_is_not_a_picture() {
        assert!(decode_jpeg(b"").is_err());
        assert!(decode_jpeg(b"<html>").is_err());
        assert!(decode_jpeg(&[0xFF, 0xD8, 0xFF, 0xE0, 0, 3, 1]).is_err());
    }
}

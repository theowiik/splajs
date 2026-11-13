#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod theme;

use eframe::egui;
use rodio::Source;
use std::fs::File;
use std::io::BufReader;
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

const DEFAULT_ROOT: &str = r"D:\ppsync\music\_packs";
const AUDIO_EXT: &[&str] = &["wav", "mp3", "flac", "ogg", "aif", "aiff", "m4a"];

struct Entry {
    name: String,
    path: PathBuf,
    is_dir: bool,
}

struct App {
    root: PathBuf,
    /// Lazily loaded subfolders per folder, for the tree.
    tree_children: HashMap<PathBuf, Vec<(String, PathBuf)>>,
    expanded: HashSet<PathBuf>,
    tree_scroll_to_cwd: bool,
    cwd: PathBuf,
    entries: Vec<Entry>,
    filter: String,
    visible: Vec<usize>,
    selected: Option<usize>, // index into `visible`
    scroll_to_selected: bool,
    scroll_off: f32,
    _device: Option<rodio::MixerDeviceSink>,
    player: Option<rodio::Player>,
    playing: Option<PathBuf>,
    duration: Option<f32>,
    volume: f32,
    error: Option<String>,
}

fn is_audio(p: &Path) -> bool {
    p.extension()
        .and_then(|e| e.to_str())
        .is_some_and(|e| AUDIO_EXT.iter().any(|a| a.eq_ignore_ascii_case(e)))
}

impl App {
    fn new(root: PathBuf) -> Self {
        let (device, player, error) = match rodio::DeviceSinkBuilder::open_default_sink() {
            Ok(d) => {
                let p = rodio::Player::connect_new(d.mixer());
                (Some(d), Some(p), None)
            }
            Err(e) => (None, None, Some(format!("Audio device error: {e}"))),
        };
        let mut app = Self {
            root: root.clone(),
            tree_children: HashMap::new(),
            expanded: HashSet::new(),
            tree_scroll_to_cwd: false,
            cwd: root.clone(),
            entries: vec![],
            filter: String::new(),
            visible: vec![],
            selected: None,
            scroll_to_selected: false,
            scroll_off: 0.0,
            _device: device,
            player,
            playing: None,
            duration: None,
            volume: 0.8,
            error,
        };
        app.navigate(root);
        app
    }

    fn navigate(&mut self, dir: PathBuf) {
        let mut entries: Vec<Entry> = match std::fs::read_dir(&dir) {
            Ok(rd) => rd
                .flatten()
                .filter_map(|e| {
                    let path = e.path();
                    let is_dir = e.file_type().ok()?.is_dir();
                    if !is_dir && !is_audio(&path) {
                        return None;
                    }
                    Some(Entry { name: e.file_name().to_string_lossy().into_owned(), path, is_dir })
                })
                .collect(),
            Err(e) => {
                self.error = Some(format!("{}: {e}", dir.display()));
                return;
            }
        };
        entries.sort_by(|a, b| {
            b.is_dir.cmp(&a.is_dir).then_with(|| a.name.to_lowercase().cmp(&b.name.to_lowercase()))
        });
        // Expand the tree down to the new folder.
        for a in dir.ancestors().skip(1) {
            self.expanded.insert(a.to_path_buf());
            if a == self.root { break; }
        }
        self.tree_scroll_to_cwd = true;
        let came_from = std::mem::replace(&mut self.cwd, dir);
        self.entries = entries;
        self.filter.clear();
        self.refilter();
        // When going up, select the folder we came from.
        self.selected = self
            .visible
            .iter()
            .position(|&i| self.entries[i].path == came_from)
            .or(if self.visible.is_empty() { None } else { Some(0) });
        self.scroll_to_selected = true;
    }

    fn refilter(&mut self) {
        let f = self.filter.to_lowercase();
        self.visible = (0..self.entries.len())
            .filter(|&i| f.is_empty() || self.entries[i].name.to_lowercase().contains(&f))
            .collect();
        self.selected = if self.visible.is_empty() { None } else { Some(0) };
    }

    fn play(&mut self, path: &Path) {
        let Some(player) = &self.player else { return };
        player.stop();
        match File::open(path).map_err(|e| e.to_string()).and_then(|f| {
            rodio::Decoder::try_from(BufReader::new(f)).map_err(|e| e.to_string())
        }) {
            Ok(src) => {
                player.set_volume(self.volume);
                self.duration = src.total_duration().map(|d| d.as_secs_f32());
                player.append(src);
                player.play();
                self.playing = Some(path.to_path_buf());
                self.error = None;
            }
            Err(e) => self.error = Some(format!("Can't play {}: {e}", path.display())),
        }
    }

    fn stop(&mut self) {
        if let Some(p) = &self.player {
            p.stop();
        }
        self.playing = None;
    }

    fn selected_entry(&self) -> Option<&Entry> {
        self.selected.map(|s| &self.entries[self.visible[s]])
    }

    fn select(&mut self, idx: usize) {
        self.selected = Some(idx);
        self.scroll_to_selected = true;
        if let Some(e) = self.selected_entry() {
            if !e.is_dir {
                let p = e.path.clone();
                self.play(&p);
            }
        }
    }

    fn activate(&mut self) {
        if let Some(e) = self.selected_entry() {
            let (p, d) = (e.path.clone(), e.is_dir);
            if d { self.navigate(p) } else { self.play(&p) }
        }
    }

    fn subdirs(&mut self, dir: &Path) -> Vec<(String, PathBuf)> {
        self.tree_children
            .entry(dir.to_path_buf())
            .or_insert_with(|| {
                let mut v: Vec<(String, PathBuf)> = std::fs::read_dir(dir)
                    .map(|rd| {
                        rd.flatten()
                            .filter(|e| e.file_type().is_ok_and(|t| t.is_dir()))
                            .map(|e| (e.file_name().to_string_lossy().into_owned(), e.path()))
                            .collect()
                    })
                    .unwrap_or_default();
                v.sort_by_key(|(n, _)| n.to_lowercase());
                v
            })
            .clone()
    }

    fn tree_rows(&mut self, ui: &mut egui::Ui, name: &str, path: PathBuf, depth: usize, nav: &mut Option<PathBuf>) {
        use theme::*;
        let children = self.subdirs(&path);
        let open = self.expanded.contains(&path);
        let is_cwd = path == self.cwd;
        let (rect, r) = ui.allocate_exact_size(egui::vec2(ui.available_width(), 24.0), egui::Sense::click());
        let p = ui.painter_at(rect);
        if is_cwd {
            p.rect_filled(rect, 0.0, PAPER);
            p.rect_filled(egui::Rect::from_min_size(rect.min, egui::vec2(3.0, rect.height())), 0.0, ORANGE);
        } else if r.hovered() {
            p.rect_filled(rect, 0.0, HOVER);
        }
        let x = rect.left() + 12.0 + depth as f32 * 14.0;
        let cy = rect.center().y;
        let chevron_hit = egui::Rect::from_center_size(egui::pos2(x + 4.0, cy), egui::vec2(18.0, 24.0));
        if !children.is_empty() {
            chevron(&p, egui::pos2(x + 4.0, cy), open, if is_cwd { INK } else { MUTED });
        }
        let font = if is_cwd { bold(13.0) } else { egui::FontId::proportional(13.0) };
        p.text(egui::pos2(x + 16.0, cy), egui::Align2::LEFT_CENTER, name, font, if is_cwd { INK } else { INK_SOFT });
        if is_cwd && self.tree_scroll_to_cwd {
            r.scroll_to_me(None);
            self.tree_scroll_to_cwd = false;
        }
        if r.hovered() { ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand); }
        let on_chevron = r.interact_pointer_pos().is_some_and(|pos| chevron_hit.contains(pos));
        if (r.clicked() && on_chevron && !children.is_empty()) || r.double_clicked() {
            if open { self.expanded.remove(&path); } else { self.expanded.insert(path.clone()); }
        } else if r.clicked() {
            *nav = Some(path.clone());
        }
        if open {
            for (n, c) in children {
                self.tree_rows(ui, &n, c, depth + 1, nav);
            }
        }
    }

    fn go_up(&mut self) {
        if let Some(parent) = self.cwd.parent() {
            self.navigate(parent.to_path_buf());
        }
    }

    fn handle_keys(&mut self, ctx: &egui::Context) {
        let n = self.visible.len();
        let (down, up, pgdn, pgup, home, end, enter, back, space, esc) = ctx.input_mut(|i| {
            use egui::{Key, Modifiers as M};
            (
                i.consume_key(M::NONE, Key::ArrowDown),
                i.consume_key(M::NONE, Key::ArrowUp),
                i.consume_key(M::NONE, Key::PageDown),
                i.consume_key(M::NONE, Key::PageUp),
                i.consume_key(M::NONE, Key::Home),
                i.consume_key(M::NONE, Key::End),
                i.consume_key(M::NONE, Key::Enter) || i.consume_key(M::NONE, Key::ArrowRight),
                i.consume_key(M::NONE, Key::ArrowLeft)
                    || (self.filter.is_empty() && i.consume_key(M::NONE, Key::Backspace)),
                i.consume_key(M::CTRL, Key::Space) || (self.filter.is_empty() && i.consume_key(M::NONE, Key::Space)),
                i.consume_key(M::NONE, Key::Escape),
            )
        });
        if n > 0 {
            let cur = self.selected.unwrap_or(0);
            let target = if down { Some((cur + 1).min(n - 1)) }
                else if up { Some(cur.saturating_sub(1)) }
                else if pgdn { Some((cur + 20).min(n - 1)) }
                else if pgup { Some(cur.saturating_sub(20)) }
                else if home { Some(0) }
                else if end { Some(n - 1) }
                else { None };
            if let Some(t) = target {
                if Some(t) != self.selected || up || down {
                    self.select(t);
                }
            }
        }
        if enter { self.activate() }
        if back { self.go_up() }
        if space {
            if let Some(e) = self.selected_entry() {
                if !e.is_dir {
                    let p = e.path.clone();
                    self.play(&p);
                }
            }
        }
        if esc {
            if self.filter.is_empty() { self.stop() } else { self.filter.clear(); self.refilter(); }
        }
    }
}

impl eframe::App for App {
    fn ui(&mut self, root_ui: &mut egui::Ui, frame: &mut eframe::Frame) {
        use theme::*;
        let ctx = &root_ui.ctx().clone();
        self.handle_keys(ctx);

        // ── Header: wordmark, nav keys, breadcrumbs, filter ──────────────────
        egui::Panel::top("top")
            .show_separator_line(false)
            .frame(egui::Frame::new().fill(PAPER).inner_margin(egui::Margin { left: 14, right: 14, top: 12, bottom: 10 }))
            .show(root_ui, |ui| {
                ui.horizontal(|ui| {
                    let (dot, _) = ui.allocate_exact_size(egui::vec2(12.0, 24.0), egui::Sense::hover());
                    ui.painter().rect_filled(egui::Rect::from_center_size(dot.center(), egui::vec2(10.0, 10.0)), 0.0, ORANGE);
                    ui.label(egui::RichText::new("SPLAJS").font(bold(15.0)));
                    ui.add_space(10.0);
                    if key(ui, "↑", 28.0).on_hover_text("Parent  (← / Backspace)").clicked() {
                        self.go_up();
                    }
                    if key(ui, "OPEN", 52.0).on_hover_text("Pick a root folder").clicked() {
                        if let Some(d) = rfd::FileDialog::new().set_directory(&self.cwd).pick_folder() {
                            self.root = d.clone();
                            self.navigate(d);
                        }
                    }
                    ui.add_space(10.0);
                    let mut nav = None;
                    let comps: Vec<PathBuf> = self.cwd.ancestors().map(Path::to_path_buf).collect();
                    let last = comps.len() - 1;
                    for (i, p) in comps.iter().rev().enumerate() {
                        let label = p.file_name().map(|s| s.to_string_lossy().into_owned())
                            .unwrap_or_else(|| p.display().to_string().trim_end_matches('\\').to_string());
                        if i > 0 { ui.label(egui::RichText::new("/").color(LINE)); }
                        let txt = if i == last {
                            egui::RichText::new(label).font(bold(13.0)).color(INK)
                        } else {
                            egui::RichText::new(label).color(MUTED)
                        };
                        let r = ui.add(egui::Label::new(txt).sense(egui::Sense::click()));
                        if r.hovered() { ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand); }
                        if r.clicked() { nav = Some(p.clone()); }
                    }
                    if let Some(p) = nav { self.navigate(p); }
                });
                ui.add_space(12.0);
                ui.horizontal(|ui| {
                    label(ui, "FIND");
                    let count = format!("{:03} ITEMS", self.visible.len());
                    let r = ui.add(
                        egui::TextEdit::singleline(&mut self.filter)
                            .frame(egui::Frame::NONE)
                            .hint_text(egui::RichText::new("type to filter").color(MUTED))
                            .desired_width(ui.available_width() - 90.0),
                    );
                    let y = r.rect.bottom() + 3.0;
                    ui.painter().hline(r.rect.left()..=r.rect.right(), y,
                        egui::Stroke::new(if r.has_focus() { 1.5 } else { 1.0 }, if r.has_focus() { INK } else { LINE }));
                    if r.changed() { self.refilter(); }
                    if !r.has_focus() && !ctx.egui_wants_keyboard_input() { r.request_focus(); }
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| label(ui, &count));
                });
            });

        // ── Transport bar ─────────────────────────────────────────────────────
        egui::Panel::bottom("bottom")
            .show_separator_line(false)
            .exact_size(48.0)
            .frame(egui::Frame::new().fill(INK).inner_margin(egui::Margin::symmetric(14, 0)))
            .show(root_ui, |ui| {
                let busy = self.player.as_ref().is_some_and(|p| !p.empty());
                if busy { ctx.request_repaint(); }

                // Progress line along the top edge.
                let full = ui.max_rect();
                if let (true, Some(p), Some(d)) = (busy, &self.player, self.duration) {
                    let t = (p.get_pos().as_secs_f32() / d.max(0.001)).clamp(0.0, 1.0);
                    let bar = egui::Rect::from_min_size(
                        egui::pos2(full.left() - 14.0, full.top()),
                        egui::vec2((full.width() + 28.0) * t, 2.0),
                    );
                    ui.painter().rect_filled(bar, 0.0, ORANGE);
                }

                ui.horizontal_centered(|ui| {
                    // Big round play/stop button.
                    let (rect, r) = ui.allocate_exact_size(egui::vec2(30.0, 30.0), egui::Sense::click());
                    let p = ui.painter();
                    let c = rect.center();
                    p.circle_filled(c, 15.0, if r.hovered() { ORANGE.gamma_multiply(0.85) } else { ORANGE });
                    if busy {
                        p.rect_filled(egui::Rect::from_center_size(c, egui::vec2(9.0, 9.0)), 1.0, INK);
                    } else {
                        p.add(egui::Shape::convex_polygon(
                            vec![c + egui::vec2(-3.5, -6.0), c + egui::vec2(6.5, 0.0), c + egui::vec2(-3.5, 6.0)],
                            INK, egui::Stroke::NONE));
                    }
                    if r.hovered() { ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand); }
                    if r.clicked() { if busy { self.stop() } else { self.activate() } }

                    ui.add_space(10.0);
                    ui.label(egui::RichText::new("VOL").font(bold(10.0)).color(MUTED));
                    // Segmented volume meter.
                    let (vr, resp) = ui.allocate_exact_size(egui::vec2(110.0, 14.0), egui::Sense::click_and_drag());
                    if let Some(pos) = resp.interact_pointer_pos() {
                        self.volume = ((pos.x - vr.left()) / vr.width()).clamp(0.0, 1.0);
                        if let Some(p) = &self.player { p.set_volume(self.volume); }
                    }
                    let segs = 16;
                    let w = vr.width() / segs as f32;
                    for i in 0..segs {
                        let on = (i as f32 + 0.5) / segs as f32 <= self.volume;
                        let sr = egui::Rect::from_min_size(egui::pos2(vr.left() + i as f32 * w, vr.top()), egui::vec2(w - 2.0, vr.height()));
                        ui.painter().rect_filled(sr, 0.0, if on { PAPER } else { INK_SOFT });
                    }

                    ui.add_space(14.0);
                    let status_w = ui.available_width() - 330.0;
                    let (sr, _) = ui.allocate_exact_size(egui::vec2(status_w.max(0.0), 30.0), egui::Sense::hover());
                    let sp = ui.painter_at(sr);
                    let lc = egui::pos2(sr.left(), sr.center().y);
                    if let Some(e) = &self.error {
                        sp.text(lc, egui::Align2::LEFT_CENTER, e, egui::FontId::proportional(12.0), ORANGE);
                    } else if let (true, Some(path)) = (busy, &self.playing) {
                        let name = path.file_name().unwrap_or_default().to_string_lossy();
                        let pos = self.player.as_ref().map(|p| p.get_pos().as_secs_f32()).unwrap_or(0.0);
                        let time = match self.duration {
                            Some(d) => format!("{pos:05.2} / {d:05.2}"),
                            None => format!("{pos:05.2}"),
                        };
                        let g = sp.text(lc, egui::Align2::LEFT_CENTER, time, bold(12.0), ORANGE);
                        sp.text(egui::pos2(g.right() + 12.0, lc.y), egui::Align2::LEFT_CENTER, name, egui::FontId::proportional(12.0), PAPER);
                    } else {
                        sp.text(lc, egui::Align2::LEFT_CENTER, "READY", bold(12.0), MUTED);
                    }

                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        for (k, what) in [("ESC", "STOP"), ("SPACE", "REPLAY"), ("←", "UP"), ("ENTER", "OPEN"), ("↑↓", "PLAY")] {
                            ui.label(egui::RichText::new(what).font(egui::FontId::proportional(10.0)).color(MUTED));
                            let g = ui.painter().layout_no_wrap(k.into(), bold(10.0), PAPER);
                            let (kr, _) = ui.allocate_exact_size(g.size() + egui::vec2(8.0, 6.0), egui::Sense::hover());
                            ui.painter().rect_stroke(kr, 2.0, egui::Stroke::new(1.0, MUTED), egui::StrokeKind::Inside);
                            ui.painter().galley(kr.center() - g.size() / 2.0, g, PAPER);
                            ui.add_space(6.0);
                        }
                    });
                });
            });

        // ── Folder tree ───────────────────────────────────────────────────────
        egui::Panel::left("tree")
            .resizable(true)
            .default_size(260.0)
            .show_separator_line(false)
            .frame(egui::Frame::new().fill(PANEL).inner_margin(egui::Margin { left: 0, right: 0, top: 10, bottom: 6 }))
            .show(root_ui, |ui| {
                ui.horizontal(|ui| { ui.add_space(12.0); label(ui, "BROWSE"); });
                ui.add_space(6.0);
                let mut nav = None;
                egui::ScrollArea::vertical().auto_shrink(false).show(ui, |ui| {
                    let root = self.root.clone();
                    let name = root.file_name().map(|s| s.to_string_lossy().into_owned()).unwrap_or_else(|| root.display().to_string());
                    self.tree_rows(ui, &name, root, 0, &mut nav);
                });
                if let Some(p) = nav { self.navigate(p); }
            });

        // ── File list ─────────────────────────────────────────────────────────
        egui::CentralPanel::default()
            .frame(egui::Frame::new().fill(PAPER).inner_margin(egui::Margin { left: 0, right: 0, top: 4, bottom: 0 }))
            .show(root_ui, |ui| {
                let row_h = 26.0;
                let pad = 16.0;
                // Column header
                let (hr, _) = ui.allocate_exact_size(egui::vec2(ui.available_width(), 22.0), egui::Sense::hover());
                let hp = ui.painter();
                hp.text(egui::pos2(hr.left() + pad, hr.center().y), egui::Align2::LEFT_CENTER, "NO.", bold(10.0), MUTED);
                hp.text(egui::pos2(hr.left() + pad + 48.0, hr.center().y), egui::Align2::LEFT_CENTER, "NAME", bold(10.0), MUTED);
                hp.text(egui::pos2(hr.right() - pad, hr.center().y), egui::Align2::RIGHT_CENTER, "TYPE", bold(10.0), MUTED);
                hp.hline(hr.left()..=hr.right(), hr.bottom(), egui::Stroke::new(1.0, INK));

                let mut clicked = None;
                let mut dbl = None;
                let mut drag_out = None;
                let scroll_target = self.scroll_to_selected.then_some(self.selected).flatten();
                self.scroll_to_selected = false;
                let mut area = egui::ScrollArea::vertical().auto_shrink(false);
                if let Some(s) = scroll_target {
                    let viewport_h = ui.available_height();
                    let y = s as f32 * row_h;
                    let off = self.scroll_off;
                    if y < off { area = area.vertical_scroll_offset(y); }
                    else if y + row_h > off + viewport_h { area = area.vertical_scroll_offset(y + row_h - viewport_h); }
                }
                let out = area.show_rows(ui, row_h, self.visible.len(), |ui, range| {
                    for vi in range {
                        let e = &self.entries[self.visible[vi]];
                        let (rect, r) = ui.allocate_exact_size(egui::vec2(ui.available_width(), row_h), egui::Sense::click_and_drag());
                        let p = ui.painter_at(rect);
                        let selected = self.selected == Some(vi);
                        let playing = self.playing.as_deref() == Some(&e.path);
                        if selected { p.rect_filled(rect, 0.0, INK); }
                        else if r.hovered() { p.rect_filled(rect, 0.0, HOVER); }
                        p.hline(rect.left() + pad..=rect.right() - pad, rect.bottom() - 0.5, egui::Stroke::new(0.5, LINE));

                        let cy = rect.center().y;
                        let (fg, dim) = if selected { (PAPER, MUTED) } else { (INK, MUTED) };
                        let idx_col = if playing { ORANGE } else { dim };
                        p.text(egui::pos2(rect.left() + pad, cy), egui::Align2::LEFT_CENTER, format!("{:03}", vi + 1), egui::FontId::proportional(11.0), idx_col);
                        if playing {
                            p.rect_filled(egui::Rect::from_center_size(egui::pos2(rect.left() + 6.0, cy), egui::vec2(4.0, 12.0)), 0.0, ORANGE);
                        }
                        let tag = if e.is_dir { "DIR".to_string() } else {
                            e.path.extension().map(|x| x.to_string_lossy().to_uppercase()).unwrap_or_default()
                        };
                        let tg = p.text(egui::pos2(rect.right() - pad, cy), egui::Align2::RIGHT_CENTER, tag, bold(10.0), if e.is_dir { fg } else { dim });
                        let name_x = rect.left() + pad + 48.0;
                        let clip = egui::Rect::from_x_y_ranges(name_x..=tg.left() - 12.0, rect.y_range());
                        let name_col = if playing && !selected { ORANGE } else { fg };
                        let font = if e.is_dir { bold(13.0) } else { egui::FontId::proportional(13.0) };
                        let label = if e.is_dir { format!("{}/", e.name) } else { e.name.clone() };
                        p.with_clip_rect(clip).text(egui::pos2(name_x, cy), egui::Align2::LEFT_CENTER, label, font, name_col);

                        if r.hovered() { ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand); }
                        if r.drag_started() && !e.is_dir { drag_out = Some(e.path.clone()); }
                        if r.clicked() { clicked = Some(vi); }
                        if r.double_clicked() { dbl = Some(vi); }
                    }
                });
                self.scroll_off = out.state.clamped_offset().y;
                if self.visible.is_empty() {
                    ui.add_space(20.0);
                    ui.horizontal(|ui| { ui.add_space(pad); label(ui, "EMPTY"); });
                }
                if let Some(i) = clicked {
                    self.selected = Some(i);
                    if !self.entries[self.visible[i]].is_dir { self.select(i); }
                }
                if let Some(p) = drag_out {
                    // Hand the file to the OS so it can be dropped into FL Studio, Explorer, etc.
                    // Always Copy mode, so the sample is never moved out of the library.
                    let opts = drag::Options { mode: drag::DragMode::Copy, ..Default::default() };
                    if let Err(e) = drag::start_drag(frame, drag::DragItem::Files(vec![p]), drag::Image::Raw(vec![]), |_, _| {}, opts) {
                        self.error = Some(format!("Drag failed: {e}"));
                    }
                }
                if let Some(i) = dbl {
                    self.selected = Some(i);
                    self.activate();
                }
            });
    }
}

fn main() -> eframe::Result {
    let root = std::env::args().nth(1).map(PathBuf::from).unwrap_or_else(|| PathBuf::from(DEFAULT_ROOT));
    let opts = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([1000.0, 680.0])
            .with_min_inner_size([600.0, 360.0])
            .with_title("splajs"),
        ..Default::default()
    };
    eframe::run_native("splajs", opts, Box::new(|cc| {
        theme::setup(&cc.egui_ctx);
        Ok(Box::new(App::new(root)))
    }))
}

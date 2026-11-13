#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use eframe::egui;
use std::fs::File;
use std::io::BufReader;
use std::path::{Path, PathBuf};

const DEFAULT_ROOT: &str = r"D:\ppsync\music\_packs";
const AUDIO_EXT: &[&str] = &["wav", "mp3", "flac", "ogg", "aif", "aiff", "m4a"];

struct Entry {
    name: String,
    path: PathBuf,
    is_dir: bool,
}

struct App {
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
    fn ui(&mut self, root_ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let ctx = &root_ui.ctx().clone();
        self.handle_keys(ctx);

        egui::Panel::top("top").show(root_ui, |ui| {
            ui.add_space(4.0);
            ui.horizontal(|ui| {
                if ui.button("⬆").on_hover_text("Parent (Left / Backspace)").clicked() {
                    self.go_up();
                }
                if ui.button("📁").on_hover_text("Open folder…").clicked() {
                    if let Some(d) = rfd::FileDialog::new().set_directory(&self.cwd).pick_folder() {
                        self.navigate(d);
                    }
                }
                // Breadcrumbs
                let mut nav = None;
                let comps: Vec<PathBuf> = self.cwd.ancestors().map(Path::to_path_buf).collect();
                for (i, p) in comps.iter().rev().enumerate() {
                    let label = p.file_name().map(|s| s.to_string_lossy().into_owned())
                        .unwrap_or_else(|| p.display().to_string());
                    if i > 0 { ui.label("›"); }
                    if ui.link(label).clicked() { nav = Some(p.clone()); }
                }
                if let Some(p) = nav { self.navigate(p); }
            });
            ui.horizontal(|ui| {
                ui.label("🔍");
                let r = ui.add(egui::TextEdit::singleline(&mut self.filter)
                    .hint_text("Type to filter…").desired_width(f32::INFINITY));
                if r.changed() { self.refilter(); }
                if !r.has_focus() && !ctx.egui_wants_keyboard_input() { r.request_focus(); }
            });
            ui.add_space(4.0);
        });

        egui::Panel::bottom("bottom").show(root_ui, |ui| {
            ui.horizontal(|ui| {
                let busy = self.player.as_ref().is_some_and(|p| !p.empty());
                if busy { ctx.request_repaint_after(std::time::Duration::from_millis(100)); }
                if ui.button(if busy { "⏹" } else { "▶" }).clicked() {
                    if busy { self.stop() } else { self.activate() }
                }
                if ui.add(egui::Slider::new(&mut self.volume, 0.0..=1.0).show_value(false)).changed() {
                    if let Some(p) = &self.player { p.set_volume(self.volume); }
                }
                if let Some(e) = &self.error {
                    ui.colored_label(egui::Color32::LIGHT_RED, e);
                } else if let (true, Some(p)) = (busy, &self.playing) {
                    ui.label(format!("♪ {}", p.file_name().unwrap_or_default().to_string_lossy()));
                } else {
                    ui.weak("↑↓ play · Enter/→ open · ←/Backspace up · Space replay · Esc stop");
                }
            });
        });

        egui::CentralPanel::default().show(root_ui, |ui| {
            let row_h = ui.text_style_height(&egui::TextStyle::Body) + 6.0;
            let mut clicked = None;
            let mut dbl = None;
            let scroll_target = self.scroll_to_selected.then_some(self.selected).flatten();
            self.scroll_to_selected = false;
            let mut area = egui::ScrollArea::vertical().auto_shrink(false);
            if let Some(s) = scroll_target {
                // Keep the selected row in view.
                let viewport_h = ui.available_height();
                let y = s as f32 * (row_h + ui.spacing().item_spacing.y);
                let off = self.scroll_off;
                if y < off { area = area.vertical_scroll_offset(y); }
                else if y + row_h > off + viewport_h { area = area.vertical_scroll_offset(y + row_h - viewport_h); }
            }
            let out = area.show_rows(ui, row_h, self.visible.len(), |ui, range| {
                for vi in range {
                    let e = &self.entries[self.visible[vi]];
                    let icon = if e.is_dir { "📁" } else if self.playing.as_deref() == Some(&e.path) { "🔊" } else { "🎵" };
                    let r = ui.add_sized(
                        [ui.available_width(), row_h],
                        egui::Button::selectable(self.selected == Some(vi), format!("{icon}  {}", e.name))
                            .frame_when_inactive(false),
                    );
                    if r.clicked() { clicked = Some(vi); }
                    if r.double_clicked() { dbl = Some(vi); }
                }
            });
            self.scroll_off = out.state.clamped_offset().y;
            if self.visible.is_empty() { ui.weak("Nothing here."); }
            if let Some(i) = clicked {
                self.selected = Some(i);
                if !self.entries[self.visible[i]].is_dir { self.select(i); }
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
        viewport: egui::ViewportBuilder::default().with_inner_size([900.0, 650.0]).with_title("splajs"),
        ..Default::default()
    };
    eframe::run_native("splajs", opts, Box::new(|_| Ok(Box::new(App::new(root)))))
}

//! crossfire on a simulated board.
//!
//! The application is `light_app_crossfire`, unchanged: the same crate the firmware links,
//! instantiated here over the framework's simulated seams instead of a board's. What a board
//! module builds out of pins and buses, this builds out of `light-sim`'s parts -- an SH1107
//! panel that decodes the real driver's commands, an indicator line, a button line, a hub with
//! four sockets, the console as a byte feed -- and runs the application's `serve` on a thread
//! of its own, forever, while a window shows the glass, lights the indicator, presses the
//! button, plugs instruments in and plays them, and carries the console.
//!
//! The board it stands in for is the Pico with the Pico-OLED-1.3: a display, one indicator,
//! BOOTSEL as the navigation control, instruments through a hub. The production board's two
//! indicators and hub lines would be a second set of parts over the same seams.
//!
//! The assets -- the font, the look-and-feel, the interface -- are compiled at start-up by
//! crush-core, the code crush runs in the firmware build, from the same sources. Nothing about
//! the picture is approximated.

use std::path::{Path, PathBuf};
use std::sync::OnceLock;
use std::time::Duration;

use light_app_crossfire as app;
use app::{ConsoleMod, LedMod, NavMod, OledMod, Permanent, ProbationMod, UsbMod, Vitals};
use light_core::{log, ConstStaticCell, Idle, StaticCell};
use light_display::sh1107::Sh1107;
use light_display::{Display, FrameLayer};
use light_draw::PixelFormat;
use light_font::Font;
use light_host_gui::{eframe, egui};
use light_sim::sh1107::{PanelView, Sh1107Panel};
use light_sim::usb::{self, BusHandle, Instrument, SimUsbHost};
use light_sim::{Line, LogView, SimClock, SimIdle};

/// The glass is physically portrait: 64 wide, 128 tall; the application draws along its long
/// side.
const OLED_WIDTH: u16 = 64;
const OLED_HEIGHT: u16 = 128;
/// The RAM offset the Pico-OLED-1.3's glass sits at, which the firmware configures.
const OLED_DISPLAY_OFFSET: u8 = 96;
/// The status font: 16 px, as the firmware builds it.
const FONT_PX: u16 = 16;

/// The same face the firmware's font is rendered from.
const TTF: &[u8] = include_bytes!("../../../assets/fonts/TypeLightSans.ttf");
const THEME_JSON: &str = include_str!("../../../crates/light_app_crossfire/theme.json");
const DESIGN_JSON: &str = include_str!("../../../crates/light_app_crossfire/design.json");

/// The framework checkout: `LIGHT_PATH`, or the sibling of this project.
fn light_path() -> PathBuf {
        match std::env::var_os("LIGHT_PATH") {
                Some(p) => PathBuf::from(p),
                None => Path::new(env!("CARGO_MANIFEST_DIR")).join("../../../light_mk5"),
        }
}

struct Assets {
        font: &'static [u8],
        theme: &'static [u8],
        ui: &'static [u8],
}

/// Compile the three assets the way the firmware build does, and keep them for the program's
/// life: the application takes them as `&'static`.
fn compile_assets() -> Result<Assets, String> {
        let light = light_path();
        let rendered = crush_core::render::rasterize(TTF, 0, FONT_PX, 0.0, 96.0, 96.0, true)?;
        //   the theme extends the framework's mono default, found in its themes directory; the
        // source is resolved from a scratch copy so relative references work as they do in the
        // repository
        let scratch = std::env::temp_dir().join("crossfire-sim");
        std::fs::create_dir_all(&scratch).map_err(|e| e.to_string())?;
        let theme_path = scratch.join("theme.json");
        std::fs::write(&theme_path, THEME_JSON).map_err(|e| e.to_string())?;
        let theme = crush_core::theme::compile_file(&theme_path, Some(&light.join("themes")), Some("mono"))?;
        let design_path = scratch.join("design.json");
        std::fs::write(&design_path, DESIGN_JSON).map_err(|e| e.to_string())?;
        let design = crush_core::design::resolve_file(&design_path, Some(&light.join("crates")))?;
        let ui = crush_core::lui::compile(&design)?;
        Ok(Assets { font: Vec::leak(rendered.lgf), theme: Vec::leak(theme), ui: Vec::leak(ui) })
}

// --- the application's thread --------------------------------------------------------------------------------

/// The log drain's view, for the probation checks' console pulse.
static LOG_VIEW: OnceLock<LogView> = OnceLock::new();

/// The application's signs of life, with the log drain as the console core.
fn vitals() -> Vitals {
        app::vitals(LOG_VIEW.get().map_or(0, LogView::ticks))
}

/// The parts the window keeps a handle to, built once and handed to the application's thread.
struct Parts {
        panel: Sh1107Panel,
        led: Line,
        button: Line,
        host: SimUsbHost,
}

/// What a board module does at `light_app_main`, on the host: construct the big pieces in
/// statics, the modules around them, and serve forever.
fn run_application(parts: Parts, assets: Assets) -> ! {
        log::set_clock(light_sim::now_us);
        static FRAME: ConstStaticCell<[u8; PixelFormat::Mono1.buffer_len(OLED_WIDTH, OLED_HEIGHT)]> = ConstStaticCell::new([0; PixelFormat::Mono1.buffer_len(OLED_WIDTH, OLED_HEIGHT)]);
        static LAYER: ConstStaticCell<FrameLayer> = ConstStaticCell::new(FrameLayer::new(OLED_WIDTH, OLED_HEIGHT, PixelFormat::Mono1));
        let display = Display::new(Sh1107::new(parts.panel), FRAME.take(), OLED_WIDTH, OLED_HEIGHT, PixelFormat::Mono1, light_sim::now_us);
        let font = match Font::parse(assets.font) {
                Ok(f) => f,
                Err(e) => panic!("the rasterised font does not parse: {e:?}"),
        };
        light_core::info!("crossfire (simulated board): host stack, display, indicator and button are simulated; console on the window and standard input");

        static USB_MOD: StaticCell<UsbMod<SimUsbHost>> = StaticCell::new();
        let usb_mod = USB_MOD.init(UsbMod::new(parts.host));
        static OLED_MOD: StaticCell<OledMod<Sh1107Panel, SimClock>> = StaticCell::new();
        let oled_mod = OLED_MOD.init(OledMod::new(display, LAYER.take(), font, assets.theme, assets.ui, SimClock, OLED_DISPLAY_OFFSET));
        let mut led_mod = LedMod::new(parts.led.output());
        let mut console_mod = ConsoleMod::new();
        //   BOOTSEL, as the window presses it: the line low is the button down
        let button = parts.button.input();
        let mut nav_mod = NavMod::new(move || {
                use light_core::InputPin;
                button.is_low()
        });
        //   nothing is on probation on the host: every image is already kept
        let mut probation_mod = ProbationMod::new(Permanent, vitals);
        let mut idle = SimIdle;
        app::serve(usb_mod, &mut console_mod, &mut nav_mod, &mut [oled_mod, &mut led_mod], &mut probation_mod, move || idle.idle())
}

// --- the window ------------------------------------------------------------------------------------------------

/// Instruments to plug in: a name and its cables each way.
const PRESETS: &[(&str, u8, u8)] = &[("keyboard (1 cable)", 1, 1), ("controller (2 cables)", 2, 2), ("sound module (listens only)", 1, 1), ("interface (4 cables)", 4, 4)];

/// How many packets a socket's monitor keeps.
const MONITOR_LINES: usize = 14;

/// One socket's controls and what its instrument has heard.
struct SocketPane {
        preset: usize,
        note: u8,
        channel: u8,
        heard: Vec<String>,
        heard_total: u32,
}

impl Default for SocketPane {
        fn default() -> Self {
                Self { preset: 0, note: 60, channel: 1, heard: Vec::new(), heard_total: 0 }
        }
}

struct SimApp {
        panel: PanelView,
        led: Line,
        button: Line,
        bus: BusHandle,
        logs: LogView,
        /// The glass, as a texture, rebuilt each frame.
        glass: Option<egui::TextureHandle>,
        /// Turned so the long side is horizontal and the text reads, as the board is held; a
        /// quarter turn at a time, for the other way up.
        turn: u8,
        scale: f32,
        sockets: [SocketPane; usb::PORTS as usize],
        log_lines: Vec<String>,
        console_input: String,
}

impl SimApp {
        fn new(panel: PanelView, led: Line, button: Line, bus: BusHandle, logs: LogView) -> Self {
                //   three quarter-turns clockwise, which is the one that reads: the application
                // puts its top on the panel's right edge, and a quarter turn the other way would
                // show it upside down -- as the first build did
                Self { panel, led, button, bus, logs, glass: None, turn: 3, scale: 3.0, sockets: Default::default(), log_lines: Vec::new(), console_input: String::new() }
        }

        /// The glass as an image, turned `turn` quarter-turns clockwise from the panel's own
        /// portrait orientation. The application draws its landscape interface rotated so its
        /// top lands on the panel's right edge; three turns on (one back) reads it.
        fn glass_image(&self) -> egui::ColorImage {
                let pw = self.panel.width();
                let ph = self.panel.height();
                let px = self.panel.pixels();
                let (w, h) = if self.turn % 2 == 1 { (ph, pw) } else { (pw, ph) };
                let mut rgba = vec![0u8; w * h * 4];
                for y in 0..h {
                        for x in 0..w {
                                //   the panel pixel this screen pixel shows, for each quarter turn
                                let (sx, sy) = match self.turn % 4 {
                                        0 => (x, y),
                                        1 => (y, ph - 1 - x),
                                        2 => (pw - 1 - x, ph - 1 - y),
                                        _ => (pw - 1 - y, x),
                                };
                                let lit = px[sy * pw + sx];
                                let i = (y * w + x) * 4;
                                let (r, g, b) = if lit { (235, 240, 255) } else { (12, 14, 20) };
                                rgba[i] = r;
                                rgba[i + 1] = g;
                                rgba[i + 2] = b;
                                rgba[i + 3] = 255;
                        }
                }
                egui::ColorImage::from_rgba_unmultiplied([w, h], &rgba)
        }

        fn device_pane(&mut self, ui: &mut egui::Ui) {
                ui.heading("device");
                let img = self.glass_image();
                let size = egui::vec2(img.size[0] as f32 * self.scale, img.size[1] as f32 * self.scale);
                match &mut self.glass {
                        Some(t) => t.set(img, egui::TextureOptions::NEAREST),
                        None => self.glass = Some(ui.ctx().load_texture("glass", img, egui::TextureOptions::NEAREST)),
                }
                if let Some(t) = &self.glass {
                        ui.image(egui::load::SizedTexture::new(t.id(), size));
                }
                ui.horizontal(|ui| {
                        if ui.button("turn").clicked() {
                                self.turn = (self.turn + 1) % 4;
                        }
                        ui.add(egui::Slider::new(&mut self.scale, 1.0..=6.0).text("scale"));
                });
                ui.horizontal(|ui| {
                        let (rect, _) = ui.allocate_exact_size(egui::vec2(18.0, 18.0), egui::Sense::hover());
                        let colour = if self.led.is_high() { egui::Color32::from_rgb(70, 230, 90) } else { egui::Color32::from_rgb(40, 60, 44) };
                        ui.painter().circle_filled(rect.center(), 8.0, colour);
                        ui.label("indicator");
                });
                ui.horizontal(|ui| {
                        //   held down while the pointer is: the line follows the button
                        let r = ui.button("BOOTSEL");
                        self.button.set(!r.is_pointer_button_down_on());
                        ui.label("hold to press; a press turns the page");
                });
                ui.separator();
                ui.label(format!("display {}, offset {}, {} bytes written", if self.panel.is_on() { "on" } else { "off" }, self.panel.configured_offset(), self.panel.writes()));
        }

        fn hub_pane(&mut self, ui: &mut egui::Ui) {
                ui.heading("hub");
                for port in 1..=usb::PORTS {
                        let pane = &mut self.sockets[usize::from(port - 1)];
                        //   collect what the application sent this instrument since last time
                        for p in self.bus.take_sent(port) {
                                pane.heard_total += 1;
                                pane.heard.push(usb::describe(&p));
                                if pane.heard.len() > MONITOR_LINES {
                                        pane.heard.remove(0);
                                }
                        }
                        egui::CollapsingHeader::new(format!("socket {port}")).default_open(true).show(ui, |ui| match self.bus.socket(port) {
                                None => {
                                        ui.horizontal(|ui| {
                                                egui::ComboBox::from_id_salt(("preset", port)).selected_text(PRESETS[pane.preset].0).show_ui(ui, |ui| {
                                                        for (i, (name, _, _)) in PRESETS.iter().enumerate() {
                                                                ui.selectable_value(&mut pane.preset, i, *name);
                                                        }
                                                });
                                                if ui.button("plug in").clicked() {
                                                        let (name, rx, tx) = PRESETS[pane.preset];
                                                        if let Err(e) = self.bus.plug(port, Instrument::new(name, rx, tx)) {
                                                                self.logs.note(format!("sim: socket {port}: {e}"));
                                                        }
                                                        pane.heard.clear();
                                                        pane.heard_total = 0;
                                                }
                                        });
                                }
                                Some(s) => {
                                        ui.horizontal(|ui| {
                                                ui.label(format!("{} -- slot {}, address {}", s.instrument.name, s.slot, s.daddr));
                                                if ui.button("unplug").clicked() {
                                                        self.bus.unplug(port);
                                                }
                                        });
                                        ui.horizontal(|ui| {
                                                ui.add(egui::DragValue::new(&mut pane.note).range(0..=127).prefix("note "));
                                                ui.add(egui::DragValue::new(&mut pane.channel).range(1..=16).prefix("ch "));
                                                let ch = pane.channel - 1;
                                                if ui.button("note on").clicked() {
                                                        self.bus.play(port, usb::note_on(0, ch, pane.note, 100));
                                                }
                                                if ui.button("note off").clicked() {
                                                        self.bus.play(port, usb::note_off(0, ch, pane.note));
                                                }
                                                if ui.button("cc 7").clicked() {
                                                        self.bus.play(port, usb::control_change(0, ch, 7, 100));
                                                }
                                        });
                                        ui.label(format!("heard {} packets; flushed {} times", pane.heard_total, s.flushes));
                                        for line in &pane.heard {
                                                ui.monospace(line);
                                        }
                                }
                        });
                }
        }

        fn console_pane(&mut self, ui: &mut egui::Ui) {
                let mut fresh = self.logs.lines_from(self.log_lines.len());
                self.log_lines.append(&mut fresh);
                ui.horizontal(|ui| {
                        ui.label("console");
                        let r = ui.add(egui::TextEdit::singleline(&mut self.console_input).desired_width(360.0).hint_text("help"));
                        let enter = r.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
                        if ui.button("send").clicked() || enter {
                                let line = std::mem::take(&mut self.console_input);
                                light_sim::send_line(app::push_console_byte, &line);
                                r.request_focus();
                        }
                });
                egui::ScrollArea::vertical().stick_to_bottom(true).auto_shrink([false, false]).show(ui, |ui| {
                        for line in &self.log_lines {
                                ui.monospace(line);
                        }
                });
        }
}

impl eframe::App for SimApp {
        fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
                egui::SidePanel::left("device").resizable(false).show(ctx, |ui| self.device_pane(ui));
                egui::TopBottomPanel::bottom("console").resizable(true).default_height(260.0).show(ctx, |ui| self.console_pane(ui));
                egui::CentralPanel::default().show(ctx, |ui| self.hub_pane(ui));
                // the glass and the log move on their own: keep looking
                ctx.request_repaint_after(Duration::from_millis(30));
        }
}

fn main() -> eframe::Result {
        let assets = match compile_assets() {
                Ok(a) => a,
                Err(e) => {
                        eprintln!("crossfire-sim: could not compile the assets: {e}");
                        std::process::exit(1);
                }
        };
        let (panel, view) = Sh1107Panel::new(OLED_WIDTH, OLED_HEIGHT, OLED_DISPLAY_OFFSET);
        let led = Line::new(false);
        //   pulled up: released until pressed
        let button = Line::new(true);
        let (host, bus) = SimUsbHost::new();
        let logs = light_sim::start_log_drain(true);
        let _ = LOG_VIEW.set(logs.clone());
        light_sim::start_stdin(app::push_console_byte);
        let parts = Parts { panel, led: led.clone(), button: button.clone(), host };
        std::thread::Builder::new().name("crossfire".into()).spawn(move || run_application(parts, assets)).expect("spawn the application");
        light_host_gui::run("crossfire -- simulated board", [1040.0, 720.0], SimApp::new(view, led, button, bus, logs))
}

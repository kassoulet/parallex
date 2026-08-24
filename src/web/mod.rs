//! The web frontend: the same three synchronized columns as the gpui and TUI
//! frontends, drawn onto a single `<canvas>` through web-sys.
//!
//! Like every frontend it owns no maths: layout, geometry, colour and the
//! thumbnail generators all come from `core`. Pure canvas-specific layout
//! (column x-ranges, divider hit-testing) lives in `layout`, drawing in
//! `render`, state and event wiring here.

mod layout;
mod render;

mod app;
pub(crate) use app::App;

use std::cell::{Cell, RefCell};

use wasm_bindgen::prelude::*;
use web_sys::{HtmlCanvasElement, KeyboardEvent, MouseEvent, WheelEvent};

thread_local! {
    static APP: RefCell<App> = RefCell::new(App::new());
    static CANVAS: RefCell<Option<HtmlCanvasElement>> = const { RefCell::new(None) };
    static SCHEDULED: Cell<bool> = const { Cell::new(false) };
    static FRAME_CLOSURE: RefCell<Option<Closure<dyn FnMut()>>> = RefCell::new(None);
}

pub(crate) fn with_app<R>(f: impl FnOnce(&mut App) -> R) -> R {
    APP.with(|a| f(&mut a.borrow_mut()))
}

pub(crate) fn with_canvas<R>(f: impl FnOnce(&HtmlCanvasElement) -> R) -> Option<R> {
    CANVAS.with(|c| c.borrow().as_ref().map(f))
}

/// Schedule one redraw on the next animation frame (coalesced).
pub(crate) fn request_frame() {
    if SCHEDULED.with(Cell::get) {
        return;
    }
    SCHEDULED.set(true);
    FRAME_CLOSURE.with(|slot| {
        let mut slot = slot.borrow_mut();
        let closure = slot.get_or_insert_with(|| {
            Closure::new(|| {
                SCHEDULED.set(false);
                with_app(|app| {
                    with_canvas(|canvas| render::draw(app, canvas));
                });
            })
        });
        if let Some(w) = web_sys::window() {
            let _ = w.request_animation_frame(closure.as_ref().unchecked_ref());
        }
    });
}

// ----- entry points ---------------------------------------------------------

/// Boot the app: take over `#parallhex`, size it to the window and wire every
/// event listener. Called once from `index.html` after the wasm module loads.
///
/// (Named `parallhex_start` rather than `start`: wasm-bindgen reserves a
/// function called `start` as the module's automatic start hook instead of
/// exporting it.)
///
/// # Panics
///
/// When `#parallhex` is missing from the document or is not a canvas — a
/// build/packaging mistake, so failing loudly is correct.
#[wasm_bindgen]
pub fn parallhex_start() {
    let window = web_sys::window().expect("no window");
    let document = window.document().expect("no document");
    let canvas = document
        .get_element_by_id("parallhex")
        .expect("#parallhex canvas")
        .dyn_into::<HtmlCanvasElement>()
        .expect("#parallhex is a canvas");

    // Measure the monospace glyph once; every horizontal position in the hex
    // column derives from it, exactly like the gpui frontend's `hex_char_w`.
    let ctx = render::ctx_of(&canvas);
    ctx.set_font(render::HEX_FONT);
    let char_w = ctx.measure_text("0").map_or(8.0, |m| m.width() as f32);

    APP.with(|a| {
        let mut app = a.borrow_mut();
        app.char_w = char_w;
        resize(&mut app, &canvas);
    });
    CANVAS.with(|c| *c.borrow_mut() = Some(canvas.clone()));

    wire_events(&window, &canvas);
    wire_file_input(&document);
    request_frame();
    load_demo_file_from_query(&window);
}

/// `?file=<url>` loads a file at boot, so a Pages site can link a demo binary
/// directly and the app is testable without a file picker.
fn load_demo_file_from_query(window: &web_sys::Window) {
    let search = window.location().search().unwrap_or_default();
    let Some(param) = web_sys::UrlSearchParams::new_with_str(&search)
        .ok()
        .and_then(|q| q.get("file"))
        .filter(|s| !s.is_empty())
    else {
        return;
    };
    let future = wasm_bindgen_futures::JsFuture::from(window.fetch_with_str(&param));
    wasm_bindgen_futures::spawn_local(async move {
        let Ok(resp) = future.await else {
            web_sys::console::error_1(&format!("fetch {param} failed").into());
            return;
        };
        let resp: web_sys::Response = resp.dyn_into().expect("response");
        let name = param.rsplit('/').next().unwrap_or(&param).to_string();
        match wasm_bindgen_futures::JsFuture::from(resp.array_buffer().expect("array_buffer")).await
        {
            Ok(buf) => {
                let bytes = js_sys::Uint8Array::new(&buf).to_vec();
                with_app(|app| app.set_data(name, bytes));
                request_frame();
            }
            Err(e) => web_sys::console::error_1(&format!("read failed: {e:?}").into()),
        }
    });
}

/// Load a user-picked file into the viewer.
#[wasm_bindgen]
pub fn load_file(file: web_sys::File) {
    wasm_bindgen_futures::spawn_local(async move {
        let name = file.name();
        match wasm_bindgen_futures::JsFuture::from(file.array_buffer()).await {
            Ok(buf) => {
                let bytes = js_sys::Uint8Array::new(&buf).to_vec();
                with_app(|app| app.set_data(name, bytes));
                request_frame();
            }
            Err(e) => web_sys::console::error_1(&format!("read failed: {e:?}").into()),
        }
    });
}

fn resize(app: &mut App, canvas: &HtmlCanvasElement) {
    let Some(window) = web_sys::window() else {
        return;
    };
    let css_w = window
        .inner_width()
        .ok()
        .and_then(|v| v.as_f64())
        .unwrap_or(800.0) as f32;
    let css_h = window
        .inner_height()
        .ok()
        .and_then(|v| v.as_f64())
        .unwrap_or(600.0) as f32;
    let dpr = window.device_pixel_ratio() as f32;
    app.set_viewport(css_w, css_h, dpr);
    canvas.set_width((css_w * dpr).round() as u32);
    canvas.set_height((css_h * dpr).round() as u32);
}

/// One `listen` per browser event the app reacts to; long by construction.
#[allow(clippy::too_many_lines)]
fn wire_events(window: &web_sys::Window, canvas: &HtmlCanvasElement) {
    let listen = |target: &web_sys::EventTarget,
                  kind: &'static str,
                  handler: Box<dyn FnMut(&web_sys::Event)>| {
        let closure = Closure::wrap(handler);
        target
            .add_event_listener_with_callback(kind, closure.as_ref().unchecked_ref())
            .expect("addEventListener");
        closure.forget();
    };

    {
        let canvas = canvas.clone();
        listen(
            window.as_ref(),
            "resize",
            Box::new(move |_| {
                with_app(|app| resize(app, &canvas));
                request_frame();
            }),
        );
    }

    // Wheel: scroll the shared anchor. The hex column is the reference, as on
    // the desktop.
    {
        let canvas = canvas.clone();
        listen(
            canvas.as_ref(),
            "wheel",
            Box::new(move |e| {
                let Some(e) = e.dyn_ref::<WheelEvent>() else {
                    return;
                };
                e.prevent_default();
                let dy = -e.delta_y() as f32;
                with_app(|app| {
                    let delta = (dy / layout::BLOCK_H * app.hex_bpr as f32) as isize;
                    app.scroll_anchor_by(delta);
                });
                request_frame();
            }),
        );
    }

    {
        listen(
            canvas.as_ref(),
            "mousedown",
            Box::new(|e| {
                let Some(e) = e.dyn_ref::<MouseEvent>() else {
                    return;
                };
                with_app(|app| app.on_mouse_down(e.offset_x() as f32, e.offset_y() as f32));
                request_frame();
            }),
        );
    }

    {
        listen(
            canvas.as_ref(),
            "mousemove",
            Box::new(|e| {
                let Some(e) = e.dyn_ref::<MouseEvent>() else {
                    return;
                };
                with_app(|app| app.on_mouse_move(e.offset_x() as f32, e.offset_y() as f32));
                request_frame();
            }),
        );
    }

    listen(
        window.as_ref(),
        "mouseup",
        Box::new(|_| {
            with_app(App::end_drag);
            request_frame();
        }),
    );

    listen(
        window.as_ref(),
        "keydown",
        Box::new(|e| {
            let Some(e) = e.dyn_ref::<KeyboardEvent>() else {
                return;
            };
            if e.target() != e.current_target() {
                return; // focus is in the file input, not the page
            }
            if app_key(e) {
                e.prevent_default();
                request_frame();
            }
        }),
    );

    // Drag & drop a file onto the canvas.
    listen(
        canvas.as_ref(),
        "dragover",
        Box::new(|e| {
            e.prevent_default();
        }),
    );
    listen(
        canvas.as_ref(),
        "drop",
        Box::new(|e| {
            e.prevent_default();
            let Some(e) = e.dyn_ref::<web_sys::DragEvent>() else {
                return;
            };
            let Some(files) = e.data_transfer().and_then(|d| d.files()) else {
                return;
            };
            if let Some(file) = files.get(0) {
                load_file(file);
            }
        }),
    );
}

fn wire_file_input(document: &web_sys::Document) {
    let Some(input) = document.get_element_by_id("file") else {
        return;
    };
    let closure: Closure<dyn FnMut()> = Closure::new(|| {
        let window = web_sys::window().expect("no window");
        let document = window.document().expect("no document");
        let input = document
            .get_element_by_id("file")
            .expect("#file input")
            .dyn_into::<web_sys::HtmlInputElement>()
            .expect("#file is an input");
        if let Some(files) = input.files().and_then(|f| f.get(0)) {
            load_file(files);
        }
    });
    input
        .add_event_listener_with_callback("change", closure.as_ref().unchecked_ref())
        .expect("addEventListener");
    closure.forget();
}

/// Map a keyboard event to a navigation step; returns true when handled.
fn app_key(e: &KeyboardEvent) -> bool {
    use crate::core::geom::Nav;
    let nav = match e.key().as_str() {
        "ArrowLeft" => Nav::Left,
        "ArrowRight" => Nav::Right,
        "ArrowUp" => Nav::Up,
        "ArrowDown" => Nav::Down,
        "PageUp" => Nav::PageUp,
        "PageDown" => Nav::PageDown,
        "Home" => Nav::Home,
        "End" => Nav::End,
        "=" | "+" => {
            with_app(App::zoom_in);
            return true;
        }
        "-" => {
            with_app(App::zoom_out);
            return true;
        }
        _ => return false,
    };
    with_app(|app| app.navigate(nav));
    true
}

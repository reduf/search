use glium::Surface;
use glutin::{
    config::ConfigTemplateBuilder,
    context::{ContextAttributesBuilder, NotCurrentGlContext},
    display::{GetGlDisplay, GlDisplay},
    surface::{SurfaceAttributesBuilder, WindowSurface},
};
use raw_window_handle::HasRawWindowHandle;

use imgui::{ConfigFlags, Context, FontConfig, FontGlyphRanges, FontSource};
use imgui_glium_renderer::Renderer;
use imgui_winit_support::{HiDpiMode, WinitPlatform};
use imgui_winit_support::winit::{dpi::LogicalSize, event_loop::EventLoop, window::WindowBuilder};
use winit::{
    event::{Event, WindowEvent},
    window::{Icon, Window},
};
use std::{
    num::NonZeroU32,
    path::Path,
    time::Instant,
};

use crate::{app::App, clipboard};

pub struct System {
    pub event_loop: EventLoop<()>,
    pub display: glium::Display<WindowSurface>,
    pub imgui: Context,
    pub platform: WinitPlatform,
    pub renderer: Renderer,
    pub window: Window,
}

fn load_icon() -> Option<Icon> {
    let buffer = include_bytes!("../resources/icons8-magnifying-glass-tilted-left-96.png");
    if let Ok(img) = crate::stb_image::load_bytes(buffer.as_ref()) {
        let rgba_bytes = img.data().to_vec();
        return Icon::from_rgba(rgba_bytes, img.width, img.height).ok();
    } else {
        return None;
    }
}

pub fn init(title: &str) -> System {
    let title = match Path::new(&title).file_name() {
        Some(file_name) => file_name.to_str().unwrap(),
        None => title,
    };
    let event_loop = EventLoop::new().expect("Failed to create EventLoop");
    // let context = glutin::ContextBuilder::new().with_vsync(true);
    let builder = WindowBuilder::new()
        .with_title(title.to_owned())
        .with_inner_size(LogicalSize::new(1024f64, 768f64))
        .with_window_icon(load_icon());

     let (window, cfg) = glutin_winit::DisplayBuilder::new()
        .with_window_builder(Some(builder))
        .build(&event_loop, ConfigTemplateBuilder::new(), |mut configs| {
            configs.next().unwrap()
        })
        .expect("Failed to create OpenGL window");
    let window = window.unwrap();

    let context_attribs = ContextAttributesBuilder::new().build(Some(window.raw_window_handle()));
    let context = unsafe {
        cfg.display()
            .create_context(&cfg, &context_attribs)
            .expect("Failed to create OpenGL context")
    };

    let surface_attribs = SurfaceAttributesBuilder::<WindowSurface>::new().build(
        window.raw_window_handle(),
        NonZeroU32::new(1024).unwrap(),
        NonZeroU32::new(768).unwrap(),
    );
    let surface = unsafe {
        cfg.display()
            .create_window_surface(&cfg, &surface_attribs)
            .expect("Failed to create OpenGL surface")
    };

    let context = context
        .make_current(&surface)
        .expect("Failed to make OpenGL context current");

    let display = glium::Display::from_context_surface(context, surface)
        .expect("Failed to create glium Display");

    let mut imgui = Context::create();
    imgui.set_ini_filename(None);

    if let Some(backend) = clipboard::init() {
        imgui.set_clipboard_backend(backend);
    } else {
        eprintln!("Failed to initialize clipboard");
    }

    let mut platform = WinitPlatform::init(&mut imgui);
    {
        let dpi_mode = if let Ok(factor) = std::env::var("IMGUI_EXAMPLE_FORCE_DPI_FACTOR") {
            // Allow forcing of HiDPI factor for debugging purposes
            match factor.parse::<f64>() {
                Ok(f) => HiDpiMode::Locked(f),
                Err(e) => panic!("Invalid scaling factor: {}", e),
            }
        } else {
            HiDpiMode::Default
        };

        platform.attach_window(imgui.io_mut(), &window, dpi_mode);
    }

    let hidpi_factor = platform.hidpi_factor() as f32;

    imgui.fonts().add_font(&[
        FontSource::TtfData {
            data: include_bytes!("../resources/Lucon.ttf"),
            size_pixels: 12.0 * hidpi_factor,
            config: Some(FontConfig {
                // As imgui-glium-renderer isn't gamma-correct with it's font rendering,
                // we apply an arbitrary multiplier to make the font a bit "heavier".
                // With default imgui-glow-renderer this is unnecessary.
                rasterizer_multiply: 1.2,
                // Oversampling font helps improve text rendering at expense of larger
                // font atlas texture.
                oversample_h: 4,
                oversample_v: 4,
                ..FontConfig::default()
            }),
        },
        FontSource::TtfData {
            data: include_bytes!("../resources/mplus-1p-regular.ttf"),
            size_pixels: 15.0 * hidpi_factor,
            config: Some(FontConfig {
                // Oversampling font helps improve text rendering at expense of larger
                // font atlas texture.
                oversample_h: 4,
                oversample_v: 4,
                // Range of glyphs to rasterize
                glyph_ranges: FontGlyphRanges::japanese(),
                ..FontConfig::default()
            }),
        },
        FontSource::TtfData {
            data: include_bytes!("../resources/mplus-1p-regular.ttf"),
            size_pixels: 15.0 * hidpi_factor,
            config: Some(FontConfig {
                // Oversampling font helps improve text rendering at expense of larger
                // font atlas texture.
                oversample_h: 4,
                oversample_v: 4,
                // Range of glyphs to rasterize
                glyph_ranges: FontGlyphRanges::from_slice(&[
                    0x0370, 0x03FF, // Greek and Coptic
                    0x0400, 0x052F, // Cyrillic + Cyrillic Supplement
                    0x0E00, 0x0E7F, // Thai
                    0x2010, 0x205E, // Punctuations
                    0x2DE0, 0x2DFF, // Cyrillic Extended-A
                    0x3131, 0x3163, // Korean alphabets
                    0xA640, 0xA69F, // Cyrillic Extended-B
                    0xAC00, 0xD7A3, // Korean characters
                    0xFFFD, 0xFFFD, // Invalid
                    0,
                ]),
                ..FontConfig::default()
            }),
        },
    ]);

    // @Cleanup:
    // This is apprently necessary on MacOS, because it pretend it has 2x less pixel
    // than it actually does, so the trick is to rasterize the font twice as big and
    // scale it down in order to have font of the right size, but crisp looking.
    //
    // Can somebody test??
    //
    // imgui.io_mut().font_global_scale = (1.0 / hidpi_factor) as f32;

    imgui.style_mut().scale_all_sizes(hidpi_factor);

    let renderer = Renderer::init(&mut imgui, &display).expect("Failed to initialize renderer");

    return System {
        event_loop,
        display,
        imgui,
        platform,
        renderer,
        window,
    };
}

impl System {
    pub fn main_loop(self, mut app: App) {
        let System {
            event_loop,
            display,
            mut imgui,
            mut platform,
            mut renderer,
            window,
            ..
        } = self;

        // Allow us to use PageUp and PageDown to navigate in the result window.
        imgui
            .io_mut()
            .config_flags
            .set(ConfigFlags::NAV_ENABLE_KEYBOARD, true);

        let hidpi_factor = platform.hidpi_factor() as f32;

        let mut last_frame = Instant::now();
        event_loop.run(move |event, window_target| match event {
            Event::NewEvents(_) => {
                let now = Instant::now();
                imgui.io_mut().update_delta_time(now - last_frame);
                last_frame = now;
            }
            Event::AboutToWait => {
                platform
                    .prepare_frame(imgui.io_mut(), &window)
                    .expect("Failed to prepare frame");
                window.request_redraw();
            }
            Event::WindowEvent {
                event: WindowEvent::RedrawRequested,
                ..
            } => {
                // Create frame for the all important `&imgui::Ui`
                let ui = imgui.frame();

                let mut run = true;
                app.update(&mut run, ui);
                if !run {
                    window_target.exit();
                }

                // Setup for drawing
                let mut target = display.draw();

                // Renderer doesn't automatically clear window
                target.clear_color_srgb(1.0, 1.0, 1.0, 1.0);

                // Perform rendering
                platform.prepare_render(ui, &window);
                let draw_data = imgui.render();
                renderer
                    .render(&mut target, draw_data)
                    .expect("Rendering failed");
                target.finish().expect("Failed to swap buffers");

                app.process_drag_drop(imgui.io_mut());
            }
            Event::WindowEvent {
                event: WindowEvent::CloseRequested,
                ..
            } => window_target.exit(),
            Event::WindowEvent {
                event: WindowEvent::Resized(new_size),
                ..
            } => imgui.io_mut().display_size = [new_size.width as f32, new_size.height as f32],
            // @Cleanup:
            // Unclear whether that's really necessary or there is an issue in "imgui-winit-support"
            // crate, but we need to do it.
            Event::WindowEvent {
                event: WindowEvent::CursorMoved { position, ..},
                ..
            } => imgui.io_mut().add_mouse_pos_event([(position.x as f32)/hidpi_factor, (position.y as f32)/hidpi_factor]),
            event => {
                if !app.handle_event(&window, &event) {
                    platform.handle_event(imgui.io_mut(), &window, &event);
                }
            }
        }).expect("why did this fail?!?");
    }
}

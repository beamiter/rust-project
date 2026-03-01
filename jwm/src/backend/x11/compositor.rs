use glow::HasContext;
use std::collections::HashMap;
use std::ffi::CString;
use std::sync::Arc;
use x11rb::connection::{Connection, RequestConnection};
use x11rb::protocol::composite::ConnectionExt as CompositeExt;
use x11rb::protocol::damage::{self, ConnectionExt as DamageExt};
use x11rb::protocol::xfixes::ConnectionExt as XFixesExt;
use x11rb::protocol::xproto::ConnectionExt as XProtoExt;
use x11rb::rust_connection::RustConnection;

use super::shaders;

// ---------------------------------------------------------------------------
// TFP function pointers (glXBindTexImageEXT / glXReleaseTexImageEXT)
// ---------------------------------------------------------------------------

type GlXBindTexImageEXT =
    unsafe extern "C" fn(*mut x11::xlib::Display, x11::glx::GLXDrawable, i32, *const i32);
type GlXReleaseTexImageEXT =
    unsafe extern "C" fn(*mut x11::xlib::Display, x11::glx::GLXDrawable, i32);
type GlXSwapIntervalEXT =
    unsafe extern "C" fn(*mut x11::xlib::Display, x11::glx::GLXDrawable, i32);

struct TfpFunctions {
    bind: GlXBindTexImageEXT,
    release: GlXReleaseTexImageEXT,
}

// GLX_BIND_TO_TEXTURE_*_EXT constants
const GLX_BIND_TO_TEXTURE_RGBA_EXT: i32 = 0x20D1;
const GLX_BIND_TO_TEXTURE_RGB_EXT: i32 = 0x20D0;
#[allow(dead_code)]
const GLX_Y_INVERTED_EXT: i32 = 0x20D4;
const GLX_TEXTURE_FORMAT_EXT: i32 = 0x20D5;
const GLX_TEXTURE_TARGET_EXT: i32 = 0x20D6;
const GLX_TEXTURE_2D_EXT: i32 = 0x20DC;
const GLX_TEXTURE_FORMAT_RGBA_EXT: i32 = 0x20DA;
const GLX_TEXTURE_FORMAT_RGB_EXT: i32 = 0x20D9;
const GLX_FRONT_LEFT_EXT: i32 = 0x20DE;

// ---------------------------------------------------------------------------
// Per-window texture state
// ---------------------------------------------------------------------------

struct WindowTexture {
    #[allow(dead_code)]
    x: i32,
    #[allow(dead_code)]
    y: i32,
    w: u32,
    h: u32,
    damage: u32,
    pixmap: u32,
    glx_pixmap: x11::glx::GLXPixmap,
    gl_texture: glow::Texture,
    dirty: bool,
    has_rgba: bool,
}

// ---------------------------------------------------------------------------
// Compositor
// ---------------------------------------------------------------------------

pub(super) struct Compositor {
    conn: Arc<RustConnection>,
    xlib_display: *mut x11::xlib::Display,
    tfp: TfpFunctions,
    glx_context: x11::glx::GLXContext,
    fbconfig_rgba: x11::glx::GLXFBConfig,
    fbconfig_rgb: x11::glx::GLXFBConfig,
    overlay_window: u32,
    glx_drawable: x11::glx::GLXDrawable,
    gl: glow::Context,
    program: glow::Program,
    quad_vao: glow::VertexArray,
    windows: HashMap<u32, WindowTexture>,
    screen_w: u32,
    screen_h: u32,
    #[allow(dead_code)]
    root: u32,
    damage_event_base: u8,
}

// Safety: The compositor is only accessed from the single-threaded X11 event loop.
// All raw pointers (Display*, GLXContext, etc.) are only used from that thread.
unsafe impl Send for Compositor {}

impl Drop for Compositor {
    fn drop(&mut self) {
        unsafe {
            self.gl.delete_program(self.program);
            self.gl.delete_vertex_array(self.quad_vao);
        }
        let wins: Vec<u32> = self.windows.keys().copied().collect();
        for w in wins {
            self.remove_window(w);
        }
        // Undo the MANUAL redirect so the X server renders windows normally again
        let _ = self.conn.composite_unredirect_subwindows(
            self.root,
            x11rb::protocol::composite::Redirect::MANUAL,
        );
        let _ = self.conn.composite_release_overlay_window(self.overlay_window);
        let _ = self.conn.flush();
        unsafe {
            x11::glx::glXDestroyContext(self.xlib_display, self.glx_context);
            x11::xlib::XCloseDisplay(self.xlib_display);
        }
    }
}

impl Compositor {
    pub(super) fn new(
        conn: Arc<RustConnection>,
        root: u32,
        screen_w: u32,
        screen_h: u32,
    ) -> Result<Self, String> {
        // 1. Check composite extension
        conn.composite_query_version(0, 4)
            .map_err(|e| format!("composite_query_version: {e}"))?
            .reply()
            .map_err(|e| format!("composite reply: {e}"))?;

        // 2. Redirect subwindows
        conn.composite_redirect_subwindows(root, x11rb::protocol::composite::Redirect::MANUAL)
            .map_err(|e| format!("redirect_subwindows: {e}"))?;

        // RAII guard: if we return Err after the redirect, undo it so the screen
        // doesn't go permanently black.
        struct RedirectGuard {
            conn: Arc<RustConnection>,
            root: u32,
            overlay: Option<u32>,
            active: bool,
        }
        impl Drop for RedirectGuard {
            fn drop(&mut self) {
                if self.active {
                    let _ = self.conn.composite_unredirect_subwindows(
                        self.root,
                        x11rb::protocol::composite::Redirect::MANUAL,
                    );
                    if let Some(ow) = self.overlay {
                        let _ = self.conn.composite_release_overlay_window(ow);
                    }
                    let _ = self.conn.flush();
                }
            }
        }
        let mut guard = RedirectGuard {
            conn: conn.clone(),
            root,
            overlay: None,
            active: true,
        };

        // 3. Damage extension
        conn.damage_query_version(1, 1)
            .map_err(|e| format!("damage_query_version: {e}"))?
            .reply()
            .map_err(|e| format!("damage reply: {e}"))?;

        let damage_ext = conn
            .extension_information(damage::X11_EXTENSION_NAME)
            .map_err(|e| format!("damage ext info: {e}"))?
            .ok_or("damage extension not available")?;
        let damage_event_base = damage_ext.first_event;

        // 4. Get overlay window
        let overlay_reply = conn
            .composite_get_overlay_window(root)
            .map_err(|e| format!("get_overlay_window: {e}"))?
            .reply()
            .map_err(|e| format!("overlay reply: {e}"))?;
        let overlay_window = overlay_reply.overlay_win;
        guard.overlay = Some(overlay_window);

        // 5. Make overlay input-passthrough using XFixes
        {
            let region = conn.generate_id().map_err(|e| format!("gen id: {e}"))?;
            conn.xfixes_create_region(region, &[])
                .map_err(|e| format!("create_region: {e}"))?;
            conn.xfixes_set_window_shape_region(
                overlay_window,
                x11rb::protocol::shape::SK::INPUT,
                0,
                0,
                region,
            )
            .map_err(|e| format!("set_window_shape_region: {e}"))?;
            conn.xfixes_destroy_region(region)
                .map_err(|e| format!("destroy_region: {e}"))?;
        }

        conn.flush().map_err(|e| format!("flush: {e}"))?;

        // 6. Open Xlib display for GLX
        let xlib_display = unsafe { x11::xlib::XOpenDisplay(std::ptr::null()) };
        if xlib_display.is_null() {
            return Err("XOpenDisplay failed".into());
        }
        // Install a no-op error handler for this Xlib display permanently.
        // The default Xlib handler calls exit() on ANY X error, which would
        // kill the entire WM for benign issues like stale pixmaps.
        unsafe {
            x11::xlib::XSetErrorHandler(Some(ignore_x_error));
        }

        let screen_num = unsafe { x11::xlib::XDefaultScreen(xlib_display) };

        // 6b. Verify GLX_EXT_texture_from_pixmap is advertised in the extension string.
        // glXGetProcAddress can return non-null pointers even when the extension
        // is not actually supported (e.g. indirect GLX in nested X servers).
        {
            let ext_str = unsafe {
                let raw = x11::glx::glXQueryExtensionsString(xlib_display, screen_num);
                if raw.is_null() {
                    ""
                } else {
                    std::ffi::CStr::from_ptr(raw).to_str().unwrap_or("")
                }
            };
            if !ext_str.contains("GLX_EXT_texture_from_pixmap") {
                unsafe { x11::xlib::XCloseDisplay(xlib_display) };
                // Guard will undo redirect + release overlay
                return Err(
                    "GLX_EXT_texture_from_pixmap not available (nested X server?)".into(),
                );
            }
            log::info!("GLX extensions: {ext_str}");
        }

        // 7. Choose FBConfig for GLX context.
        // We must pick an FBConfig whose visual matches the overlay window's
        // visual — otherwise glXCreateWindow / glXMakeContextCurrent will fail
        // (or even segfault) due to the visual mismatch.
        let overlay_visual_id = {
            let attrs = conn
                .get_window_attributes(overlay_window)
                .map_err(|e| format!("get_window_attributes(overlay): {e}"))?
                .reply()
                .map_err(|e| format!("overlay attrs reply: {e}"))?;
            attrs.visual
        };
        log::info!(
            "compositor: overlay visual=0x{:x}, choosing matching FBConfig...",
            overlay_visual_id
        );

        // First try: request an FBConfig matching the overlay's exact visual.
        let ctx_attrs_visual: Vec<i32> = vec![
            x11::glx::GLX_RENDER_TYPE,
            x11::glx::GLX_RGBA_BIT,
            x11::glx::GLX_DRAWABLE_TYPE,
            x11::glx::GLX_WINDOW_BIT,
            x11::glx::GLX_DOUBLEBUFFER,
            1,
            x11::glx::GLX_RED_SIZE,
            8,
            x11::glx::GLX_GREEN_SIZE,
            8,
            x11::glx::GLX_BLUE_SIZE,
            8,
            0,
        ];

        let mut n_configs: i32 = 0;
        let configs = unsafe {
            x11::glx::glXChooseFBConfig(
                xlib_display,
                screen_num,
                ctx_attrs_visual.as_ptr(),
                &mut n_configs,
            )
        };
        if configs.is_null() || n_configs == 0 {
            return Err("No suitable GLX FBConfig found".into());
        }

        // Pick the first FBConfig whose visual matches the overlay window.
        let mut ctx_fbconfig: x11::glx::GLXFBConfig = std::ptr::null_mut();
        unsafe {
            for i in 0..n_configs {
                let cfg = *configs.offset(i as isize);
                let vi = x11::glx::glXGetVisualFromFBConfig(xlib_display, cfg);
                if !vi.is_null() {
                    let vid = (*vi).visualid;
                    x11::xlib::XFree(vi as *mut _);
                    if vid == overlay_visual_id as u64 {
                        ctx_fbconfig = cfg;
                        break;
                    }
                }
            }
            // Fallback: if no exact match, just use the first config
            if ctx_fbconfig.is_null() {
                log::warn!(
                    "compositor: no FBConfig matching overlay visual 0x{:x}, using first available",
                    overlay_visual_id
                );
                ctx_fbconfig = *configs;
            }
            x11::xlib::XFree(configs as *mut _);
        }
        log::info!("compositor: found matching FBConfig for context (from {} candidates)", n_configs);

        // 8. Create GLX context
        log::info!("compositor: creating GLX context...");
        let glx_context = unsafe {
            x11::glx::glXCreateNewContext(
                xlib_display,
                ctx_fbconfig,
                x11::glx::GLX_RGBA_TYPE,
                std::ptr::null_mut(),
                1,
            )
        };
        if glx_context.is_null() {
            return Err("glXCreateNewContext failed".into());
        }

        log::info!("compositor: GLX context created, checking direct rendering...");
        // 8b. Require direct rendering — indirect GLX (e.g. in Xephyr) cannot
        //     do texture-from-pixmap because the pixmaps live in the nested
        //     server's address space, not the host GPU's.
        let is_direct = unsafe { x11::glx::glXIsDirect(xlib_display, glx_context) };
        if is_direct == 0 {
            log::warn!("GLX context is indirect — compositor cannot work (nested X server?)");
            unsafe {
                x11::glx::glXDestroyContext(xlib_display, glx_context);
                x11::xlib::XCloseDisplay(xlib_display);
            }
            return Err("GLX context is indirect; compositor requires direct rendering".into());
        }

        log::info!("compositor: direct rendering OK, creating GLX window on overlay 0x{:x}...", overlay_window);
        // 9. Create GLX window on the overlay
        let glx_drawable = unsafe {
            x11::glx::glXCreateWindow(
                xlib_display,
                ctx_fbconfig,
                overlay_window as _,
                std::ptr::null(),
            )
        };
        if glx_drawable == 0 {
            return Err("glXCreateWindow failed".into());
        }

        log::info!("compositor: GLX window created, making context current...");
        // Make context current
        let ok = unsafe {
            x11::glx::glXMakeContextCurrent(
                xlib_display,
                glx_drawable,
                glx_drawable,
                glx_context,
            )
        };
        if ok == 0 {
            return Err("glXMakeContextCurrent failed".into());
        }

        log::info!("compositor: context current OK, loading TFP extension functions...");
        // 10. Load TFP extension functions
        let bind_name = CString::new("glXBindTexImageEXT").unwrap();
        let release_name = CString::new("glXReleaseTexImageEXT").unwrap();
        let bind_ptr =
            unsafe { x11::glx::glXGetProcAddress(bind_name.as_ptr() as *const u8) };
        let release_ptr =
            unsafe { x11::glx::glXGetProcAddress(release_name.as_ptr() as *const u8) };
        if bind_ptr.is_none() || release_ptr.is_none() {
            return Err("glXBindTexImageEXT / glXReleaseTexImageEXT not available".into());
        }
        let tfp = TfpFunctions {
            bind: unsafe { std::mem::transmute(bind_ptr.unwrap()) },
            release: unsafe { std::mem::transmute(release_ptr.unwrap()) },
        };

        log::info!("compositor: TFP functions loaded, enabling VSync...");
        // 11. Try to enable VSync
        {
            let swap_name = CString::new("glXSwapIntervalEXT").unwrap();
            let swap_ptr =
                unsafe { x11::glx::glXGetProcAddress(swap_name.as_ptr() as *const u8) };
            if let Some(ptr) = swap_ptr {
                let swap_fn: GlXSwapIntervalEXT = unsafe { std::mem::transmute(ptr) };
                unsafe { swap_fn(xlib_display, glx_drawable, 1) };
            }
        }

        log::info!("compositor: finding TFP FBConfigs...");
        // 12. Find FBConfigs for TFP (RGBA and RGB)
        let tfp_rgba_attrs: Vec<i32> = vec![
            x11::glx::GLX_DRAWABLE_TYPE,
            x11::glx::GLX_PIXMAP_BIT,
            x11::glx::GLX_RENDER_TYPE,
            x11::glx::GLX_RGBA_BIT,
            GLX_BIND_TO_TEXTURE_RGBA_EXT,
            1,
            x11::glx::GLX_RED_SIZE,
            8,
            x11::glx::GLX_GREEN_SIZE,
            8,
            x11::glx::GLX_BLUE_SIZE,
            8,
            x11::glx::GLX_ALPHA_SIZE,
            8,
            0,
        ];
        let tfp_rgb_attrs: Vec<i32> = vec![
            x11::glx::GLX_DRAWABLE_TYPE,
            x11::glx::GLX_PIXMAP_BIT,
            x11::glx::GLX_RENDER_TYPE,
            x11::glx::GLX_RGBA_BIT,
            GLX_BIND_TO_TEXTURE_RGB_EXT,
            1,
            x11::glx::GLX_RED_SIZE,
            8,
            x11::glx::GLX_GREEN_SIZE,
            8,
            x11::glx::GLX_BLUE_SIZE,
            8,
            0,
        ];

        let mut n = 0i32;
        let cfgs_rgba = unsafe {
            x11::glx::glXChooseFBConfig(
                xlib_display,
                screen_num,
                tfp_rgba_attrs.as_ptr(),
                &mut n,
            )
        };
        let fbconfig_rgba = if !cfgs_rgba.is_null() && n > 0 {
            let c = unsafe { *cfgs_rgba };
            unsafe { x11::xlib::XFree(cfgs_rgba as *mut _) };
            c
        } else {
            std::ptr::null_mut()
        };

        let cfgs_rgb = unsafe {
            x11::glx::glXChooseFBConfig(
                xlib_display,
                screen_num,
                tfp_rgb_attrs.as_ptr(),
                &mut n,
            )
        };
        let fbconfig_rgb = if !cfgs_rgb.is_null() && n > 0 {
            let c = unsafe { *cfgs_rgb };
            unsafe { x11::xlib::XFree(cfgs_rgb as *mut _) };
            c
        } else {
            std::ptr::null_mut()
        };

        if fbconfig_rgba.is_null() && fbconfig_rgb.is_null() {
            return Err("No FBConfig for texture_from_pixmap".into());
        }
        log::info!(
            "compositor: TFP FBConfigs: rgba={} rgb={}",
            !fbconfig_rgba.is_null(),
            !fbconfig_rgb.is_null()
        );

        // 13. Create glow GL context
        log::info!("compositor: creating glow GL context...");
        let gl = unsafe {
            glow::Context::from_loader_function(|name| {
                let cname = CString::new(name).unwrap();
                match x11::glx::glXGetProcAddress(cname.as_ptr() as *const u8) {
                    Some(f) => f as *const _,
                    None => std::ptr::null(),
                }
            })
        };

        log::info!("compositor: glow GL context created, compiling shaders...");
        // 14. Compile shaders and create program
        let program = unsafe { Self::create_program(&gl)? };

        // 15. Create VAO (empty — vertex shader generates quad from gl_VertexID)
        let quad_vao = unsafe {
            let vao = gl
                .create_vertex_array()
                .map_err(|e| format!("create vao: {e}"))?;
            gl.bind_vertex_array(Some(vao));
            gl.bind_vertex_array(None);
            vao
        };

        // 16. Setup GL state
        unsafe {
            gl.viewport(0, 0, screen_w as i32, screen_h as i32);
            gl.enable(glow::BLEND);
            gl.blend_func(glow::ONE, glow::ONE_MINUS_SRC_ALPHA);
            gl.clear_color(0.0, 0.0, 0.0, 1.0);
        }

        log::info!(
            "Compositor initialized: {}x{}, overlay=0x{:x}, damage_event_base={}",
            screen_w,
            screen_h,
            overlay_window,
            damage_event_base
        );

        // Success — defuse the guard so it doesn't undo our redirect
        guard.active = false;

        Ok(Self {
            conn,
            xlib_display,
            tfp,
            glx_context,
            fbconfig_rgba,
            fbconfig_rgb,
            overlay_window,
            glx_drawable,
            gl,
            program,
            quad_vao,
            windows: HashMap::new(),
            screen_w,
            screen_h,
            root,
            damage_event_base,
        })
    }

    unsafe fn create_program(gl: &glow::Context) -> Result<glow::Program, String> {
        unsafe {
            let vs = gl
                .create_shader(glow::VERTEX_SHADER)
                .map_err(|e| format!("create vs: {e}"))?;
            gl.shader_source(vs, shaders::VERTEX_SHADER);
            gl.compile_shader(vs);
            if !gl.get_shader_compile_status(vs) {
                let info = gl.get_shader_info_log(vs);
                gl.delete_shader(vs);
                return Err(format!("vertex shader: {info}"));
            }

            let fs = gl
                .create_shader(glow::FRAGMENT_SHADER)
                .map_err(|e| format!("create fs: {e}"))?;
            gl.shader_source(fs, shaders::FRAGMENT_SHADER);
            gl.compile_shader(fs);
            if !gl.get_shader_compile_status(fs) {
                let info = gl.get_shader_info_log(fs);
                gl.delete_shader(vs);
                gl.delete_shader(fs);
                return Err(format!("fragment shader: {info}"));
            }

            let program = gl
                .create_program()
                .map_err(|e| format!("create program: {e}"))?;
            gl.attach_shader(program, vs);
            gl.attach_shader(program, fs);
            gl.link_program(program);
            if !gl.get_program_link_status(program) {
                let info = gl.get_program_info_log(program);
                gl.delete_program(program);
                gl.delete_shader(vs);
                gl.delete_shader(fs);
                return Err(format!("link program: {info}"));
            }
            gl.delete_shader(vs);
            gl.delete_shader(fs);
            Ok(program)
        }
    }

    pub(super) fn damage_event_base(&self) -> u8 {
        self.damage_event_base
    }

    // ----- Window management -----

    pub(super) fn add_window(&mut self, x11_win: u32, x: i32, y: i32, w: u32, h: u32) {
        if self.windows.contains_key(&x11_win) {
            return;
        }
        if w == 0 || h == 0 {
            return;
        }
        log::info!(
            "compositor: add_window START 0x{:x} {}x{} at ({},{})",
            x11_win, w, h, x, y
        );

        // Create damage
        let damage_id = match self.conn.generate_id() {
            Ok(id) => id,
            Err(e) => {
                log::warn!("compositor: generate_id for damage failed: {e}");
                return;
            }
        };
        if let Err(e) = self
            .conn
            .damage_create(damage_id, x11_win, damage::ReportLevel::NON_EMPTY)
        {
            log::warn!("compositor: damage_create failed for 0x{x11_win:x}: {e}");
            return;
        }

        // NameWindowPixmap
        let pixmap = match self.conn.generate_id() {
            Ok(id) => id,
            Err(e) => {
                log::warn!("compositor: generate_id for pixmap failed: {e}");
                let _ = self.conn.damage_destroy(damage_id);
                return;
            }
        };
        if let Err(e) = self.conn.composite_name_window_pixmap(x11_win, pixmap) {
            log::warn!("compositor: name_window_pixmap failed for 0x{x11_win:x}: {e}");
            let _ = self.conn.damage_destroy(damage_id);
            return;
        }
        // Flush x11rb AND sync Xlib so the pixmap XID is visible to GLX.
        let _ = self.conn.flush();

        // Determine if we use RGBA or RGB fbconfig based on the window's depth.
        // Most X11 windows are 24-bit (RGB); only 32-bit windows have alpha.
        // Using the wrong fbconfig for TFP causes black textures.
        let win_depth = self
            .conn
            .get_geometry(x11_win)
            .ok()
            .and_then(|c| c.reply().ok())
            .map(|g| g.depth)
            .unwrap_or(24);
        let use_rgba = win_depth == 32 && !self.fbconfig_rgba.is_null();
        let fbconfig = if use_rgba {
            self.fbconfig_rgba
        } else {
            self.fbconfig_rgb
        };
        if fbconfig.is_null() {
            log::warn!(
                "compositor: no fbconfig for depth={} win=0x{:x}",
                win_depth,
                x11_win
            );
            let _ = self.conn.free_pixmap(pixmap);
            let _ = self.conn.damage_destroy(damage_id);
            return;
        }
        let tex_fmt = if use_rgba {
            GLX_TEXTURE_FORMAT_RGBA_EXT
        } else {
            GLX_TEXTURE_FORMAT_RGB_EXT
        };

        // Create GLX pixmap for TFP
        let pixmap_attrs: Vec<i32> = vec![
            GLX_TEXTURE_TARGET_EXT,
            GLX_TEXTURE_2D_EXT,
            GLX_TEXTURE_FORMAT_EXT,
            tex_fmt,
            0,
        ];

        log::info!(
            "compositor: add_window 0x{:x} depth={} rgba={} pixmap=0x{:x}, calling glXCreatePixmap...",
            x11_win, win_depth, use_rgba, pixmap
        );
        let glx_pixmap = unsafe {
            // Sync both connections so the Xlib display can see the pixmap
            // created by x11rb.
            x11::xlib::XSync(self.xlib_display, 0);

            x11::glx::glXCreatePixmap(
                self.xlib_display,
                fbconfig,
                pixmap as _,
                pixmap_attrs.as_ptr(),
            )
        };
        log::info!("compositor: glXCreatePixmap returned 0x{:x}", glx_pixmap);
        if glx_pixmap == 0 {
            log::warn!("compositor: glXCreatePixmap failed for 0x{x11_win:x}");
            let _ = self.conn.free_pixmap(pixmap);
            let _ = self.conn.damage_destroy(damage_id);
            return;
        }

        // Create GL texture
        let gl_texture = unsafe {
            match self.gl.create_texture() {
                Ok(t) => t,
                Err(e) => {
                    log::warn!("compositor: create_texture failed: {e}");
                    x11::glx::glXDestroyPixmap(self.xlib_display, glx_pixmap);
                    let _ = self.conn.free_pixmap(pixmap);
                    let _ = self.conn.damage_destroy(damage_id);
                    return;
                }
            }
        };

        // Bind texture
        log::info!("compositor: add_window 0x{:x} binding TFP texture...", x11_win);
        unsafe {
            self.gl.bind_texture(glow::TEXTURE_2D, Some(gl_texture));
            self.gl.tex_parameter_i32(
                glow::TEXTURE_2D,
                glow::TEXTURE_MIN_FILTER,
                glow::LINEAR as i32,
            );
            self.gl.tex_parameter_i32(
                glow::TEXTURE_2D,
                glow::TEXTURE_MAG_FILTER,
                glow::LINEAR as i32,
            );
            self.gl.tex_parameter_i32(
                glow::TEXTURE_2D,
                glow::TEXTURE_WRAP_S,
                glow::CLAMP_TO_EDGE as i32,
            );
            self.gl.tex_parameter_i32(
                glow::TEXTURE_2D,
                glow::TEXTURE_WRAP_T,
                glow::CLAMP_TO_EDGE as i32,
            );
            (self.tfp.bind)(
                self.xlib_display,
                glx_pixmap,
                GLX_FRONT_LEFT_EXT,
                std::ptr::null(),
            );
            self.gl.bind_texture(glow::TEXTURE_2D, None);
        }
        log::info!("compositor: add_window 0x{:x} COMPLETE", x11_win);

        self.windows.insert(
            x11_win,
            WindowTexture {
                x,
                y,
                w,
                h,
                damage: damage_id,
                pixmap,
                glx_pixmap,
                gl_texture,
                dirty: true,
                has_rgba: use_rgba,
            },
        );

        log::debug!(
            "compositor: add_window 0x{:x} {}x{} at ({},{})",
            x11_win,
            w,
            h,
            x,
            y
        );
    }

    pub(super) fn remove_window(&mut self, x11_win: u32) {
        let Some(wt) = self.windows.remove(&x11_win) else {
            return;
        };

        unsafe {
            self.gl.bind_texture(glow::TEXTURE_2D, Some(wt.gl_texture));
            (self.tfp.release)(self.xlib_display, wt.glx_pixmap, GLX_FRONT_LEFT_EXT);
            self.gl.bind_texture(glow::TEXTURE_2D, None);
            self.gl.delete_texture(wt.gl_texture);
            x11::glx::glXDestroyPixmap(self.xlib_display, wt.glx_pixmap);
        }
        let _ = self.conn.free_pixmap(wt.pixmap);
        let _ = self.conn.damage_destroy(wt.damage);

        log::debug!("compositor: remove_window 0x{:x}", x11_win);
    }

    pub(super) fn update_geometry(&mut self, x11_win: u32, x: i32, y: i32, w: u32, h: u32) {
        if let Some(wt) = self.windows.get_mut(&x11_win) {
            let size_changed = wt.w != w || wt.h != h;
            wt.x = x;
            wt.y = y;

            if size_changed && w > 0 && h > 0 {
                wt.w = w;
                wt.h = h;

                // Release old texture binding and pixmap
                unsafe {
                    self.gl.bind_texture(glow::TEXTURE_2D, Some(wt.gl_texture));
                    (self.tfp.release)(self.xlib_display, wt.glx_pixmap, GLX_FRONT_LEFT_EXT);
                    self.gl.bind_texture(glow::TEXTURE_2D, None);
                    x11::glx::glXDestroyPixmap(self.xlib_display, wt.glx_pixmap);
                }
                let _ = self.conn.free_pixmap(wt.pixmap);

                // Re-create pixmap
                let pixmap = match self.conn.generate_id() {
                    Ok(id) => id,
                    Err(_) => return,
                };
                if self
                    .conn
                    .composite_name_window_pixmap(x11_win, pixmap)
                    .is_err()
                {
                    return;
                }
                let _ = self.conn.flush();

                let fbconfig = if wt.has_rgba {
                    self.fbconfig_rgba
                } else {
                    self.fbconfig_rgb
                };
                let tex_fmt = if wt.has_rgba {
                    GLX_TEXTURE_FORMAT_RGBA_EXT
                } else {
                    GLX_TEXTURE_FORMAT_RGB_EXT
                };
                let pixmap_attrs: Vec<i32> = vec![
                    GLX_TEXTURE_TARGET_EXT,
                    GLX_TEXTURE_2D_EXT,
                    GLX_TEXTURE_FORMAT_EXT,
                    tex_fmt,
                    0,
                ];
                let glx_pixmap = unsafe {
                    x11::xlib::XSync(self.xlib_display, 0);
                    x11::glx::glXCreatePixmap(
                        self.xlib_display,
                        fbconfig,
                        pixmap as _,
                        pixmap_attrs.as_ptr(),
                    )
                };
                if glx_pixmap == 0 {
                    let _ = self.conn.free_pixmap(pixmap);
                    return;
                }

                // Re-bind
                unsafe {
                    self.gl.bind_texture(glow::TEXTURE_2D, Some(wt.gl_texture));
                    (self.tfp.bind)(
                        self.xlib_display,
                        glx_pixmap,
                        GLX_FRONT_LEFT_EXT,
                        std::ptr::null(),
                    );
                    self.gl.bind_texture(glow::TEXTURE_2D, None);
                }

                wt.pixmap = pixmap;
                wt.glx_pixmap = glx_pixmap;
                wt.dirty = true;
            }
        }
    }

    pub(super) fn mark_damaged(&mut self, x11_win: u32) {
        if let Some(wt) = self.windows.get_mut(&x11_win) {
            wt.dirty = true;
            // Subtract damage so we get future notifications
            let _ = self.conn.damage_subtract(wt.damage, 0u32, 0u32);
        }
    }

    // ----- Rendering -----

    /// Render a composited frame.
    ///
    /// `scene` is an ordered list of (x11_win, x, y, w, h) from bottom to top.
    /// Returns true if a frame was rendered.
    pub(super) fn render_frame(&mut self, scene: &[(u32, i32, i32, u32, u32)]) -> bool {
        // One-time log to confirm rendering is happening
        static RENDER_LOG_COUNT: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);
        let count = RENDER_LOG_COUNT.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        if count < 5 || count % 500 == 0 {
            log::info!(
                "[compositor::render_frame] frame={} scene={} tracked={}",
                count,
                scene.len(),
                self.windows.len()
            );
            for &(win, x, y, w, h) in scene {
                let tracked = self.windows.contains_key(&win);
                log::info!(
                    "  win=0x{:x} at ({},{}) {}x{} tracked={}",
                    win, x, y, w, h, tracked
                );
            }
        }

        // Diagnostics: log mismatches between scene and tracked windows
        if !scene.is_empty() {
            let mut drawn = 0usize;
            let mut missed = 0usize;
            for &(win, _, _, _, _) in scene {
                if self.windows.contains_key(&win) {
                    drawn += 1;
                } else {
                    missed += 1;
                }
            }
            if missed > 0 {
                log::warn!(
                    "[compositor::render_frame] scene={} drawn={} missed={} (not tracked)",
                    scene.len(),
                    drawn,
                    missed
                );
            }
        }

        // Ensure context is current
        unsafe {
            x11::glx::glXMakeContextCurrent(
                self.xlib_display,
                self.glx_drawable,
                self.glx_drawable,
                self.glx_context,
            );
        }

        // Refresh dirty textures
        for &(win, _, _, _, _) in scene {
            if let Some(wt) = self.windows.get_mut(&win) {
                if wt.dirty {
                    unsafe {
                        self.gl.bind_texture(glow::TEXTURE_2D, Some(wt.gl_texture));
                        (self.tfp.release)(
                            self.xlib_display,
                            wt.glx_pixmap,
                            GLX_FRONT_LEFT_EXT,
                        );
                        (self.tfp.bind)(
                            self.xlib_display,
                            wt.glx_pixmap,
                            GLX_FRONT_LEFT_EXT,
                            std::ptr::null(),
                        );
                        self.gl.bind_texture(glow::TEXTURE_2D, None);
                    }
                    wt.dirty = false;
                }
            }
        }

        // Clear
        unsafe {
            self.gl
                .viewport(0, 0, self.screen_w as i32, self.screen_h as i32);
            self.gl.clear(glow::COLOR_BUFFER_BIT);
        }

        // Build orthographic projection matrix (column-major)
        let proj = ortho(
            0.0,
            self.screen_w as f32,
            self.screen_h as f32,
            0.0,
            -1.0,
            1.0,
        );

        unsafe {
            self.gl.use_program(Some(self.program));

            let loc_proj = self.gl.get_uniform_location(self.program, "u_projection");
            self.gl
                .uniform_matrix_4_f32_slice(loc_proj.as_ref(), false, &proj);

            let loc_rect = self.gl.get_uniform_location(self.program, "u_rect");
            let loc_tex = self.gl.get_uniform_location(self.program, "u_texture");
            let loc_opacity = self.gl.get_uniform_location(self.program, "u_opacity");
            self.gl.uniform_1_i32(loc_tex.as_ref(), 0);

            self.gl.bind_vertex_array(Some(self.quad_vao));

            // Draw each window quad bottom-to-top
            for &(win, x, y, w, h) in scene {
                if let Some(wt) = self.windows.get(&win) {
                    // For RGBA windows, pass through the texture alpha;
                    // for RGB windows, force opacity to 1.0 since TFP alpha is undefined.
                    let opacity = if wt.has_rgba { -1.0f32 } else { 1.0f32 };
                    self.gl.uniform_1_f32(loc_opacity.as_ref(), opacity);
                    self.gl.uniform_4_f32(
                        loc_rect.as_ref(),
                        x as f32,
                        y as f32,
                        w as f32,
                        h as f32,
                    );
                    self.gl.active_texture(glow::TEXTURE0);
                    self.gl.bind_texture(glow::TEXTURE_2D, Some(wt.gl_texture));
                    self.gl.draw_arrays(glow::TRIANGLE_STRIP, 0, 4);
                }
            }

            self.gl.bind_vertex_array(None);
            self.gl.use_program(None);
        }

        // Swap
        unsafe {
            x11::glx::glXSwapBuffers(self.xlib_display, self.glx_drawable);
        }

        true
    }

    pub(super) fn tracked_window_count(&self) -> usize {
        self.windows.len()
    }

    #[allow(dead_code)]
    pub(super) fn has_window(&self, x11_win: u32) -> bool {
        self.windows.contains_key(&x11_win)
    }
}

/// Dummy X error handler used to suppress errors from stale TFP pixmaps.
unsafe extern "C" fn ignore_x_error(
    _display: *mut x11::xlib::Display,
    _event: *mut x11::xlib::XErrorEvent,
) -> i32 {
    0
}

// Orthographic projection matrix (column-major for OpenGL)
fn ortho(left: f32, right: f32, bottom: f32, top: f32, near: f32, far: f32) -> [f32; 16] {
    let tx = -(right + left) / (right - left);
    let ty = -(top + bottom) / (top - bottom);
    let tz = -(far + near) / (far - near);
    #[rustfmt::skip]
    let m = [
        2.0 / (right - left), 0.0,                  0.0,                 0.0,
        0.0,                  2.0 / (top - bottom),  0.0,                 0.0,
        0.0,                  0.0,                  -2.0 / (far - near),  0.0,
        tx,                   ty,                    tz,                  1.0,
    ];
    m
}

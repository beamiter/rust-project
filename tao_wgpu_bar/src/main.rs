use anyhow::Result;
use cairo::{Context, Format, ImageSurface};
use log::warn;
use pango::FontDescription;
use shared_structures::SharedRingBuffer;
use std::env;
use std::sync::Arc;
use std::thread;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};
use tao::event_loop::EventLoopBuilder;

// tao 替换 winit
use tao::{
    dpi::{LogicalSize, PhysicalSize},
    event::{Event, WindowEvent},
    event_loop::{ControlFlow, EventLoop, EventLoopProxy},
    window::{Window, WindowBuilder, WindowId},
};

use xbar_core::{
    AppState, BarConfig, ShapeStyle, default_colors, draw_bar, initialize_logging,
    spawn_shared_eventfd_notifier,
};

// ===== 新增：wgpu 封装（保持不变，仅将 Window 改成 tao::window::Window） =====
#[allow(unused)]
struct Gpu {
    surface: wgpu::Surface<'static>,
    device: wgpu::Device,
    queue: wgpu::Queue,
    config: wgpu::SurfaceConfiguration,
    surface_format: wgpu::TextureFormat,
    cpu_tex: wgpu::Texture,
    cpu_tex_view: wgpu::TextureView,
    cpu_tex_format: wgpu::TextureFormat,
    sampler: wgpu::Sampler,
    pipeline: wgpu::RenderPipeline,
    bind_group: wgpu::BindGroup,
    width: u32,
    height: u32,
}

// 全屏三角形采样纹理的 WGSL 着色器
const FULLSCREEN_WGSL: &str = r#"
@group(0) @binding(0) var tex: texture_2d<f32>;
@group(0) @binding(1) var samp: sampler;

struct VSOut {
  @builtin(position) pos: vec4<f32>,
  @location(0) uv: vec2<f32>,
};

@vertex
fn vs(@builtin(vertex_index) vid: u32) -> VSOut {
  var pos = array<vec2<f32>, 3>(
    vec2(-1.0, -1.0),
    vec2( 3.0, -1.0),
    vec2(-1.0,  3.0),
  )[vid];

  var out: VSOut;
  out.pos = vec4(pos, 0.0, 1.0);

  let uv = 0.5 * pos + vec2(0.5, 0.5);
  out.uv = vec2(uv.x, 1.0 - uv.y);
  return out;
}

@fragment
fn fs(in: VSOut) -> @location(0) vec4<f32> {
  return textureSample(tex, samp, in.uv);
}
"#;

impl Gpu {
    async fn new(window: Arc<Window>, width: u32, height: u32) -> Result<Self> {
        let instance = wgpu::Instance::default();

        // tao Window 提供 raw_window_handle/raw_display_handle，wgpu 可直接创建 Surface
        // 注意：不同 wgpu 版本的 create_surface 接口略有差异，请与项目当前 wgpu 版本保持一致
        let surface = instance.create_surface(window.clone())?;

        let adapter = instance
            .request_adapter(&wgpu::RequestAdapterOptions {
                power_preference: wgpu::PowerPreference::LowPower,
                compatible_surface: Some(&surface),
                force_fallback_adapter: false,
            })
            .await?;

        let (device, queue) = adapter
            .request_device(&wgpu::DeviceDescriptor {
                label: Some("wgpu-device"),
                required_features: wgpu::Features::empty(),
                required_limits: wgpu::Limits::default(),
                experimental_features: Default::default(),
                memory_hints: Default::default(),
                trace: Default::default(),
            })
            .await?;

        let caps = surface.get_capabilities(&adapter);
        let surface_format = caps
            .formats
            .iter()
            .copied()
            .find(|f| f.is_srgb())
            .unwrap_or(caps.formats[0]);

        let config = wgpu::SurfaceConfiguration {
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
            format: surface_format,
            width,
            height,
            present_mode: wgpu::PresentMode::Fifo,
            alpha_mode: caps.alpha_modes[0],
            view_formats: vec![],
            desired_maximum_frame_latency: 2,
        };
        surface.configure(&device, &config);

        // 上传纹理格式：优先 BGRA（匹配 Cairo 的 ARgb32 小端 BGRA）
        let cpu_tex_format = if surface_format == wgpu::TextureFormat::Bgra8UnormSrgb {
            wgpu::TextureFormat::Bgra8UnormSrgb
        } else {
            wgpu::TextureFormat::Rgba8UnormSrgb
        };

        let cpu_tex = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("cpu-upload-texture"),
            size: wgpu::Extent3d {
                width,
                height,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: cpu_tex_format,
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        });
        let cpu_tex_view = cpu_tex.create_view(&wgpu::TextureViewDescriptor::default());

        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("nearest-sampler"),
            address_mode_u: wgpu::AddressMode::ClampToEdge,
            address_mode_v: wgpu::AddressMode::ClampToEdge,
            address_mode_w: wgpu::AddressMode::ClampToEdge,
            mag_filter: wgpu::FilterMode::Nearest,
            min_filter: wgpu::FilterMode::Nearest,
            mipmap_filter: wgpu::FilterMode::Nearest,
            ..Default::default()
        });

        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("fullscreen-shader"),
            source: wgpu::ShaderSource::Wgsl(FULLSCREEN_WGSL.into()),
        });

        let bind_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("tex-sampler-layout"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: true },
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 1,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                    count: None,
                },
            ],
        });

        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("pipeline-layout"),
            bind_group_layouts: &[&bind_layout],
            push_constant_ranges: &[],
        });

        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("fullscreen-pipeline"),
            layout: Some(&pipeline_layout),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: Some("vs"),
                buffers: &[],
                compilation_options: wgpu::PipelineCompilationOptions::default(),
            },
            fragment: Some(wgpu::FragmentState {
                module: &shader,
                entry_point: Some("fs"),
                targets: &[Some(wgpu::ColorTargetState {
                    format: surface_format,
                    blend: Some(wgpu::BlendState::ALPHA_BLENDING),
                    write_mask: wgpu::ColorWrites::ALL,
                })],
                compilation_options: wgpu::PipelineCompilationOptions::default(),
            }),
            primitive: wgpu::PrimitiveState::default(),
            depth_stencil: None,
            multisample: wgpu::MultisampleState::default(),
            multiview: None,
            cache: None,
        });

        let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("tex-sampler-bindgroup"),
            layout: &bind_layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: wgpu::BindingResource::TextureView(&cpu_tex_view),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::Sampler(&sampler),
                },
            ],
        });

        Ok(Self {
            surface,
            device,
            queue,
            config,
            surface_format,
            cpu_tex,
            cpu_tex_view,
            cpu_tex_format,
            sampler,
            pipeline,
            bind_group,
            width,
            height,
        })
    }

    fn resize(&mut self, width: u32, height: u32) {
        if width == 0 || height == 0 {
            return;
        }
        self.width = width;
        self.height = height;
        self.config.width = width;
        self.config.height = height;
        self.surface.configure(&self.device, &self.config);

        self.cpu_tex = self.device.create_texture(&wgpu::TextureDescriptor {
            label: Some("cpu-upload-texture"),
            size: wgpu::Extent3d {
                width,
                height,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: self.cpu_tex_format,
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        });
        self.cpu_tex_view = self
            .cpu_tex
            .create_view(&wgpu::TextureViewDescriptor::default());

        // 重新绑定（纹理视图变了）
        let bind_layout = self.pipeline.get_bind_group_layout(0);
        self.bind_group = self.device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("tex-sampler-bindgroup"),
            layout: &bind_layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: wgpu::BindingResource::TextureView(&self.cpu_tex_view),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::Sampler(&self.sampler),
                },
            ],
        });
    }

    fn upload_and_present(&self, cpu_data: &[u8], stride: u32) -> Result<()> {
        // 行对齐到 256 字节
        let bpr = stride;
        let height = self.height;
        let width = self.width;
        let aligned_bpr = ((bpr + 255) / 256) * 256;

        let mut padded: Vec<u8>;
        let data_ref: &[u8] = if aligned_bpr == bpr {
            cpu_data
        } else {
            padded = vec![0u8; aligned_bpr as usize * height as usize];
            for y in 0..height as usize {
                let src = &cpu_data[y * bpr as usize..(y + 1) * bpr as usize];
                let dst =
                    &mut padded[y * aligned_bpr as usize..y * aligned_bpr as usize + bpr as usize];
                dst.copy_from_slice(src);
            }
            &padded
        };

        self.queue.write_texture(
            wgpu::TexelCopyTextureInfo {
                texture: &self.cpu_tex,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            data_ref,
            wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(aligned_bpr),
                rows_per_image: Some(height),
            },
            wgpu::Extent3d {
                width,
                height,
                depth_or_array_layers: 1,
            },
        );

        let frame = match self.surface.get_current_texture() {
            Ok(f) => f,
            Err(e) => {
                log::warn!("get_current_texture error: {e}, reconfiguring surface");
                self.surface.configure(&self.device, &self.config);
                self.surface.get_current_texture()?
            }
        };
        let view = frame
            .texture
            .create_view(&wgpu::TextureViewDescriptor::default());

        let mut encoder = self
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("present-encoder"),
            });

        {
            let mut rp = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("present-pass"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &view,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color::BLACK),
                        store: wgpu::StoreOp::Store,
                    },
                    depth_slice: None,
                })],
                depth_stencil_attachment: None,
                occlusion_query_set: None,
                timestamp_writes: None,
            });

            rp.set_pipeline(&self.pipeline);
            rp.set_bind_group(0, &self.bind_group, &[]);
            rp.draw(0..3, 0..1);
        }

        self.queue.submit(Some(encoder.finish()));
        frame.present();
        Ok(())
    }
}

// ===== 原有：事件与应用逻辑（保留） =====

#[derive(Debug, Clone, Copy)]
enum UserEvent {
    Tick,
    SharedUpdated,
}

// Tick 线程：始终按秒对齐唤醒（低开销），用 state.show_seconds 决定是否重绘
fn spawn_tick_thread(proxy: EventLoopProxy<UserEvent>) {
    thread::spawn(move || {
        loop {
            let now = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap_or_else(|_| Duration::from_secs(0));
            let subns = now.subsec_nanos() as u64;
            let sleep_dur = Duration::from_nanos(1_000_000_000u64.saturating_sub(subns));
            thread::sleep(sleep_dur);
            let _ = proxy.send_event(UserEvent::Tick);
        }
    });
}

fn spawn_shared_thread(proxy: EventLoopProxy<UserEvent>, shared_efd: Option<i32>) {
    if let Some(efd) = shared_efd {
        thread::spawn(move || {
            let mut buf8 = [0u8; 8];
            let mut pfd = libc::pollfd {
                fd: efd,
                events: libc::POLLIN,
                revents: 0,
            };
            loop {
                let pr = unsafe { libc::poll(&mut pfd as *mut libc::pollfd, 1, -1) };
                if pr < 0 {
                    let err = std::io::Error::last_os_error();
                    if let Some(code) = err.raw_os_error() {
                        if code == libc::EINTR {
                            continue;
                        }
                    }
                    warn!("[shared-thread] poll error: {}", err);
                    thread::sleep(Duration::from_millis(50));
                    continue;
                }
                if (pfd.revents & libc::POLLIN) != 0 {
                    let r = unsafe { libc::read(efd, buf8.as_mut_ptr() as *mut _, buf8.len()) };
                    if r == 8 {
                        let _ = proxy.send_event(UserEvent::SharedUpdated);
                    } else if r < 0 {
                        let err = std::io::Error::last_os_error();
                        if let Some(code) = err.raw_os_error() {
                            if code == libc::EINTR {
                                continue;
                            }
                        }
                        warn!("[shared-thread] eventfd read error: {}", err);
                        thread::sleep(Duration::from_millis(50));
                    }
                }
            }
        });
    }
}

struct App {
    window_id: Option<WindowId>,
    window: Option<Arc<Window>>,

    colors: xbar_core::Colors,
    cfg: BarConfig,
    font: FontDescription,
    state: AppState,

    scale_factor: f64,
    logical_size: LogicalSize<f64>,
    last_physical_size: PhysicalSize<u32>,

    last_monitor_update: Instant,
    last_cursor_pos_px: Option<(i32, i32)>,

    gpu: Option<Gpu>,
    cpu_frame: Vec<u8>,

    last_time_bucket: u64,
}

impl App {
    fn new(
        shared_buffer: Option<Arc<SharedRingBuffer>>,
        logical_size: LogicalSize<f64>,
        scale: f64,
    ) -> Self {
        let colors = default_colors();
        let cfg = BarConfig {
            bar_height: 40,
            padding_x: 8.0,
            padding_y: 4.0,
            tag_spacing: 6.0,
            pill_hpadding: 10.0,
            pill_radius: 8.0,
            shape_style: ShapeStyle::Pill,
            time_icon: "",
            screenshot_label: " Screenshot",
        };
        let font = FontDescription::from_string("JetBrainsMono Nerd Font 11");
        let state = AppState::new(shared_buffer);

        Self {
            window_id: None,
            window: None,
            colors,
            cfg,
            font,
            state,
            scale_factor: scale,
            logical_size,
            last_physical_size: PhysicalSize::new(
                logical_size.width.round() as u32,
                logical_size.height.round() as u32,
            ),
            last_monitor_update: Instant::now(),
            last_cursor_pos_px: None,
            gpu: None,
            cpu_frame: Vec::new(),
            last_time_bucket: 0,
        }
    }

    fn init_window_and_gpu(&mut self, event_loop: &EventLoop<UserEvent>) -> Result<()> {
        // 选择主显示器（如无则选首个）
        let primary = event_loop
            .primary_monitor()
            .or_else(|| event_loop.available_monitors().next());
        let scale = primary.as_ref().map(|m| m.scale_factor()).unwrap_or(1.0);
        self.scale_factor = scale;

        let screen_size: PhysicalSize<u32> = primary
            .as_ref()
            .map(|m| m.size())
            .unwrap_or(PhysicalSize::new(1920, 1080));

        let width_px = screen_size.width;
        let height_px = self.cfg.bar_height as u32;

        self.logical_size = LogicalSize::new(
            (width_px as f64) / self.scale_factor,
            (height_px as f64) / self.scale_factor,
        );
        self.last_physical_size = PhysicalSize::new(width_px, height_px);

        // 创建窗口
        let window = WindowBuilder::new()
            .with_title("tao_wgpu_bar")
            .with_inner_size(self.logical_size)
            .with_decorations(false)
            .with_resizable(true)
            .with_visible(true)
            .with_transparent(false)
            .build(event_loop)
            .expect("create_window failed");

        let win_id = window.id();
        let arc = Arc::new(window);

        // 初始化 wgpu
        let width_px = (self.logical_size.width * self.scale_factor).round() as u32;
        let height_px = (self.logical_size.height * self.scale_factor).round() as u32;

        self.window = Some(arc.clone());
        self.gpu = Some(
            pollster::block_on(Gpu::new(arc.clone(), width_px, height_px))
                .expect("wgpu init failed"),
        );
        self.cpu_frame = vec![0u8; (width_px * height_px * 4) as usize];
        self.window_id = Some(win_id);

        // 时间 bucket 初始化，并请求首次重绘
        self.last_time_bucket = self.current_time_bucket();
        self.request_redraw();

        Ok(())
    }

    fn redraw(&mut self) -> anyhow::Result<()> {
        if self.window_id.is_none() {
            return Ok(());
        }

        let width_px = (self.logical_size.width * self.scale_factor).round() as i32;
        let height_px = (self.logical_size.height * self.scale_factor).round() as i32;
        let stride = width_px
            .checked_mul(4)
            .ok_or_else(|| anyhow::anyhow!("stride overflow"))?;

        if let Some(gpu) = self.gpu.as_ref() {
            // 确保 cpu_frame 大小匹配
            let needed = (width_px as usize) * (height_px as usize) * 4;
            if self.cpu_frame.len() != needed {
                self.cpu_frame.resize(needed, 0);
            }

            // Cairo 绘制到 CPU 帧缓冲
            let surface = unsafe {
                ImageSurface::create_for_data_unsafe(
                    self.cpu_frame.as_mut_ptr(),
                    Format::ARgb32,
                    width_px,
                    height_px,
                    stride,
                )?
            };
            let cr = Context::new(&surface)?;
            cr.save()?;
            cr.set_source_rgba(0.0, 0.0, 0.0, 1.0);
            cr.set_operator(cairo::Operator::Source);
            cr.paint()?;
            cr.restore()?;
            let w_u16 = (width_px as u32).min(u16::MAX as u32) as u16;
            let h_u16 = (height_px as u32).min(u16::MAX as u32) as u16;
            draw_bar(
                &cr,
                w_u16,
                h_u16,
                &self.colors,
                &mut self.state,
                &self.font,
                &self.cfg,
            )?;
            surface.flush();

            gpu.upload_and_present(&self.cpu_frame, stride as u32)?;
        }
        Ok(())
    }

    #[inline]
    fn request_redraw(&self) {
        if let Some(win) = self.window.as_ref() {
            win.request_redraw();
        }
    }

    fn update_hover_and_redraw(&mut self, px: i32, py: i32) {
        if self.state.update_hover(px as i16, py as i16) {
            self.request_redraw();
        }
    }

    fn handle_button(&mut self, px: i32, py: i32, button_id: u8) {
        let prev_show_seconds = self.state.show_seconds;

        if self.state.handle_buttons(px as i16, py as i16, button_id) {
            if self.state.show_seconds != prev_show_seconds {
                self.last_time_bucket = self.current_time_bucket();
            }
            self.request_redraw();
        }
    }

    fn current_time_bucket(&self) -> u64 {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_else(|_| Duration::from_secs(0));
        if self.state.show_seconds {
            now.as_secs()
        } else {
            now.as_secs() / 60
        }
    }

    // 将 winit 的窗口事件处理逻辑直接迁移为方法，供 run 闭包调用
    fn on_user_event(&mut self, event: UserEvent) {
        match event {
            UserEvent::Tick => {
                let mut need_redraw = false;
                let bucket = self.current_time_bucket();
                if bucket != self.last_time_bucket {
                    self.last_time_bucket = bucket;
                    log::trace!("redraw by time bucket update: {}", bucket);
                    need_redraw = true;
                }

                if self.last_monitor_update.elapsed() >= Duration::from_secs(2) {
                    self.state.system_monitor.update_if_needed();
                    self.state.audio_manager.update_if_needed();
                    self.last_monitor_update = Instant::now();
                    log::trace!("maybe redraw by system update");
                    need_redraw = true;
                }

                if need_redraw {
                    self.request_redraw();
                }
            }
            UserEvent::SharedUpdated => {
                let mut need_redraw = false;
                if let Some(buf_arc) = self.state.shared_buffer.as_ref().cloned() {
                    match buf_arc.try_read_latest_message() {
                        Ok(Some(msg)) => {
                            log::trace!("redraw by msg: {:?}", msg);
                            self.state.update_from_shared(msg);
                            need_redraw = true;
                        }
                        Ok(None) => { /* 没有消息 */ }
                        Err(e) => {
                            warn!("Shared try_read_latest_message failed: {}", e);
                        }
                    }
                }
                if need_redraw {
                    self.request_redraw();
                }
            }
        }
    }

    fn on_window_event(&mut self, window_id: WindowId, event: WindowEvent) -> Option<ControlFlow> {
        if Some(window_id) != self.window_id {
            return None;
        }

        match event {
            WindowEvent::CloseRequested => {
                // 请求退出
                return Some(ControlFlow::Exit);
            }
            WindowEvent::Resized(new_size) => {
                self.last_physical_size = new_size;
                self.logical_size = new_size.to_logical::<f64>(self.scale_factor);

                if let Some(gpu) = self.gpu.as_mut() {
                    let w = (self.logical_size.width * self.scale_factor).round() as u32;
                    let h = (self.logical_size.height * self.scale_factor).round() as u32;
                    gpu.resize(w, h);
                    self.cpu_frame.resize((w * h * 4) as usize, 0);
                }

                self.request_redraw();
            }
            // tao 的 ScaleFactorChanged 事件包含 scale_factor 与 new_inner_size（可能是一个建议的逻辑尺寸）
            WindowEvent::ScaleFactorChanged { scale_factor, .. } => {
                self.scale_factor = scale_factor;
                self.logical_size = self.last_physical_size.to_logical::<f64>(self.scale_factor);

                if let Some(gpu) = self.gpu.as_mut() {
                    let w = (self.logical_size.width * self.scale_factor).round() as u32;
                    let h = (self.logical_size.height * self.scale_factor).round() as u32;
                    gpu.resize(w, h);
                    self.cpu_frame.resize((w * h * 4) as usize, 0);
                }

                self.request_redraw();
            }
            WindowEvent::CursorMoved { position, .. } => {
                let px = position.x.round() as i32;
                let py = position.y.round() as i32;
                self.last_cursor_pos_px = Some((px, py));
                self.update_hover_and_redraw(px, py);
                log::trace!(
                    "cursor px={}, py={}, time_rect={:?}, ss_rect={:?}",
                    px,
                    py,
                    self.state.time_rect,
                    self.state.ss_rect
                );
            }
            WindowEvent::MouseInput { state, button, .. } => {
                use tao::event::{ElementState, MouseButton};
                if state == ElementState::Pressed {
                    if let Some((px, py)) = self.last_cursor_pos_px {
                        let button_id = match button {
                            MouseButton::Left => 1,
                            MouseButton::Middle => 2,
                            MouseButton::Right => 3,
                            MouseButton::Other(n) => n as u8,
                            _ => todo!(),
                        };
                        self.handle_button(px, py, button_id);
                    }
                }
            }
            _ => {}
        }
        None
    }
}

fn main() -> Result<()> {
    // 参数
    let args: Vec<String> = env::args().collect();
    let shared_path = args.get(1).cloned().unwrap_or_default();

    // 日志
    if let Err(e) = initialize_logging("tao_wgpu_bar", &shared_path) {
        eprintln!("Failed to initialize logging: {}", e);
        std::process::exit(1);
    }

    // 共享内存与通知
    let shared_buffer = SharedRingBuffer::create_shared_ring_buffer_aux(&shared_path).map(Arc::new);
    let shared_efd = spawn_shared_eventfd_notifier(shared_buffer.clone(), false);

    // 事件循环与代理（tao 0.34.3）
    let event_loop: EventLoop<UserEvent> = EventLoopBuilder::with_user_event().build();
    let proxy = event_loop.create_proxy();

    // 后台线程：Tick 与 SharedUpdated
    spawn_tick_thread(proxy.clone());
    spawn_shared_thread(proxy.clone(), shared_efd);

    // 初始逻辑尺寸，实际在 init_window_and_gpu 中根据显示器设置
    let logical_size = LogicalSize::new(800.0, 40.0);
    let mut app = App::new(shared_buffer, logical_size, 1.0);

    // 在进入 run 前创建窗口与 wgpu（也可在 NewEvents(StartCause::Init) 里做）
    app.init_window_and_gpu(&event_loop)?;

    // 运行事件循环
    event_loop.run(move |event, _event_loop_window_target, control_flow| {
        *control_flow = ControlFlow::Wait;

        match event {
            Event::UserEvent(ev) => {
                app.on_user_event(ev);
            }
            Event::WindowEvent {
                window_id, event, ..
            } => {
                if let Some(cf) = app.on_window_event(window_id, event) {
                    *control_flow = cf;
                }
            }
            Event::MainEventsCleared => {
                // 周期性刷新由 Tick/SharedUpdated 驱动，这里无需操作
            }
            Event::RedrawRequested(window_id) => {
                if Some(window_id) == app.window_id {
                    if let Err(e) = app.redraw() {
                        warn!("redraw error (Event::RedrawRequested): {}", e);
                    }
                }
            }
            _ => {}
        }
    });
}

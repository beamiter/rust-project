// src/backend/wayland/render.rs
use winit::{event::{Event, WindowEvent, ElementState, MouseButton, KeyboardInput}, event_loop::ControlFlow};
use smithay::{
    backend::renderer::gles2::Gles2Renderer,
    wayland::output::{Output, Mode},
    desktop::{Space, Window},
};
use crossbeam_channel::Sender;
use std::sync::{Arc, Mutex};

pub struct WinitRenderer {
    pub space: Arc<Mutex<Space<Window>>>,
    pub output_manager: Arc<Mutex<smithay::wayland::output::OutputManagerState>>,
    pub registry: Arc<Mutex<super::window_ops::WaylandRegistry>>,
    pub tx: Sender<crate::backend::api::BackendEvent>,
}

impl WinitRenderer {
    pub fn run(self) -> Result<(), Box<dyn std::error::Error>> {
        let event_loop = winit::event_loop::EventLoop::new();
        let window = winit::window::WindowBuilder::new()
            .with_title("JWM Wayland Compositor")
            .with_inner_size(winit::dpi::LogicalSize::new(1280.0, 800.0))
            .build(&event_loop)?;

        // 用 glow 创建 GL 上下文（略），构造 Gles2Renderer
        // let renderer = Gles2Renderer::new(context, /* logger */ None)?;

        // 注册 Output 到 Smithay
        let size = window.inner_size();
        {
            let mut reg = self.registry.lock().unwrap();
            reg.screen_w = size.width as i32;
            reg.screen_h = size.height as i32;
        }
        // let dh = /*DisplayHandle*/ todo!();
        // let output = Output::new(&dh, "JWM-Output", /* physical properties */);
        // output.change_current_state(/* position=0,0 */ None, Some(Mode { size: (size.width as i32, size.height as i32), refresh: 60000 }));
        // self.output_manager.lock().unwrap().add_output(output.clone());

        // 简单渲染循环：收到需要重绘时绘制 Space
        event_loop.run(move |event, _, control_flow| {
            *control_flow = ControlFlow::Poll;
            match event {
                Event::RedrawRequested(_) => {
                    // renderer.bind(window_surface);
                    // draw Space: 遍历 self.space.lock().unwrap().elements() 并绘制（示例略）
                    // renderer.unbind();
                }
                Event::WindowEvent { event, .. } => match event {
                    WindowEvent::Resized(new_size) => {
                        let mut reg = self.registry.lock().unwrap();
                        reg.screen_w = new_size.width as i32;
                        reg.screen_h = new_size.height as i32;
                        // 更新 Output mode 与请求客户端重绘（send_configure）
                        // 广播 ConfigureNotify 到 JWM（BackendEvent）
                        let _ = self.tx.send(crate::backend::api::BackendEvent::ConfigureNotify {
                            window: crate::backend::common_define::WindowId(0), // root
                            x: 0, y: 0,
                            w: new_size.width as u16, h: new_size.height as u16,
                        });
                    }
                    WindowEvent::CursorMoved { position, .. } => {
                        let x = position.x as i32; let y = position.y as i32;
                        // 命中检测 Space，找到顶部窗口 id（如需）
                        let win_id = {
                            // 可选：space.under(Point::from((x,y))) 找到元素并映射到 registry id
                            crate::backend::common_define::WindowId(0) // 暂用 root
                        };
                        let _ = self.tx.send(crate::backend::api::BackendEvent::MotionNotify {
                            window: win_id, root_x: x as i16, root_y: y as i16, time: 0
                        });
                    }
                    WindowEvent::MouseInput { state, button, .. } => {
                        let pressed = state == ElementState::Pressed;
                        let btn_u8 = match button {
                            MouseButton::Left => 1,
                            MouseButton::Right => 3,
                            MouseButton::Middle => 2,
                            MouseButton::Other(x) => x,
                        };
                        if pressed {
                            let _ = self.tx.send(crate::backend::api::BackendEvent::ButtonPress {
                                window: crate::backend::common_define::WindowId(0),
                                state: 0, detail: btn_u8, time: 0,
                            });
                        } else {
                            let _ = self.tx.send(crate::backend::api::BackendEvent::ButtonRelease {
                                window: crate::backend::common_define::WindowId(0), time: 0
                            });
                        }
                    }
                    WindowEvent::KeyboardInput { input: KeyboardInput { /*virtual_keycode*/ _, scancode, state, .. }, .. } => {
                        if state == ElementState::Pressed {
                            let keycode = (scancode & 0xFF) as u8;
                            let _ = self.tx.send(crate::backend::api::BackendEvent::KeyPress { keycode, state: 0 });
                        }
                    }
                    WindowEvent::CloseRequested => {
                        *control_flow = ControlFlow::Exit;
                    }
                    _ => {}
                },
                _ => {}
            }
        });
    }
}

# rust-project (Archived)

> ⚠️ **This repository has been archived.** All packages have been split into individual repositories.

This was a Rust mono repo containing multiple related projects. It has been split into independent repositories for better maintainability.

## Migrated Repositories

### Core Libraries
| Repository | Description |
|------------|-------------|
| [shared_structures](https://github.com/beamiter/shared_structures) | Shared data structures and IPC utilities |
| [xbar_core](https://github.com/beamiter/xbar_core) | Core status bar functionality (depends on shared_structures) |

### Main Projects
| Repository | Description |
|------------|-------------|
| [jwm](https://github.com/beamiter/jwm) | X11/Wayland window manager |
| [jterm4](https://github.com/beamiter/jterm4) | GTK4-based terminal emulator |

### Standalone Tools
| Repository | Description |
|------------|-------------|
| [clash_client](https://github.com/beamiter/clash_client) | Clash proxy client with egui UI |
| [eframe_toy](https://github.com/beamiter/eframe_toy) | Eframe/egui experiments |

### Status Bar Implementations (GUI Frameworks)
| Repository | Description |
|------------|-------------|
| [egui_bar](https://github.com/beamiter/egui_bar) | egui-based status bar |
| [gtk_bar](https://github.com/beamiter/gtk_bar) | GTK4-based status bar |
| [iced_bar](https://github.com/beamiter/iced_bar) | Iced-based status bar |
| [relm_bar](https://github.com/beamiter/relm_bar) | Relm4-based status bar |
| [dioxus_bar](https://github.com/beamiter/dioxus_bar) | Dioxus-based status bar |

### Status Bar Implementations (winit + Rendering Backends)
| Repository | Description |
|------------|-------------|
| [winit_pixels_bar](https://github.com/beamiter/winit_pixels_bar) | winit + pixels |
| [winit_softbuffer_bar](https://github.com/beamiter/winit_softbuffer_bar) | winit + softbuffer |
| [winit_wgpu_bar](https://github.com/beamiter/winit_wgpu_bar) | winit + wgpu |

### Status Bar Implementations (tao + Rendering Backends)
| Repository | Description |
|------------|-------------|
| [tao_pixels_bar](https://github.com/beamiter/tao_pixels_bar) | tao + pixels |
| [tao_softbuffer_bar](https://github.com/beamiter/tao_softbuffer_bar) | tao + softbuffer |
| [tao_wgpu_bar](https://github.com/beamiter/tao_wgpu_bar) | tao + wgpu |

### Status Bar Implementations (X11 Direct)
| Repository | Description |
|------------|-------------|
| [x11rb_bar](https://github.com/beamiter/x11rb_bar) | x11rb-based status bar |
| [xcb_bar](https://github.com/beamiter/xcb_bar) | xcb-based status bar |

### Tauri Projects
| Repository | Description |
|------------|-------------|
| [tauri_react_bar](https://github.com/beamiter/tauri_react_bar) | Tauri + React status bar |
| [tauri_vue_bar](https://github.com/beamiter/tauri_vue_bar) | Tauri + Vue status bar |

## Dependency Graph

```
shared_structures (base library)
    └── xbar_core
            └── jwm
            └── egui_bar
            └── gtk_bar
            └── iced_bar
            └── relm_bar
            └── dioxus_bar
            └── x11rb_bar
            └── xcb_bar
            └── winit_*_bar
            └── tao_*_bar
            └── tauri_*_bar

jterm4 (independent)
clash_client (independent)
eframe_toy (independent)
```

---
*Archived on 2026-03-01*

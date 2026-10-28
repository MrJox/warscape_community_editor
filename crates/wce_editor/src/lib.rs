//! The editor shell: dockspace, menus, toolbar, tool switching with a layout per tool, the viewport with gizmos, the shared panels, built-in
//! commands and scripts, first run.

pub mod camera_world;
pub mod commands;
pub mod coords;
pub mod editor;
pub mod gui;
pub mod icons;
pub mod images;
pub mod panels;
pub mod perf;
pub mod settings_window;
pub mod steam_detect;
pub mod theme;
pub mod tool;
pub mod viewport;

pub use editor::Editor;
pub use tool::Tool;

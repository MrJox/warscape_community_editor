//! The editor shell without UI or GPU code: what `editor_check` tests. The dockspace, panels, viewport and gizmos are built on top of it.

pub mod camera_controller;
pub mod command;
pub mod command_registry;
pub mod document;
pub mod document_manager;
pub mod inspector;
pub mod selection;

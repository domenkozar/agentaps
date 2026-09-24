//! Agentaps' dark palette. Keep the interaction accent and surfaces consistent
//! across the sidebar, chat, picker, and activity views.

use gpui::App;
use gpui_component::{
    scroll::ScrollbarShow,
    theme::{Theme, ThemeMode},
};

pub fn apply(cx: &mut App) {
    Theme::change(ThemeMode::Dark, None, cx);
    Theme::global_mut(cx).scrollbar_show = ScrollbarShow::Always;
}

pub const BG: u32 = 0x10151c;
pub const SIDEBAR: u32 = 0x151c25;
pub const SURFACE: u32 = 0x202a36;
pub const BORDER: u32 = 0x334253;
pub const TEXT: u32 = 0xedf2f7;
pub const MUTED: u32 = 0x9aaaba;
pub const ACCENT: u32 = 0x8fc5ec;

pub const HOVER: u32 = 0x293747;
pub const SELECTED: u32 = 0x30485f;
pub const DROP_TARGET: u32 = 0x395871;
pub const USER_BUBBLE: u32 = 0x29465c;
pub const AGENT_BUBBLE: u32 = 0x202e3b;
pub const CHIP: u32 = 0x364e63;
pub const ACCENT_SURFACE: u32 = 0x304b60;

pub const STATUS_CONNECTING: u32 = 0xa2b0be;
pub const STATUS_IDLE: u32 = 0x8aa9c0;
pub const STATUS_WORKING: u32 = 0xe9b978;
pub const STATUS_DONE: u32 = 0x84c8a7;
pub const STATUS_ERROR: u32 = 0xe99191;

pub const AGENT_PICKER_CHIP: u32 = ACCENT_SURFACE;
pub const AGENT_PICKER_TEXT: u32 = ACCENT;
pub const TOOL_MARKER: u32 = STATUS_WORKING;
pub const PERMISSION_BORDER: u32 = 0xc99463;
pub const ERROR_SURFACE: u32 = 0x51343a;
pub const ERROR_TEXT: u32 = 0xffc9c9;

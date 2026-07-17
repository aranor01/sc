use anyhow::{bail, Context, Result};
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use serde::Deserialize;

// ── Color ────────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Color(pub u8, pub u8, pub u8);

impl Color {
    pub fn from_hex(s: &str) -> Result<Self> {
        let s = s
            .strip_prefix('#')
            .with_context(|| format!("color {:?} must start with '#'", s))?;
        if s.len() != 6 {
            bail!("color hex must be exactly 6 digits, got {:?}", s);
        }
        let r = u8::from_str_radix(&s[0..2], 16).context("invalid red component")?;
        let g = u8::from_str_radix(&s[2..4], 16).context("invalid green component")?;
        let b = u8::from_str_radix(&s[4..6], 16).context("invalid blue component")?;
        Ok(Color(r, g, b))
    }
}

const fn rgb(hex: u32) -> Color {
    Color(
        ((hex >> 16) & 0xFF) as u8,
        ((hex >> 8) & 0xFF) as u8,
        (hex & 0xFF) as u8,
    )
}

// ── KeyBinding ───────────────────────────────────────────────────────────────

/// One binding: a single key press or a two-key chord (e.g. Ctrl+x then t).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum KeyBinding {
    Single(KeyEvent),
    Chord(KeyEvent, KeyEvent),
}

/// All alternative bindings for one action (any one triggers the action).
pub type ActionBindings = Vec<KeyBinding>;

pub fn bindings_match_event(bindings: &ActionBindings, event: &KeyEvent) -> bool {
    bindings.iter().any(|b| matches!(b, KeyBinding::Single(ke) if ke == event))
}

fn ke(code: KeyCode, mods: KeyModifiers) -> KeyEvent {
    KeyEvent::new(code, mods)
}

fn parse_key_event(s: &str) -> Result<KeyEvent> {
    let mut remaining = s;
    let mut mods = KeyModifiers::NONE;

    loop {
        if remaining.starts_with("Ctrl-") {
            mods |= KeyModifiers::CONTROL;
            remaining = &remaining[5..];
        } else if remaining.starts_with("C-") {
            mods |= KeyModifiers::CONTROL;
            remaining = &remaining[2..];
        } else if remaining.starts_with("Alt-") {
            mods |= KeyModifiers::ALT;
            remaining = &remaining[4..];
        } else if remaining.starts_with("A-") {
            mods |= KeyModifiers::ALT;
            remaining = &remaining[2..];
        } else if remaining.starts_with("Shift-") {
            mods |= KeyModifiers::SHIFT;
            remaining = &remaining[6..];
        } else if remaining.starts_with("S-") {
            mods |= KeyModifiers::SHIFT;
            remaining = &remaining[2..];
        } else {
            break;
        }
    }

    let code = match remaining {
        "Enter" => KeyCode::Enter,
        "Tab" => KeyCode::Tab,
        "Backspace" => KeyCode::Backspace,
        "Delete" | "Del" => KeyCode::Delete,
        "Insert" => KeyCode::Insert,
        "Up" => KeyCode::Up,
        "Down" => KeyCode::Down,
        "Left" => KeyCode::Left,
        "Right" => KeyCode::Right,
        "Home" => KeyCode::Home,
        "End" => KeyCode::End,
        "PageUp" => KeyCode::PageUp,
        "PageDown" => KeyCode::PageDown,
        "Esc" => KeyCode::Esc,
        s if s.len() > 1 && s.starts_with('F') => {
            let n: u8 = s[1..].parse().with_context(|| format!("invalid function key {:?}", s))?;
            KeyCode::F(n)
        }
        s if s.chars().count() == 1 => KeyCode::Char(s.chars().next().unwrap()),
        s => bail!("unknown key code {:?}", s),
    };

    // crossterm maps bytes 0x1C-0x1F (Ctrl+\ ] ^ _) to Char('4'-'7') with CONTROL,
    // not to the original character. Normalize here so config files work correctly.
    let code = if mods.contains(KeyModifiers::CONTROL) {
        match code {
            KeyCode::Char('\\') => KeyCode::Char('4'),
            KeyCode::Char(']')  => KeyCode::Char('5'),
            KeyCode::Char('^')  => KeyCode::Char('6'),
            KeyCode::Char('_')  => KeyCode::Char('7'),
            other => other,
        }
    } else {
        code
    };

    Ok(KeyEvent::new(code, mods))
}

pub fn parse_key_binding(s: &str) -> Result<KeyBinding> {
    // A chord is two single-key specs separated by exactly one space.
    if let Some(pos) = s.find(' ') {
        let first = &s[..pos];
        let second = &s[pos + 1..];
        if !second.contains(' ') {
            return Ok(KeyBinding::Chord(
                parse_key_event(first)
                    .with_context(|| format!("parsing first key in chord {:?}", s))?,
                parse_key_event(second)
                    .with_context(|| format!("parsing second key in chord {:?}", s))?,
            ));
        }
    }
    Ok(KeyBinding::Single(
        parse_key_event(s).with_context(|| format!("parsing key {:?}", s))?,
    ))
}

/// Format a KeyEvent into a human-readable label like "F5", "Ctrl-Alt-F5", "C-s".
pub fn format_key(event: &KeyEvent) -> String {
    format_key_with(event, "C-", "A-", "S-")
}

/// Like `format_key`, but with modifiers spelled out ("Ctrl-", "Alt-", "Shift-")
/// for use in status-bar hint text.
pub fn format_key_spelled(event: &KeyEvent) -> String {
    format_key_with(event, "Ctrl-", "Alt-", "Shift-")
}

fn format_key_with(event: &KeyEvent, ctrl: &str, alt: &str, shift: &str) -> String {
    let mut s = String::new();
    if event.modifiers.contains(KeyModifiers::CONTROL) { s.push_str(ctrl); }
    if event.modifiers.contains(KeyModifiers::ALT)     { s.push_str(alt); }
    if event.modifiers.contains(KeyModifiers::SHIFT)   { s.push_str(shift); }
    let code = match event.code {
        KeyCode::F(n) => format!("F{n}"),
        KeyCode::Char(c) => c.to_string(),
        KeyCode::Enter => "Enter".to_string(),
        KeyCode::Tab => "Tab".to_string(),
        KeyCode::Esc => "Esc".to_string(),
        KeyCode::Backspace => "BS".to_string(),
        KeyCode::Delete => "Del".to_string(),
        KeyCode::Insert => "Ins".to_string(),
        KeyCode::Home => "Home".to_string(),
        KeyCode::End => "End".to_string(),
        KeyCode::PageUp => "PgUp".to_string(),
        KeyCode::PageDown => "PgDn".to_string(),
        KeyCode::Up => "Up".to_string(),
        KeyCode::Down => "Dn".to_string(),
        KeyCode::Left => "Left".to_string(),
        KeyCode::Right => "Rght".to_string(),
        _ => "?".to_string(),
    };
    s.push_str(&code);
    s
}

fn parse_action_bindings(v: &serde_json::Value) -> Result<ActionBindings> {
    match v {
        serde_json::Value::String(s) => Ok(vec![parse_key_binding(s)?]),
        serde_json::Value::Array(arr) => arr
            .iter()
            .map(|item| {
                let s = item
                    .as_str()
                    .context("keybinding array must contain strings")?;
                parse_key_binding(s)
            })
            .collect(),
        _ => bail!("keybinding must be a string or array of strings"),
    }
}

// ── KeyBindings ───────────────────────────────────────────────────────────────

#[derive(Debug, Clone)]
pub struct KeyBindings {
    pub switch_panel: ActionBindings,
    pub toggle_layout: ActionBindings,
    pub tag_file: ActionBindings,
    pub invert_tags: ActionBindings,
    pub copy: ActionBindings,
    pub move_entry: ActionBindings,
    pub delete: ActionBindings,
    pub user_menu: ActionBindings,
    pub exit: ActionBindings,
    pub cmdline_insert_filename: ActionBindings,
    pub cmdline_insert_fullpath: ActionBindings,
    pub cmdline_complete: ActionBindings,
    pub cmdline_insert_tagged: ActionBindings,
    pub cmdline_insert_tagged_other: ActionBindings,
    pub cmdline_insert_path: ActionBindings,
    pub cmdline_insert_path_other: ActionBindings,
    pub toggle_shell: ActionBindings,
    pub toggle_shell_and_sync_command_line: ActionBindings,
    pub toggle_cmdline: ActionBindings,
    pub toggle_button_bar: ActionBindings,
    pub cmdline_history_prev: ActionBindings,
    pub cmdline_history_next: ActionBindings,
    pub reverse_search: ActionBindings,
    pub sync_panels: ActionBindings,
    pub rename: ActionBindings,
    pub sort_panel: ActionBindings,
    pub quicksearch: ActionBindings,
    pub toggle_hidden: ActionBindings,
    pub bookmark_open: ActionBindings,
    pub bookmark_add: ActionBindings,
    pub mkdir: ActionBindings,
    pub path_history: ActionBindings,
    pub filter: ActionBindings,
    pub search: ActionBindings,
    pub select_group: ActionBindings,
    pub unselect_group: ActionBindings,
    pub refresh_panel: ActionBindings,
    pub go_to_parent: ActionBindings,
    pub go_back: ActionBindings,
    pub go_forward: ActionBindings,
    pub toggle_matches_panel: ActionBindings,
    pub view: ActionBindings,
}

impl Default for KeyBindings {
    fn default() -> Self {
        use KeyCode::*;
        use KeyModifiers as M;
        let n = M::NONE;
        let c = M::CONTROL;
        let a = M::ALT;
        let ca = M::CONTROL | M::ALT;
        let cs = M::CONTROL | M::SHIFT;
        KeyBindings {
            switch_panel: vec![KeyBinding::Single(ke(Tab, n))],
            toggle_layout: vec![KeyBinding::Single(ke(Char(','), a))],
            tag_file: vec![KeyBinding::Single(ke(Insert, n))],
            invert_tags: vec![KeyBinding::Single(ke(Char('*'), n))],
            copy: vec![KeyBinding::Single(ke(F(5), n))],
            move_entry: vec![KeyBinding::Single(ke(F(6), n))],
            delete: vec![KeyBinding::Single(ke(F(8), n))],
            user_menu: vec![KeyBinding::Single(ke(F(2), n))],
            exit: vec![KeyBinding::Single(ke(F(10), n)), KeyBinding::Single(ke(Char('q'), c))],
            cmdline_insert_filename: vec![
                KeyBinding::Single(ke(Enter, a)),
                KeyBinding::Single(ke(Enter, c)),
            ],
            cmdline_insert_fullpath: vec![KeyBinding::Single(ke(Enter, cs))],
            cmdline_complete: vec![KeyBinding::Single(ke(Tab, a)), KeyBinding::Single(ke(Char(' '), c))],
            cmdline_insert_tagged: vec![KeyBinding::Chord(
                ke(Char('x'), c),
                ke(Char('t'), n),
            )],
            cmdline_insert_tagged_other: vec![KeyBinding::Chord(
                ke(Char('x'), c),
                ke(Char('t'), c),
            )],
            cmdline_insert_path: vec![KeyBinding::Chord(
                ke(Char('x'), c),
                ke(Char('p'), n),
            )],
            cmdline_insert_path_other: vec![KeyBinding::Chord(
                ke(Char('x'), c),
                ke(Char('p'), c),
            )],
            toggle_shell: vec![KeyBinding::Single(ke(Char('o'), c))],
            toggle_shell_and_sync_command_line: vec![KeyBinding::Single(ke(Char('o'), a))],
            toggle_cmdline: vec![KeyBinding::Single(ke(Char('b'), ca))],
            toggle_button_bar: vec![KeyBinding::Single(ke(Char('b'), a))],
            cmdline_history_prev: vec![KeyBinding::Single(ke(Up, c))],
            cmdline_history_next: vec![KeyBinding::Single(ke(Down, c))],
            reverse_search: vec![KeyBinding::Single(ke(Char('r'), c)), KeyBinding::Single(ke(Char('h'), a))],
            sync_panels: vec![KeyBinding::Single(ke(Char('i'), a))],
            rename: vec![KeyBinding::Single(ke(F(6), M::SHIFT))],
            sort_panel: vec![KeyBinding::Single(ke(Char('s'), c))],
            quicksearch: vec![KeyBinding::Single(ke(Char('/'), n)), KeyBinding::Single(ke(Char('s'), a))],
            toggle_hidden: vec![KeyBinding::Single(ke(Char('.'), a))],
            // crossterm maps Ctrl+\ (byte 0x1C) to Char('4') with CONTROL
            bookmark_open: vec![KeyBinding::Single(ke(Char('4'), c))],
            bookmark_add: vec![KeyBinding::Single(ke(Char('b'), c))],
            mkdir: vec![KeyBinding::Single(ke(F(7), n))],
            path_history: vec![KeyBinding::Single(ke(Char('H'), a)), KeyBinding::Single(ke(Down, a))],
            filter: vec![KeyBinding::Single(ke(Char('f'), a))],
            search: vec![KeyBinding::Single(ke(Char('?'), a)), KeyBinding::Single(ke(Char('f'), c))],
            select_group: vec![KeyBinding::Single(ke(Char('+'), n))],
            unselect_group: vec![KeyBinding::Single(ke(Char('-'), n))],
            refresh_panel: vec![KeyBinding::Single(ke(Char('r'), a))],
            go_to_parent: vec![KeyBinding::Single(ke(Up, a))],
            go_back: vec![KeyBinding::Single(ke(Left, a))],
            go_forward: vec![KeyBinding::Single(ke(Right, a))],
            toggle_matches_panel: vec![KeyBinding::Single(ke(Char('m'), a))],
            view: vec![KeyBinding::Single(ke(F(3), n))],
        }
    }
}

// ── MenuItem ─────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Deserialize)]
pub struct MenuItem {
    pub label: String,
    pub command: String,
    #[serde(default)]
    pub keys: Option<String>,
    #[serde(default)]
    pub add_to_bar: bool,
}

// ── ColorScheme ───────────────────────────────────────────────────────────────

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ColorScheme {
    pub panel_bg: Color,
    pub panel_fg: Color,
    pub active_border_fg: Color,
    pub inactive_border_fg: Color,
    pub selected_fg: Color,
    pub selected_bg: Color,
    pub tagged_fg: Color,
    pub tagged_bg: Color,
    pub cmdline_bg: Color,
    pub cmdline_fg: Color,
    pub cmdline_inactive_bg: Color,
    pub cmdline_inactive_fg: Color,
    pub dialog_bg: Color,
    pub dialog_fg: Color,
    pub dialog_border_fg: Color,
    pub dialog_butt_fg: Color,
    pub dialog_butt_bg: Color,
    pub dialog_error_fg: Color,
    pub dialog_mark_fg: Color,
    pub panel_error_fg: Color,
    pub button_bar_bg: Color,
    pub button_bar_fg: Color,
    pub button_bar_butt_bg: Color,
    pub button_bar_butt_fg: Color,
    pub status_info_fg: Color,
    pub status_info_bg: Color,
    pub status_warn_fg: Color,
    pub status_warn_bg: Color,
    pub search_match_fg: Color,
    pub search_match_bg: Color,
}

impl Default for ColorScheme {
    fn default() -> Self {
        ColorScheme {
            panel_bg: rgb(0x1a1a2e),
            panel_fg: rgb(0xeaeaea),
            active_border_fg: rgb(0x00aaff),
            inactive_border_fg: rgb(0x555555),
            selected_fg: rgb(0xffff00),
            selected_bg: rgb(0x1a3a5c),
            tagged_fg: rgb(0xff8c00),
            tagged_bg: rgb(0x2d1500),
            cmdline_bg: rgb(0x000000),
            cmdline_fg: rgb(0xffffff),
            cmdline_inactive_bg: rgb(0x000000),
            cmdline_inactive_fg: rgb(0x888888),
            dialog_bg: rgb(0x003366),
            dialog_fg: rgb(0xffffff),
            dialog_border_fg: rgb(0x00aaff),
            dialog_butt_fg: rgb(0x000000),
            dialog_butt_bg: rgb(0x00aaff),
            dialog_error_fg: rgb(0xff4444),
            dialog_mark_fg: rgb(0x00ff88),
            panel_error_fg: rgb(0xff4444),
            button_bar_bg: rgb(0x000000),
            button_bar_fg: rgb(0xffffff),
            button_bar_butt_bg: rgb(0x00aaff),
            button_bar_butt_fg: rgb(0x000000),
            status_info_fg: rgb(0x000000),
            status_info_bg: rgb(0x00aa55),
            status_warn_fg: rgb(0x000000),
            status_warn_bg: rgb(0xddaa00),
            search_match_fg: rgb(0x000000),
            search_match_bg: rgb(0xffcc00),
        }
    }
}

// ── PanelsConfig ─────────────────────────────────────────────────────────────

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PanelsConfig {
    pub time_format: String,
    pub time_length: usize,
    /// Action for Enter/double-click on an executable file. Empty falls back to
    /// `default_action`.
    pub default_action_executable: String,
    /// Action for Enter/double-click on a non-executable file. Empty falls back to
    /// `default_action`.
    pub default_action_text: String,
    /// Fallback action used when the specific field above is empty. Empty is a no-op.
    pub default_action: String,
}

impl Default for PanelsConfig {
    fn default() -> Self {
        PanelsConfig {
            time_format: "%y-%m-%d %H:%M".to_string(),
            time_length: 14,
            default_action_executable: String::new(),
            default_action_text: ":view".to_string(),
            default_action: String::new(),
        }
    }
}

// ── StartupConfig ─────────────────────────────────────────────────────────────

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StartupConfig {
    pub restore_paths: bool,
    pub subshell: bool,
    pub ipc_scripting: bool,
}

impl Default for StartupConfig {
    fn default() -> Self {
        StartupConfig { restore_paths: false, subshell: true, ipc_scripting: false }
    }
}

// ── Config ────────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Default)]
pub struct Config {
    pub keybindings: KeyBindings,
    pub menu: Vec<MenuItem>,
    pub colorscheme: ColorScheme,
    pub startup: StartupConfig,
    pub panels: PanelsConfig,
}

/// Locate the scripts directory at runtime using a two-step search:
/// 1. Compile-time install prefix: `<SC_INSTALL_PREFIX>/share/sc/scripts/`
/// 2. Fallback: `scripts/` directory alongside the running binary
pub fn find_scripts_dir() -> Option<std::path::PathBuf> {
    let prefix = std::path::PathBuf::from(env!("SC_INSTALL_PREFIX"))
        .join("share").join("sc").join("scripts");
    if prefix.join("edit.sh").exists() {
        return Some(prefix);
    }
    if let Ok(exe) = std::env::current_exe() {
        if let Some(parent) = exe.parent() {
            let scripts = parent.join("scripts");
            if scripts.join("edit.sh").exists() {
                return Some(scripts);
            }
        }
    }
    None
}

fn generate_default_config(scripts_dir: &std::path::Path) -> String {
    let s = scripts_dir.to_string_lossy();
    format!(
        r#"{{
  "menu": [
    {{ "label": "View",        "command": "{s}/view.sh %f",                             "keys": "Shift-F3" }},
    {{ "label": "Edit",        "command": "{s}/edit.sh %f",                             "keys": "F4" }},
    {{ "label": "Edit config", "command": "{s}/edit.sh ~/.config/sc/config.json" }}
  ]
}}
"#
    )
}

impl Config {
    /// Load config from the default path (~/.config/sc/config.json).
    /// If the file is absent and scripts are found, generates a default config.
    pub fn load() -> Result<Self> {
        let path = dirs::config_dir()
            .context("cannot determine config directory")?
            .join("sc")
            .join("config.json");

        if !path.exists() {
            if let Some(scripts_dir) = find_scripts_dir() {
                let content = generate_default_config(&scripts_dir);
                if let Some(parent) = path.parent() {
                    let _ = std::fs::create_dir_all(parent);
                }
                let _ = std::fs::write(&path, &content);
                return Self::load_from_str(&content);
            }
            return Ok(Self::default());
        }

        let content = std::fs::read_to_string(&path)
            .with_context(|| format!("reading {}", path.display()))?;
        Self::load_from_str(&content)
    }

    /// Parse config from a JSON string, merging missing fields with defaults.
    pub fn load_from_str(json: &str) -> Result<Self> {
        let v: serde_json::Value =
            serde_json::from_str(json).context("config is not valid JSON")?;
        let mut cfg = Config::default();

        // keybindings
        if let Some(kb) = v.get("keybindings").and_then(|v| v.as_object()) {
            for (action, value) in kb {
                let bindings = parse_action_bindings(value)
                    .with_context(|| format!("keybindings.{}", action))?;
                match action.as_str() {
                    "switch_panel" => cfg.keybindings.switch_panel = bindings,
                    "toggle_layout" => cfg.keybindings.toggle_layout = bindings,
                    "tag_file" => cfg.keybindings.tag_file = bindings,
                    "invert_tags" => cfg.keybindings.invert_tags = bindings,
                    "copy" => cfg.keybindings.copy = bindings,
                    "move" => cfg.keybindings.move_entry = bindings,
                    "delete" => cfg.keybindings.delete = bindings,
                    "user_menu" => cfg.keybindings.user_menu = bindings,
                    "exit" => cfg.keybindings.exit = bindings,
                    "cmdline_insert_filename" => cfg.keybindings.cmdline_insert_filename = bindings,
                    "cmdline_insert_fullpath" => cfg.keybindings.cmdline_insert_fullpath = bindings,
                    "cmdline_complete" => cfg.keybindings.cmdline_complete = bindings,
                    "cmdline_insert_tagged" => cfg.keybindings.cmdline_insert_tagged = bindings,
                    "cmdline_insert_tagged_other" => {
                        cfg.keybindings.cmdline_insert_tagged_other = bindings
                    }
                    "cmdline_insert_path" => cfg.keybindings.cmdline_insert_path = bindings,
                    "cmdline_insert_path_other" => {
                        cfg.keybindings.cmdline_insert_path_other = bindings
                    }
                    "toggle_shell" => cfg.keybindings.toggle_shell = bindings,
                    "toggle_shell_and_sync_command_line" => {
                        cfg.keybindings.toggle_shell_and_sync_command_line = bindings
                    }
                    "toggle_cmdline" => cfg.keybindings.toggle_cmdline = bindings,
                    "toggle_button_bar" => cfg.keybindings.toggle_button_bar = bindings,
                    "cmdline_history_prev" => cfg.keybindings.cmdline_history_prev = bindings,
                    "cmdline_history_next" => cfg.keybindings.cmdline_history_next = bindings,
                    "reverse_search" => cfg.keybindings.reverse_search = bindings,
                    "sync_panels" => cfg.keybindings.sync_panels = bindings,
                    "rename" => cfg.keybindings.rename = bindings,
                    "sort_panel" => cfg.keybindings.sort_panel = bindings,
                    "quicksearch" => cfg.keybindings.quicksearch = bindings,
                    "toggle_hidden" => cfg.keybindings.toggle_hidden = bindings,
                    "bookmark_open" => cfg.keybindings.bookmark_open = bindings,
                    "bookmark_add" => cfg.keybindings.bookmark_add = bindings,
                    "mkdir" => cfg.keybindings.mkdir = bindings,
                    "path_history" => cfg.keybindings.path_history = bindings,
                    "filter" => cfg.keybindings.filter = bindings,
                    "search" => cfg.keybindings.search = bindings,
                    "select_group" => cfg.keybindings.select_group = bindings,
                    "unselect_group" => cfg.keybindings.unselect_group = bindings,
                    "refresh_panel" => cfg.keybindings.refresh_panel = bindings,
                    "go_to_parent" => cfg.keybindings.go_to_parent = bindings,
                    "go_back" => cfg.keybindings.go_back = bindings,
                    "go_forward" => cfg.keybindings.go_forward = bindings,
                    "toggle_matches_panel" => cfg.keybindings.toggle_matches_panel = bindings,
                    "view" => cfg.keybindings.view = bindings,
                    _ => {} // unknown keys silently ignored
                }
            }
        }

        // menu
        if let Some(menu) = v.get("menu") {
            cfg.menu = serde_json::from_value(menu.clone()).context("parsing menu")?;
        }

        // colorscheme
        if let Some(cs) = v.get("colorscheme") {
            let pick = |key: &str, default: Color| -> Result<Color> {
                match cs.get(key).and_then(|v| v.as_str()) {
                    Some(s) => Color::from_hex(s)
                        .with_context(|| format!("colorscheme.{}", key)),
                    None => Ok(default),
                }
            };
            let d = ColorScheme::default();
            cfg.colorscheme = ColorScheme {
                panel_bg: pick("panel_bg", d.panel_bg)?,
                panel_fg: pick("panel_fg", d.panel_fg)?,
                active_border_fg: pick("active_border_fg", d.active_border_fg)?,
                inactive_border_fg: pick("inactive_border_fg", d.inactive_border_fg)?,
                selected_fg: pick("selected_fg", d.selected_fg)?,
                selected_bg: pick("selected_bg", d.selected_bg)?,
                tagged_fg: pick("tagged_fg", d.tagged_fg)?,
                tagged_bg: pick("tagged_bg", d.tagged_bg)?,
                cmdline_bg: pick("cmdline_bg", d.cmdline_bg)?,
                cmdline_fg: pick("cmdline_fg", d.cmdline_fg)?,
                cmdline_inactive_bg: pick("cmdline_inactive_bg", d.cmdline_inactive_bg)?,
                cmdline_inactive_fg: pick("cmdline_inactive_fg", d.cmdline_inactive_fg)?,
                dialog_bg: pick("dialog_bg", d.dialog_bg)?,
                dialog_fg: pick("dialog_fg", d.dialog_fg)?,
                dialog_border_fg: pick("dialog_border_fg", d.dialog_border_fg)?,
                dialog_butt_fg: pick("dialog_butt_fg", d.dialog_butt_fg)?,
                dialog_butt_bg: pick("dialog_butt_bg", d.dialog_butt_bg)?,
                dialog_error_fg: pick("dialog_error_fg", d.dialog_error_fg)?,
                dialog_mark_fg: pick("dialog_mark_fg", d.dialog_mark_fg)?,
                panel_error_fg: pick("panel_error_fg", d.panel_error_fg)?,
                button_bar_bg: pick("button_bar_bg", d.button_bar_bg)?,
                button_bar_fg: pick("button_bar_fg", d.button_bar_fg)?,
                button_bar_butt_bg: pick("button_bar_butt_bg", d.button_bar_butt_bg)?,
                button_bar_butt_fg: pick("button_bar_butt_fg", d.button_bar_butt_fg)?,
                status_info_fg: pick("status_info_fg", d.status_info_fg)?,
                status_info_bg: pick("status_info_bg", d.status_info_bg)?,
                status_warn_fg: pick("status_warn_fg", d.status_warn_fg)?,
                status_warn_bg: pick("status_warn_bg", d.status_warn_bg)?,
                search_match_fg: pick("search_match_fg", d.search_match_fg)?,
                search_match_bg: pick("search_match_bg", d.search_match_bg)?,
            };
        }

        // panels
        if let Some(panels) = v.get("panels") {
            if let Some(v) = panels.get("time_format").and_then(|v| v.as_str()) {
                cfg.panels.time_format = v.to_string();
            }
            if let Some(v) = panels.get("time_lenght").and_then(|v| v.as_u64()) {
                cfg.panels.time_length = v as usize;
            }
            if let Some(v) = panels.get("default_action_executable").and_then(|v| v.as_str()) {
                cfg.panels.default_action_executable = v.to_string();
            }
            if let Some(v) = panels.get("default_action_text").and_then(|v| v.as_str()) {
                cfg.panels.default_action_text = v.to_string();
            }
            if let Some(v) = panels.get("default_action").and_then(|v| v.as_str()) {
                cfg.panels.default_action = v.to_string();
            }
        }

        // startup
        if let Some(startup) = v.get("startup") {
            if let Some(v) = startup.get("restore_paths").and_then(|v| v.as_bool()) {
                cfg.startup.restore_paths = v;
            }
            if let Some(v) = startup.get("subshell").and_then(|v| v.as_bool()) {
                cfg.startup.subshell = v;
            }
            if let Some(v) = startup.get("ipc_scripting").and_then(|v| v.as_bool()) {
                cfg.startup.ipc_scripting = v;
            }
        }

        Ok(cfg)
    }
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crossterm::event::{KeyCode::*, KeyModifiers as M};

    fn single(code: KeyCode, mods: M) -> KeyBinding {
        KeyBinding::Single(KeyEvent::new(code, mods))
    }
    fn chord(c1: KeyCode, m1: M, c2: KeyCode, m2: M) -> KeyBinding {
        KeyBinding::Chord(KeyEvent::new(c1, m1), KeyEvent::new(c2, m2))
    }

    #[test]
    fn empty_json_gives_all_defaults() {
        let cfg = Config::load_from_str("{}").unwrap();
        assert!(!cfg.startup.restore_paths);
        assert!(cfg.menu.is_empty());
        assert_eq!(cfg.colorscheme, ColorScheme::default());
        // spot-check a keybinding
        assert!(cfg.keybindings.exit.contains(&single(F(10), M::NONE)));
        assert!(cfg.keybindings.toggle_matches_panel.contains(&single(Char('m'), M::ALT)));
    }

    #[test]
    fn toggle_matches_panel_override_parses() {
        let cfg = Config::load_from_str(r#"{"keybindings":{"toggle_matches_panel":"Alt-n"}}"#).unwrap();
        assert!(cfg.keybindings.toggle_matches_panel.contains(&single(Char('n'), M::ALT)));
    }

    #[test]
    fn partial_json_merges_defaults() {
        let cfg = Config::load_from_str(r#"{"startup":{"restore_paths":true}}"#).unwrap();
        assert!(cfg.startup.restore_paths);
        // other fields still have defaults
        assert_eq!(cfg.colorscheme.panel_bg, rgb(0x1a1a2e));
        assert!(cfg.keybindings.exit.contains(&single(F(10), M::NONE)));
    }

    #[test]
    fn panels_defaults_and_override() {
        let cfg = Config::load_from_str("{}").unwrap();
        assert_eq!(cfg.panels, PanelsConfig::default());
        assert_eq!(cfg.panels.time_format, "%y-%m-%d %H:%M");
        assert_eq!(cfg.panels.time_length, 14);

        let cfg = Config::load_from_str(
            r#"{"panels":{"time_format":"%Y-%m-%d","time_lenght":10}}"#,
        )
        .unwrap();
        assert_eq!(cfg.panels.time_format, "%Y-%m-%d");
        assert_eq!(cfg.panels.time_length, 10);
    }

    #[test]
    fn ipc_scripting_defaults_to_false_and_can_be_enabled() {
        assert!(!Config::load_from_str("{}").unwrap().startup.ipc_scripting);
        let cfg = Config::load_from_str(r#"{"startup":{"ipc_scripting":true}}"#).unwrap();
        assert!(cfg.startup.ipc_scripting);
    }

    #[test]
    fn parse_function_key() {
        assert_eq!(
            parse_key_binding("F5").unwrap(),
            single(F(5), M::NONE)
        );
        assert_eq!(
            parse_key_binding("F10").unwrap(),
            single(F(10), M::NONE)
        );
    }

    #[test]
    fn parse_alt_comma() {
        assert_eq!(
            parse_key_binding("Alt-,").unwrap(),
            single(Char(','), M::ALT)
        );
        // "A-," is the shorthand form
        assert_eq!(
            parse_key_binding("A-,").unwrap(),
            single(Char(','), M::ALT)
        );
    }

    #[test]
    fn parse_ctrl_shift_enter() {
        assert_eq!(
            parse_key_binding("C-S-Enter").unwrap(),
            single(Enter, M::CONTROL | M::SHIFT)
        );
    }

    #[test]
    fn parse_chord() {
        assert_eq!(
            parse_key_binding("C-x t").unwrap(),
            chord(Char('x'), M::CONTROL, Char('t'), M::NONE)
        );
        assert_eq!(
            parse_key_binding("C-x C-t").unwrap(),
            chord(Char('x'), M::CONTROL, Char('t'), M::CONTROL)
        );
    }

    #[test]
    fn parse_array_binding() {
        let v = serde_json::json!(["F8", "Delete"]);
        let bindings = super::parse_action_bindings(&v).unwrap();
        assert_eq!(bindings.len(), 2);
        assert_eq!(bindings[0], single(F(8), M::NONE));
        assert_eq!(bindings[1], single(Delete, M::NONE));
    }

    #[test]
    fn parse_ctrl_alt_b() {
        assert_eq!(
            parse_key_binding("C-A-b").unwrap(),
            single(Char('b'), M::CONTROL | M::ALT)
        );
    }

    #[test]
    fn parse_ctrl_up() {
        assert_eq!(
            parse_key_binding("C-Up").unwrap(),
            single(Up, M::CONTROL)
        );
    }

    #[test]
    fn unknown_keybinding_in_config_is_ignored() {
        let cfg = Config::load_from_str(r#"{"keybindings":{"nonexistent":"F1"}}"#).unwrap();
        // no panic, just uses defaults
        assert!(cfg.keybindings.exit.contains(&single(F(10), M::NONE)));
    }

    #[test]
    fn config_menu_parsed() {
        let json = r#"{"menu":[{"label":"View","command":"less %f","keys":"F3"}]}"#;
        let cfg = Config::load_from_str(json).unwrap();
        assert_eq!(cfg.menu.len(), 1);
        assert_eq!(cfg.menu[0].label, "View");
        assert_eq!(cfg.menu[0].command, "less %f");
        assert_eq!(cfg.menu[0].keys.as_deref(), Some("F3"));
    }

    #[test]
    fn search_and_filter_default_bindings() {
        let cfg = Config::load_from_str("{}").unwrap();
        assert_eq!(cfg.keybindings.filter, vec![single(Char('f'), M::ALT)]);
        assert_eq!(
            cfg.keybindings.search,
            vec![single(Char('?'), M::ALT), single(Char('f'), M::CONTROL)]
        );
    }

    #[test]
    fn search_binding_and_match_colors_configurable() {
        let cfg = Config::load_from_str(
            r##"{"keybindings":{"search":"F19"},
                "colorscheme":{"search_match_fg":"#111111","search_match_bg":"#222222"}}"##,
        )
        .unwrap();
        assert_eq!(cfg.keybindings.search, vec![single(F(19), M::NONE)]);
        assert_eq!(cfg.colorscheme.search_match_fg, rgb(0x111111));
        assert_eq!(cfg.colorscheme.search_match_bg, rgb(0x222222));
    }

    #[test]
    fn color_from_hex_valid() {
        assert_eq!(Color::from_hex("#1a2b3c").unwrap(), Color(0x1a, 0x2b, 0x3c));
        assert_eq!(Color::from_hex("#000000").unwrap(), Color(0, 0, 0));
        assert_eq!(Color::from_hex("#ffffff").unwrap(), Color(255, 255, 255));
    }

    #[test]
    fn color_from_hex_invalid() {
        assert!(Color::from_hex("1a2b3c").is_err()); // missing #
        assert!(Color::from_hex("#1a2b3").is_err());  // too short
        assert!(Color::from_hex("#gggggg").is_err()); // invalid hex
    }

    #[test]
    fn format_key_uses_shorthand_and_format_key_spelled_spells_it_out() {
        let ke = KeyEvent::new(Char('m'), M::ALT);
        assert_eq!(format_key(&ke), "A-m");
        assert_eq!(format_key_spelled(&ke), "Alt-m");
    }

    #[test]
    fn view_default_binding_is_f3() {
        let cfg = Config::load_from_str("{}").unwrap();
        assert_eq!(cfg.keybindings.view, vec![single(F(3), M::NONE)]);
    }

    #[test]
    fn view_binding_configurable() {
        let cfg = Config::load_from_str(r#"{"keybindings":{"view":"Alt-v"}}"#).unwrap();
        assert_eq!(cfg.keybindings.view, vec![single(Char('v'), M::ALT)]);
    }

    #[test]
    fn panels_default_action_fields_have_expected_defaults() {
        let cfg = Config::load_from_str("{}").unwrap();
        assert_eq!(cfg.panels.default_action_executable, "");
        assert_eq!(cfg.panels.default_action_text, ":view");
        assert_eq!(cfg.panels.default_action, "");
    }

    #[test]
    fn panels_default_action_fields_configurable() {
        let json = r#"{"panels":{
            "default_action_executable": "%f",
            "default_action_text": "echo %f",
            "default_action": "true"
        }}"#;
        let cfg = Config::load_from_str(json).unwrap();
        assert_eq!(cfg.panels.default_action_executable, "%f");
        assert_eq!(cfg.panels.default_action_text, "echo %f");
        assert_eq!(cfg.panels.default_action, "true");
    }
}

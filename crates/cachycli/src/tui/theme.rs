//! Themes. The default palette (`cachy-night`) is derived from the visual
//! inspection of the reference CLI: background #141414, borders #333333,
//! focused borders #505058, text #E1E1E1, muted #6C6C6C, accent #E0AF68.

use ratatui::style::Color;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Theme {
    pub name: &'static str,
    /// True for light backgrounds.
    pub light: bool,
    pub bg: Color,
    /// Slightly raised surface (selected rows).
    pub surface: Color,
    /// Input line / stronger surface.
    pub surface_strong: Color,
    /// Row under the pointer.
    pub hover: Color,
    /// Card and panel borders.
    pub border: Color,
    /// Focused box borders (prompt box).
    pub border_focused: Color,
    /// Modal frame.
    pub border_modal: Color,
    pub text: Color,
    pub text_strong: Color,
    pub text_muted: Color,
    pub text_dim: Color,
    pub accent: Color,
    pub success: Color,
    pub error: Color,
    pub warning: Color,
    /// Diff colors.
    pub diff_added: Color,
    pub diff_removed: Color,
    /// Syntax highlight base colors.
    pub code_bg: Color,
    pub code_text: Color,
    pub code_comment: Color,
    pub code_keyword: Color,
    pub code_string: Color,
    pub code_number: Color,
    pub code_function: Color,
    pub code_type: Color,
}

impl Theme {
    pub fn by_name(name: &str) -> Self {
        match name.to_ascii_lowercase().replace([' ', '_'], "-").as_str() {
            "cachy-day" | "day" | "light" => Self::cachy_day(),
            "tokyonight" | "tokyo-night" => Self::tokyo_night(),
            "rosepine-moon" | "rose-pine-moon" | "rosepine" => Self::rose_pine_moon(),
            "oscura-midnight" | "oscura" => Self::oscura_midnight(),
            _ => Self::cachy_night(),
        }
    }

    /// All built-in themes, for the settings modal.
    pub fn all() -> Vec<Self> {
        vec![
            Self::cachy_night(),
            Self::cachy_day(),
            Self::tokyo_night(),
            Self::rose_pine_moon(),
            Self::oscura_midnight(),
        ]
    }

    pub fn cachy_night() -> Self {
        Self {
            name: "Cachy Night",
            light: false,
            bg: Color::Rgb(20, 20, 20),             // #141414
            surface: Color::Rgb(36, 36, 36),        // #242424
            surface_strong: Color::Rgb(54, 54, 54), // #363636
            hover: Color::Rgb(29, 29, 30),
            border: Color::Rgb(51, 51, 51),         // #333333
            border_focused: Color::Rgb(80, 80, 88), // #505058
            border_modal: Color::Rgb(88, 88, 88),   // #585858
            text: Color::Rgb(200, 200, 200),        // #C8C8C8
            text_strong: Color::Rgb(225, 225, 225), // #E1E1E1
            text_muted: Color::Rgb(108, 108, 108),  // #6C6C6C
            text_dim: Color::Rgb(120, 120, 120),    // #787878
            accent: Color::Rgb(224, 175, 104),      // #E0AF68
            success: Color::Rgb(158, 206, 106),     // #9ECE6A
            error: Color::Rgb(247, 118, 142),       // #F7768E
            warning: Color::Rgb(224, 175, 104),
            diff_added: Color::Rgb(158, 206, 106),
            diff_removed: Color::Rgb(247, 118, 142),
            code_bg: Color::Rgb(26, 26, 30),
            code_text: Color::Rgb(200, 200, 200),
            code_comment: Color::Rgb(108, 108, 118),
            code_keyword: Color::Rgb(187, 154, 247),
            code_string: Color::Rgb(158, 206, 106),
            code_number: Color::Rgb(255, 158, 100),
            code_function: Color::Rgb(125, 207, 255),
            code_type: Color::Rgb(42, 195, 222),
        }
    }

    pub fn cachy_day() -> Self {
        Self {
            name: "Cachy Day",
            light: true,
            bg: Color::Rgb(250, 250, 250),
            surface: Color::Rgb(238, 238, 240),
            surface_strong: Color::Rgb(226, 226, 230),
            hover: Color::Rgb(232, 232, 236),
            border: Color::Rgb(210, 210, 214),
            border_focused: Color::Rgb(150, 150, 160),
            border_modal: Color::Rgb(170, 170, 178),
            text: Color::Rgb(54, 54, 60),
            text_strong: Color::Rgb(20, 20, 24),
            text_muted: Color::Rgb(120, 120, 128),
            text_dim: Color::Rgb(140, 140, 148),
            accent: Color::Rgb(180, 120, 20),
            success: Color::Rgb(60, 140, 60),
            error: Color::Rgb(190, 60, 80),
            warning: Color::Rgb(180, 120, 20),
            diff_added: Color::Rgb(60, 140, 60),
            diff_removed: Color::Rgb(190, 60, 80),
            code_bg: Color::Rgb(240, 240, 244),
            code_text: Color::Rgb(54, 54, 60),
            code_comment: Color::Rgb(130, 130, 140),
            code_keyword: Color::Rgb(120, 70, 190),
            code_string: Color::Rgb(60, 130, 60),
            code_number: Color::Rgb(200, 100, 40),
            code_function: Color::Rgb(40, 100, 190),
            code_type: Color::Rgb(30, 130, 150),
        }
    }

    pub fn tokyo_night() -> Self {
        Self {
            name: "Tokyo Night",
            light: false,
            bg: Color::Rgb(26, 27, 38),
            surface: Color::Rgb(36, 40, 59),
            surface_strong: Color::Rgb(41, 46, 66),
            hover: Color::Rgb(30, 34, 51),
            border: Color::Rgb(52, 59, 88),
            border_focused: Color::Rgb(65, 72, 104),
            border_modal: Color::Rgb(65, 72, 104),
            text: Color::Rgb(169, 177, 214),
            text_strong: Color::Rgb(192, 202, 245),
            text_muted: Color::Rgb(86, 95, 137),
            text_dim: Color::Rgb(115, 122, 162),
            accent: Color::Rgb(187, 154, 247),
            success: Color::Rgb(158, 206, 106),
            error: Color::Rgb(247, 118, 142),
            warning: Color::Rgb(224, 175, 104),
            diff_added: Color::Rgb(158, 206, 106),
            diff_removed: Color::Rgb(247, 118, 142),
            code_bg: Color::Rgb(31, 33, 48),
            code_text: Color::Rgb(169, 177, 214),
            code_comment: Color::Rgb(86, 95, 137),
            code_keyword: Color::Rgb(187, 154, 247),
            code_string: Color::Rgb(158, 206, 106),
            code_number: Color::Rgb(255, 158, 100),
            code_function: Color::Rgb(125, 207, 255),
            code_type: Color::Rgb(42, 195, 222),
        }
    }

    pub fn rose_pine_moon() -> Self {
        Self {
            name: "Rosé Pine Moon",
            light: false,
            bg: Color::Rgb(35, 33, 54),
            surface: Color::Rgb(42, 39, 63),
            surface_strong: Color::Rgb(57, 53, 82),
            hover: Color::Rgb(35, 32, 53),
            border: Color::Rgb(68, 65, 90),
            border_focused: Color::Rgb(86, 82, 110),
            border_modal: Color::Rgb(86, 82, 110),
            text: Color::Rgb(224, 222, 244),
            text_strong: Color::Rgb(244, 242, 255),
            text_muted: Color::Rgb(110, 106, 134),
            text_dim: Color::Rgb(144, 140, 170),
            accent: Color::Rgb(246, 193, 119),
            success: Color::Rgb(156, 207, 216),
            error: Color::Rgb(235, 111, 146),
            warning: Color::Rgb(246, 193, 119),
            diff_added: Color::Rgb(156, 207, 216),
            diff_removed: Color::Rgb(235, 111, 146),
            code_bg: Color::Rgb(42, 39, 63),
            code_text: Color::Rgb(224, 222, 244),
            code_comment: Color::Rgb(110, 106, 134),
            code_keyword: Color::Rgb(196, 167, 231),
            code_string: Color::Rgb(246, 193, 119),
            code_number: Color::Rgb(234, 154, 151),
            code_function: Color::Rgb(156, 207, 216),
            code_type: Color::Rgb(49, 116, 143),
        }
    }

    pub fn oscura_midnight() -> Self {
        Self {
            name: "Oscura Midnight",
            light: false,
            bg: Color::Rgb(12, 14, 20),
            surface: Color::Rgb(22, 25, 34),
            surface_strong: Color::Rgb(32, 36, 48),
            hover: Color::Rgb(18, 21, 29),
            border: Color::Rgb(38, 42, 54),
            border_focused: Color::Rgb(58, 64, 82),
            border_modal: Color::Rgb(58, 64, 82),
            text: Color::Rgb(186, 191, 204),
            text_strong: Color::Rgb(222, 226, 236),
            text_muted: Color::Rgb(92, 98, 116),
            text_dim: Color::Rgb(120, 127, 148),
            accent: Color::Rgb(130, 170, 255),
            success: Color::Rgb(120, 200, 140),
            error: Color::Rgb(240, 110, 130),
            warning: Color::Rgb(230, 180, 100),
            diff_added: Color::Rgb(120, 200, 140),
            diff_removed: Color::Rgb(240, 110, 130),
            code_bg: Color::Rgb(18, 20, 28),
            code_text: Color::Rgb(186, 191, 204),
            code_comment: Color::Rgb(92, 98, 116),
            code_keyword: Color::Rgb(170, 150, 240),
            code_string: Color::Rgb(140, 200, 150),
            code_number: Color::Rgb(240, 160, 120),
            code_function: Color::Rgb(130, 180, 240),
            code_type: Color::Rgb(100, 210, 220),
        }
    }
}

impl Default for Theme {
    fn default() -> Self {
        Self::cachy_night()
    }
}

/// The ASCII/braille logo shown on the welcome card (5 rows, 12 columns).
pub const LOGO: [&str; 5] = [
    "⢠⠒⠒⠒⠒⠒⠒⠒⠒⠒⠒⡄",
    "⢸⠀⠀⠲⣄⠀⠀⠀⠀⠀⠀⡇",
    "⢸⠀⠀⠀⣨⠅⣀⣀⣀⠀⠀⡇",
    "⢸⠀⠀⠚⠁⠀⠉⠉⠉⠀⠀⡇",
    "⠘⠤⠤⠤⠤⠤⠤⠤⠤⠤⠤⠃",
];

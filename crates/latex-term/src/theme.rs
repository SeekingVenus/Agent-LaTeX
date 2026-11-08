//! Colour themes. Themes are semantic: layout tags cells with a [`Role`] and
//! the theme decides what that role looks like.

use latex_layout::Role;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Color {
    Default,
    /// An ANSI 256-colour index.
    Ansi256(u8),
    Rgb(u8, u8, u8),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RoleStyle {
    pub color: Color,
    pub bold: bool,
    pub dim: bool,
    pub italic: bool,
}

impl RoleStyle {
    pub const fn plain() -> Self {
        Self {
            color: Color::Default,
            bold: false,
            dim: false,
            italic: false,
        }
    }

    pub const fn fg(color: Color) -> Self {
        Self {
            color,
            bold: false,
            dim: false,
            italic: false,
        }
    }

    pub const fn bold(mut self) -> Self {
        self.bold = true;
        self
    }

    pub const fn dim(mut self) -> Self {
        self.dim = true;
        self
    }

    pub const fn italic(mut self) -> Self {
        self.italic = true;
        self
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Theme {
    roles: [RoleStyle; Role::COUNT],
    color: bool,
}

impl Theme {
    pub fn mono() -> Self {
        Self {
            roles: [RoleStyle::plain(); Role::COUNT],
            color: false,
        }
    }

    pub fn ansi() -> Self {
        let mut roles = [RoleStyle::plain(); Role::COUNT];
        roles[Role::Variable.index()] = RoleStyle::fg(Color::Ansi256(39));
        roles[Role::Number.index()] = RoleStyle::fg(Color::Ansi256(220));
        roles[Role::Operator.index()] = RoleStyle::fg(Color::Ansi256(213));
        roles[Role::Relation.index()] = RoleStyle::fg(Color::Ansi256(75));
        roles[Role::Delimiter.index()] = RoleStyle::plain();
        roles[Role::Rule.index()] = RoleStyle::fg(Color::Ansi256(244)).dim();
        roles[Role::Accent.index()] = RoleStyle::fg(Color::Ansi256(81));
        roles[Role::Error.index()] = RoleStyle::fg(Color::Ansi256(203));
        roles[Role::Muted.index()] = RoleStyle::fg(Color::Ansi256(244)).dim();
        Self { roles, color: true }
    }

    pub fn dark() -> Self {
        let mut roles = [RoleStyle::plain(); Role::COUNT];
        roles[Role::Text.index()] = RoleStyle::fg(Color::Rgb(0xD4, 0xD4, 0xD4));
        roles[Role::Variable.index()] = RoleStyle::fg(Color::Rgb(0x9C, 0xDC, 0xFE));
        roles[Role::Number.index()] = RoleStyle::fg(Color::Rgb(0xB5, 0xCE, 0xA8));
        roles[Role::Operator.index()] = RoleStyle::fg(Color::Rgb(0xC5, 0x86, 0xC0));
        roles[Role::Relation.index()] = RoleStyle::fg(Color::Rgb(0x56, 0x9C, 0xD6));
        roles[Role::Delimiter.index()] = RoleStyle::fg(Color::Rgb(0xD4, 0xD4, 0xD4));
        roles[Role::Rule.index()] = RoleStyle::fg(Color::Rgb(0x80, 0x80, 0x80)).dim();
        roles[Role::Accent.index()] = RoleStyle::fg(Color::Rgb(0x4E, 0xC9, 0xB0));
        roles[Role::Error.index()] = RoleStyle::fg(Color::Rgb(0xF4, 0x47, 0x47));
        roles[Role::Muted.index()] = RoleStyle::fg(Color::Rgb(0x80, 0x80, 0x80)).dim();
        Self { roles, color: true }
    }

    pub fn light() -> Self {
        let mut roles = [RoleStyle::plain(); Role::COUNT];
        roles[Role::Text.index()] = RoleStyle::fg(Color::Rgb(0x1F, 0x23, 0x28));
        roles[Role::Variable.index()] = RoleStyle::fg(Color::Rgb(0x05, 0x50, 0xAE));
        roles[Role::Number.index()] = RoleStyle::fg(Color::Rgb(0x09, 0x86, 0x58));
        roles[Role::Operator.index()] = RoleStyle::fg(Color::Rgb(0x82, 0x50, 0xDF));
        roles[Role::Relation.index()] = RoleStyle::fg(Color::Rgb(0x09, 0x69, 0xDA));
        roles[Role::Delimiter.index()] = RoleStyle::fg(Color::Rgb(0x1F, 0x23, 0x28));
        roles[Role::Rule.index()] = RoleStyle::fg(Color::Rgb(0x8C, 0x95, 0x9F)).dim();
        roles[Role::Accent.index()] = RoleStyle::fg(Color::Rgb(0x1A, 0x7F, 0x37));
        roles[Role::Error.index()] = RoleStyle::fg(Color::Rgb(0xCF, 0x22, 0x2E));
        roles[Role::Muted.index()] = RoleStyle::fg(Color::Rgb(0x57, 0x60, 0x6A)).dim();
        Self { roles, color: true }
    }

    /// Pick a theme from the environment. `NO_COLOR` and `TERM=dumb` win.
    pub fn auto() -> Self {
        if std::env::var_os("NO_COLOR").is_some()
            || std::env::var("TERM")
                .map(|term| term == "dumb")
                .unwrap_or(false)
        {
            return Self::mono();
        }
        if std::env::var_os("COLORTERM").is_some() {
            Self::dark()
        } else {
            Self::ansi()
        }
    }

    pub fn from_name(name: &str) -> Option<Self> {
        match name {
            "auto" => Some(Self::auto()),
            "mono" | "none" | "plain" => Some(Self::mono()),
            "ansi" | "256" => Some(Self::ansi()),
            "dark" => Some(Self::dark()),
            "light" => Some(Self::light()),
            _ => None,
        }
    }

    pub fn with_color(mut self, color: bool) -> Self {
        self.color = color;
        self
    }

    pub fn color_enabled(&self) -> bool {
        self.color
    }

    pub fn role(&self, role: Role) -> RoleStyle {
        if self.color {
            self.roles[role.index()]
        } else {
            RoleStyle::plain()
        }
    }
}

impl Default for Theme {
    fn default() -> Self {
        Self::auto()
    }
}

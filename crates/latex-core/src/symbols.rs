//! TeX command tables.
//!
//! The tables cover the subset that appears in ordinary conversational math:
//! Greek letters, operators, relations, arrows, sets, logic, big operators,
//! function names, accents, delimiters and text escapes. Unknown commands are
//! preserved by the parser instead of being dropped.

use crate::ast::{AccentKind, AtomClass, Delimiter, Face, Space};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SymbolSpec {
    pub ch: char,
    pub class: AtomClass,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BigOpSpec {
    pub ch: char,
    pub class: AtomClass,
    pub display_limits: bool,
}

const fn sym(ch: char, class: AtomClass) -> SymbolSpec {
    SymbolSpec { ch, class }
}

/// Look up a command that resolves to a single glyph.
pub fn symbol(name: &str) -> Option<SymbolSpec> {
    use AtomClass::*;

    Some(match name {
        // Greek, lower case.
        "alpha" => sym('α', Ord),
        "beta" => sym('β', Ord),
        "gamma" => sym('γ', Ord),
        "delta" => sym('δ', Ord),
        "epsilon" => sym('ε', Ord),
        "varepsilon" => sym('ε', Ord),
        "zeta" => sym('ζ', Ord),
        "eta" => sym('η', Ord),
        "theta" => sym('θ', Ord),
        "vartheta" => sym('ϑ', Ord),
        "iota" => sym('ι', Ord),
        "kappa" => sym('κ', Ord),
        "varkappa" => sym('ϰ', Ord),
        "lambda" => sym('λ', Ord),
        "mu" => sym('μ', Ord),
        "nu" => sym('ν', Ord),
        "xi" => sym('ξ', Ord),
        "omicron" => sym('ο', Ord),
        "pi" => sym('π', Ord),
        "varpi" => sym('ϖ', Ord),
        "rho" => sym('ρ', Ord),
        "varrho" => sym('ϱ', Ord),
        "sigma" => sym('σ', Ord),
        "varsigma" => sym('ς', Ord),
        "tau" => sym('τ', Ord),
        "upsilon" => sym('υ', Ord),
        "phi" => sym('φ', Ord),
        "varphi" => sym('ϕ', Ord),
        "chi" => sym('χ', Ord),
        "psi" => sym('ψ', Ord),
        "omega" => sym('ω', Ord),

        // Greek, upper case.
        "Gamma" => sym('Γ', Ord),
        "Delta" => sym('Δ', Ord),
        "Theta" => sym('Θ', Ord),
        "Lambda" => sym('Λ', Ord),
        "Xi" => sym('Ξ', Ord),
        "Pi" => sym('Π', Ord),
        "Sigma" => sym('Σ', Ord),
        "Upsilon" => sym('Υ', Ord),
        "Phi" => sym('Φ', Ord),
        "Psi" => sym('Ψ', Ord),
        "Omega" => sym('Ω', Ord),

        // Binary operators.
        "pm" => sym('±', Bin),
        "mp" => sym('∓', Bin),
        "times" => sym('×', Bin),
        "div" => sym('÷', Bin),
        "cdot" | "cdotp" => sym('⋅', Bin),
        "ast" => sym('∗', Bin),
        "star" => sym('⋆', Bin),
        "circ" => sym('∘', Bin),
        "bullet" => sym('∙', Bin),
        "oplus" => sym('⊕', Bin),
        "ominus" => sym('⊖', Bin),
        "otimes" => sym('⊗', Bin),
        "oslash" => sym('⊘', Bin),
        "odot" => sym('⊙', Bin),
        "dagger" | "dag" => sym('†', Bin),
        "ddagger" | "ddag" => sym('‡', Bin),
        "amalg" => sym('⨿', Bin),
        "wr" => sym('≀', Bin),
        "uplus" => sym('⊎', Bin),
        "sqcup" => sym('⊔', Bin),
        "sqcap" => sym('⊓', Bin),
        "wedge" | "land" => sym('∧', Bin),
        "vee" | "lor" => sym('∨', Bin),
        "setminus" | "smallsetminus" => sym('∖', Bin),
        "triangleleft" | "lhd" => sym('◁', Bin),
        "triangleright" | "rhd" => sym('▷', Bin),
        "unlhd" => sym('⊴', Bin),
        "unrhd" => sym('⊵', Bin),
        "dotplus" => sym('∔', Bin),

        // Relations.
        "neq" | "ne" => sym('≠', Rel),
        "le" | "leq" => sym('≤', Rel),
        "ge" | "geq" => sym('≥', Rel),
        "ll" => sym('≪', Rel),
        "gg" => sym('≫', Rel),
        "prec" => sym('≺', Rel),
        "succ" => sym('≻', Rel),
        "preceq" => sym('⪯', Rel),
        "succeq" => sym('⪰', Rel),
        "sim" => sym('∼', Rel),
        "simeq" => sym('≃', Rel),
        "approx" => sym('≈', Rel),
        "cong" => sym('≅', Rel),
        "equiv" => sym('≡', Rel),
        "propto" => sym('∝', Rel),
        "asymp" => sym('≍', Rel),
        "doteq" => sym('≐', Rel),
        "models" => sym('⊨', Rel),
        "vdash" => sym('⊢', Rel),
        "dashv" => sym('⊣', Rel),
        "perp" => sym('⊥', Rel),
        "parallel" => sym('∥', Rel),
        "mid" => sym('∣', Rel),
        "nmid" => sym('∤', Rel),
        "in" => sym('∈', Rel),
        "notin" => sym('∉', Rel),
        "ni" | "owns" => sym('∋', Rel),
        "subset" => sym('⊂', Rel),
        "supset" => sym('⊃', Rel),
        "subseteq" => sym('⊆', Rel),
        "supseteq" => sym('⊇', Rel),
        "sqsubset" => sym('⊏', Rel),
        "sqsupset" => sym('⊐', Rel),
        "sqsubseteq" => sym('⊑', Rel),
        "sqsupseteq" => sym('⊒', Rel),
        "bowtie" | "Join" => sym('⋈', Rel),
        "smile" => sym('⌣', Rel),
        "frown" => sym('⌢', Rel),
        "leqslant" => sym('⩽', Rel),
        "geqslant" => sym('⩾', Rel),
        "lesssim" => sym('≲', Rel),
        "gtrsim" => sym('≳', Rel),
        "approxeq" => sym('≊', Rel),
        "triangleq" => sym('≜', Rel),
        "therefore" => sym('∴', Rel),
        "because" => sym('∵', Rel),

        // Arrows.
        "to" | "rightarrow" => sym('→', Rel),
        "leftarrow" | "gets" => sym('←', Rel),
        "leftrightarrow" => sym('↔', Rel),
        "Rightarrow" | "implies" => sym('⇒', Rel),
        "Leftarrow" | "impliedby" => sym('⇐', Rel),
        "Leftrightarrow" | "iff" => sym('⇔', Rel),
        "mapsto" => sym('↦', Rel),
        "longrightarrow" => sym('⟶', Rel),
        "longleftarrow" => sym('⟵', Rel),
        "longleftrightarrow" => sym('⟷', Rel),
        "Longrightarrow" => sym('⟹', Rel),
        "Longleftarrow" => sym('⟸', Rel),
        "Longleftrightarrow" => sym('⟺', Rel),
        "uparrow" => sym('↑', Rel),
        "downarrow" => sym('↓', Rel),
        "updownarrow" => sym('↕', Rel),
        "Uparrow" => sym('⇑', Rel),
        "Downarrow" => sym('⇓', Rel),
        "Updownarrow" => sym('⇕', Rel),
        "nearrow" => sym('↗', Rel),
        "searrow" => sym('↘', Rel),
        "swarrow" => sym('↙', Rel),
        "nwarrow" => sym('↖', Rel),
        "hookrightarrow" => sym('↪', Rel),
        "hookleftarrow" => sym('↩', Rel),
        "rightharpoonup" => sym('⇀', Rel),
        "rightharpoondown" => sym('⇁', Rel),
        "leftharpoonup" => sym('↼', Rel),
        "leftharpoondown" => sym('↽', Rel),
        "rightleftharpoons" => sym('⇌', Rel),
        "leadsto" => sym('↝', Rel),
        "dashrightarrow" => sym('⇢', Rel),
        "dashleftarrow" => sym('⇠', Rel),

        // Logic, sets and miscellaneous symbols.
        "forall" => sym('∀', Ord),
        "exists" => sym('∃', Ord),
        "nexists" => sym('∄', Ord),
        "neg" | "lnot" => sym('¬', Ord),
        "emptyset" | "varnothing" => sym('∅', Ord),
        "complement" => sym('∁', Ord),
        "top" => sym('⊤', Ord),
        "bot" => sym('⊥', Ord),
        "angle" => sym('∠', Ord),
        "measuredangle" => sym('∡', Ord),
        "sphericalangle" => sym('∢', Ord),
        "triangle" => sym('△', Ord),
        "square" | "Box" => sym('□', Ord),
        "blacksquare" => sym('■', Ord),
        "blacktriangle" => sym('▲', Ord),
        "Diamond" | "lozenge" => sym('◇', Ord),
        "blacklozenge" => sym('◆', Ord),
        "bigstar" => sym('★', Ord),
        "infty" => sym('∞', Ord),
        "partial" => sym('∂', Ord),
        "nabla" => sym('∇', Ord),
        "hbar" => sym('ℏ', Ord),
        "ell" => sym('ℓ', Ord),
        "Re" => sym('ℜ', Ord),
        "Im" => sym('ℑ', Ord),
        "aleph" => sym('ℵ', Ord),
        "beth" => sym('ℶ', Ord),
        "gimel" => sym('ℷ', Ord),
        "daleth" => sym('ℸ', Ord),
        "wp" => sym('℘', Ord),
        "prime" => sym('′', Ord),
        "doubleprime" => sym('″', Ord),
        "degree" => sym('°', Ord),
        "ldots" | "dots" => sym('…', Ord),
        "cdots" => sym('⋯', Ord),
        "vdots" => sym('⋮', Ord),
        "ddots" => sym('⋱', Ord),
        "colon" => sym(':', Punct),
        "checkmark" => sym('✓', Ord),
        "S" => sym('§', Ord),
        "P" => sym('¶', Ord),
        "copyright" => sym('©', Ord),
        "pounds" => sym('£', Ord),
        "euro" => sym('€', Ord),
        "yen" => sym('¥', Ord),

        // Delimiters used as ordinary symbols outside \left...\right.
        "langle" => sym('⟨', Open),
        "rangle" => sym('⟩', Close),
        "lceil" => sym('⌈', Open),
        "rceil" => sym('⌉', Close),
        "lfloor" => sym('⌊', Open),
        "rfloor" => sym('⌋', Close),
        "lvert" => sym('|', Open),
        "rvert" => sym('|', Close),
        "lVert" => sym('‖', Open),
        "rVert" => sym('‖', Close),
        "vert" => sym('|', Ord),
        "Vert" => sym('‖', Ord),
        "backslash" => sym('\\', Ord),
        "slash" => sym('/', Ord),
        "lbrace" => sym('{', Open),
        "rbrace" => sym('}', Close),
        "lbrack" => sym('[', Open),
        "rbrack" => sym(']', Close),
        "lparen" => sym('(', Open),
        "rparen" => sym(')', Close),

        // Escaped text characters.
        "%" => sym('%', Ord),
        "$" => sym('$', Ord),
        "&" => sym('&', Ord),
        "#" => sym('#', Ord),
        "_" => sym('_', Ord),
        "{" => sym('{', Open),
        "}" => sym('}', Close),
        "|" => sym('‖', Ord),
        "textbackslash" => sym('\\', Ord),
        "textbar" => sym('|', Ord),
        "textless" => sym('<', Ord),
        "textgreater" => sym('>', Ord),
        "textasciitilde" => sym('~', Ord),
        "textasciicircum" => sym('^', Ord),

        _ => return None,
    })
}

/// Look up a big operator (one that can take limits above and below).
pub fn big_operator(name: &str) -> Option<BigOpSpec> {
    use AtomClass::*;
    Some(match name {
        "sum" => BigOpSpec {
            ch: '∑',
            class: Op,
            display_limits: true,
        },
        "prod" => BigOpSpec {
            ch: '∏',
            class: Op,
            display_limits: true,
        },
        "coprod" => BigOpSpec {
            ch: '∐',
            class: Op,
            display_limits: true,
        },
        "int" => BigOpSpec {
            ch: '∫',
            class: Op,
            display_limits: false,
        },
        "iint" => BigOpSpec {
            ch: '∬',
            class: Op,
            display_limits: false,
        },
        "iiint" => BigOpSpec {
            ch: '∭',
            class: Op,
            display_limits: false,
        },
        "iiiint" => BigOpSpec {
            ch: '⨌',
            class: Op,
            display_limits: false,
        },
        "oint" => BigOpSpec {
            ch: '∮',
            class: Op,
            display_limits: false,
        },
        "oiint" => BigOpSpec {
            ch: '∯',
            class: Op,
            display_limits: false,
        },
        "oiiint" => BigOpSpec {
            ch: '∰',
            class: Op,
            display_limits: false,
        },
        "bigcap" => BigOpSpec {
            ch: '⋂',
            class: Op,
            display_limits: true,
        },
        "bigcup" => BigOpSpec {
            ch: '⋃',
            class: Op,
            display_limits: true,
        },
        "bigsqcup" => BigOpSpec {
            ch: '⨆',
            class: Op,
            display_limits: true,
        },
        "bigvee" => BigOpSpec {
            ch: '⋁',
            class: Op,
            display_limits: true,
        },
        "bigwedge" => BigOpSpec {
            ch: '⋀',
            class: Op,
            display_limits: true,
        },
        "bigodot" => BigOpSpec {
            ch: '⨀',
            class: Op,
            display_limits: true,
        },
        "bigotimes" => BigOpSpec {
            ch: '⨂',
            class: Op,
            display_limits: true,
        },
        "bigoplus" => BigOpSpec {
            ch: '⨁',
            class: Op,
            display_limits: true,
        },
        "biguplus" => BigOpSpec {
            ch: '⨄',
            class: Op,
            display_limits: true,
        },
        "bigtimes" => BigOpSpec {
            ch: '⨉',
            class: Op,
            display_limits: true,
        },
        _ => return None,
    })
}

/// Function names that are set upright.
pub fn function(name: &str) -> Option<&'static str> {
    Some(match name {
        "sin" => "sin",
        "cos" => "cos",
        "tan" => "tan",
        "cot" => "cot",
        "sec" => "sec",
        "csc" => "csc",
        "arcsin" => "arcsin",
        "arccos" => "arccos",
        "arctan" => "arctan",
        "sinh" => "sinh",
        "cosh" => "cosh",
        "tanh" => "tanh",
        "coth" => "coth",
        "sech" => "sech",
        "csch" => "csch",
        "log" => "log",
        "ln" => "ln",
        "lg" => "lg",
        "exp" => "exp",
        "lim" => "lim",
        "limsup" => "lim sup",
        "liminf" => "lim inf",
        "max" => "max",
        "min" => "min",
        "sup" => "sup",
        "inf" => "inf",
        "det" => "det",
        "dim" => "dim",
        "gcd" => "gcd",
        "hom" => "hom",
        "ker" => "ker",
        "Pr" => "Pr",
        "arg" => "arg",
        "deg" => "deg",
        "mod" => "mod",
        "bmod" => "mod",
        _ => return None,
    })
}

/// Look up a named delimiter.
pub fn delimiter(name: &str) -> Option<Delimiter> {
    Some(match name {
        "{" | "lbrace" | "rbrace" => Delimiter::Brace,
        "}" => Delimiter::Brace,
        "|" => Delimiter::DoubleBar,
        "vert" | "lvert" | "rvert" => Delimiter::Bar,
        "Vert" | "lVert" | "rVert" => Delimiter::DoubleBar,
        "langle" | "rangle" => Delimiter::Angle,
        "lceil" | "rceil" => Delimiter::Ceil,
        "lfloor" | "rfloor" => Delimiter::Floor,
        "backslash" => Delimiter::Backslash,
        "slash" => Delimiter::Slash,
        _ => return None,
    })
}

/// The single glyph used for a delimiter at one row.
pub fn delimiter_char(delimiter: Delimiter, open: bool) -> char {
    match delimiter {
        Delimiter::None => ' ',
        Delimiter::Paren => {
            if open {
                '('
            } else {
                ')'
            }
        }
        Delimiter::Bracket => {
            if open {
                '['
            } else {
                ']'
            }
        }
        Delimiter::Brace => {
            if open {
                '{'
            } else {
                '}'
            }
        }
        Delimiter::Angle => {
            if open {
                '⟨'
            } else {
                '⟩'
            }
        }
        Delimiter::Bar => '|',
        Delimiter::DoubleBar => '‖',
        Delimiter::Floor => {
            if open {
                '⌊'
            } else {
                '⌋'
            }
        }
        Delimiter::Ceil => {
            if open {
                '⌈'
            } else {
                '⌉'
            }
        }
        Delimiter::Slash => '/',
        Delimiter::Backslash => '\\',
        Delimiter::Custom(ch) => ch,
    }
}

/// Look up an accent command.
pub fn accent(name: &str) -> Option<AccentKind> {
    Some(match name {
        "hat" | "widehat" => AccentKind::Hat,
        "bar" => AccentKind::Bar,
        "overline" => AccentKind::Overline,
        "vec" => AccentKind::Vec,
        "dot" => AccentKind::Dot,
        "ddot" => AccentKind::Ddot,
        "tilde" | "widetilde" => AccentKind::Tilde,
        "check" => AccentKind::Check,
        "breve" => AccentKind::Breve,
        "acute" => AccentKind::Acute,
        "grave" => AccentKind::Grave,
        "underline" => AccentKind::Underline,
        _ => return None,
    })
}

/// Look up an explicit spacing command.
pub fn space(name: &str) -> Option<Space> {
    Some(match name {
        "," | "thinspace" => Space::Thin,
        ":" | ">" | "medspace" => Space::Medium,
        ";" | "thickspace" => Space::Thick,
        "quad" => Space::Quad,
        "qquad" => Space::DoubleQuad,
        "!" | "negthinspace" => Space::NegativeThin,
        " " => Space::Normal,
        _ => return None,
    })
}

/// Look up a command that selects a font face.
pub fn text_face(name: &str) -> Option<Face> {
    Some(match name {
        "mathrm" | "rm" | "textrm" | "textnormal" | "mbox" | "text" => Face::Roman,
        "mathbf" | "bf" | "textbf" | "boldsymbol" => Face::Bold,
        "mathit" | "it" | "textit" => Face::Italic,
        "mathsf" | "sf" | "textsf" => Face::SansSerif,
        "mathtt" | "tt" | "texttt" => Face::Monospace,
        "mathbb" | "Bbb" => Face::Blackboard,
        "mathcal" | "cal" => Face::Calligraphic,
        "mathfrak" | "frak" => Face::Fraktur,
        _ => return None,
    })
}

/// Escapes and spacing inside `\text{...}`.
pub fn text_symbol(name: &str) -> Option<&'static str> {
    Some(match name {
        "%" => "%",
        "$" => "$",
        "&" => "&",
        "#" => "#",
        "_" => "_",
        "{" => "{",
        "}" => "}",
        " " => " ",
        "textbackslash" => "\\",
        "textbar" => "|",
        "textless" => "<",
        "textgreater" => ">",
        "textasciitilde" => "~",
        "textasciicircum" => "^",
        "ldots" | "dots" => "…",
        "cdots" => "⋯",
        "," => "\u{2009}",
        ":" => "\u{2005}",
        ";" => "\u{2004}",
        "!" => "",
        "quad" => "  ",
        "qquad" => "    ",
        _ => return None,
    })
}

/// The replacement glyph for `\not<command>`.
pub fn negated(name: &str) -> Option<char> {
    Some(match name {
        "=" | "eq" => '≠',
        "in" => '∉',
        "ni" | "owns" => '∌',
        "subset" => '⊄',
        "supset" => '⊅',
        "subseteq" => '⊈',
        "supseteq" => '⊉',
        "equiv" => '≢',
        "sim" => '≁',
        "approx" => '≉',
        "le" | "leq" => '≰',
        "ge" | "geq" => '≱',
        "prec" => '⊀',
        "succ" => '⊁',
        "mid" => '∤',
        "parallel" => '∦',
        "exists" => '∄',
        _ => return None,
    })
}

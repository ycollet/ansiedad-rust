use crate::{
    config::{Cell, Config, Settings},
    expr::{Constant, Context, Expr},
};

pub const STARTER: &str = "\
# Graphical canvas: 0 = characters, 1 = RGBA pixels.
# x/y are coordinates; frame advances once per frame.
mode = 1
w = 160; l = 100; fps = 30
b = \"(x+frame)&255\"; f = \"15\"; c = \"144+(y^frame)%4\"
rgba = \"(((x+frame)&255)<<24)|(((y+frame)&255)<<16)|(((x^y)&255)<<8)|255\"
";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    Characters,
    Pixels,
}

enum Renderer {
    Characters(Config),
    Pixels { settings: Settings, rgba: Expr },
}

pub struct CanvasConfig {
    renderer: Renderer,
}

pub enum CanvasFrame {
    Characters {
        size: [usize; 2],
        cells: Vec<Cell>,
    },
    Pixels {
        size: [usize; 2],
        pixels: Vec<[u8; 4]>,
    },
}

impl CanvasFrame {
    pub fn size(&self) -> [usize; 2] {
        match self {
            Self::Characters { size, .. } | Self::Pixels { size, .. } => *size,
        }
    }
}

impl CanvasConfig {
    pub fn parse(source: &str) -> Result<Self, String> {
        let mut settings = Settings::parse(source)?;
        settings.validate()?;
        let mode = match settings.constants.remove("mode") {
            None | Some(Constant::Integer(0)) => Mode::Characters,
            Some(Constant::Integer(1)) => Mode::Pixels,
            _ => {
                return Err(settings.at_value(
                    "mode",
                    "mode must be an unquoted integer: 0 (characters) or 1 (pixels)",
                ));
            }
        };
        let rgba = match settings.constants.remove("rgba") {
            None => "0x000000ff".into(),
            Some(Constant::Text(expression)) => expression,
            _ => return Err(settings.at_value("rgba", "rgba must be a quoted integer expression")),
        };
        for name in ["x", "y", "frame"] {
            if settings.constants.contains_key(name) {
                return Err(
                    settings.at_name(name, format!("{name} is read-only in graphical patches"))
                );
            }
        }
        let renderer = match mode {
            Mode::Characters => Renderer::Characters(Config::from_settings(settings, true)?),
            Mode::Pixels => {
                settings.validate_bindings(true)?;
                let rgba = Expr::parse_located(&rgba, &settings.constants, true)
                    .map_err(|error| settings.expression_error("rgba", error))?;
                if rgba.is_text() {
                    return Err(settings.at_expression(
                        "rgba",
                        0,
                        "rgba must evaluate to an integer, not a string",
                    ));
                }
                Renderer::Pixels { settings, rgba }
            }
        };
        Ok(Self { renderer })
    }

    pub fn settings(&self) -> &Settings {
        match &self.renderer {
            Renderer::Characters(config) => &config.settings,
            Renderer::Pixels { settings, .. } => settings,
        }
    }

    pub fn mode(&self) -> Mode {
        match self.renderer {
            Renderer::Characters(_) => Mode::Characters,
            Renderer::Pixels { .. } => Mode::Pixels,
        }
    }

    pub fn render(&self, t: i64, frame: i64) -> Result<CanvasFrame, String> {
        let settings = self.settings();
        let size = [usize::from(settings.w), usize::from(settings.l)];
        let context = |offset: usize| Context {
            t: t.wrapping_add(offset as i64),
            x: (offset % size[0]) as i64,
            y: (offset / size[0]) as i64,
            frame,
        };
        let error = |context: Context, message| {
            format!(
                "{message} (x={}, y={}, frame={frame})",
                context.x, context.y
            )
        };
        match &self.renderer {
            Renderer::Characters(config) => {
                let cells = (0..size[0] * size[1])
                    .map(|offset| {
                        let context = context(offset);
                        config
                            .cell_context(context)
                            .map_err(|message| error(context, message))
                    })
                    .collect::<Result<_, _>>()?;
                Ok(CanvasFrame::Characters { size, cells })
            }
            Renderer::Pixels { rgba, .. } => {
                let pixels = (0..size[0] * size[1])
                    .map(|offset| {
                        let context = context(offset);
                        rgba.eval_context(context)
                            .map(|value| (value as u32).to_be_bytes())
                            .map_err(|message| {
                                error(
                                    context,
                                    settings.at_expression(
                                        "rgba",
                                        0,
                                        format!("rgba at t={}: {message}", context.t),
                                    ),
                                )
                            })
                    })
                    .collect::<Result<_, _>>()?;
                Ok(CanvasFrame::Pixels { size, pixels })
            }
        }
    }
}

pub fn ansi_color(index: u8) -> [u8; 3] {
    const BASE: [[u8; 3]; 16] = [
        [0, 0, 0],
        [205, 0, 0],
        [0, 205, 0],
        [205, 205, 0],
        [0, 0, 238],
        [205, 0, 205],
        [0, 205, 205],
        [229, 229, 229],
        [127, 127, 127],
        [255, 0, 0],
        [0, 255, 0],
        [255, 255, 0],
        [92, 92, 255],
        [255, 0, 255],
        [0, 255, 255],
        [255, 255, 255],
    ];
    match index {
        0..=15 => BASE[usize::from(index)],
        16..=231 => {
            let value = index - 16;
            let channel = |value| if value == 0 { 0 } else { 55 + value * 40 };
            [
                channel(value / 36),
                channel(value / 6 % 6),
                channel(value % 6),
            ]
        }
        _ => [8 + (index - 232) * 10; 3],
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn graphical_diagnostics_locate_settings_expressions_and_runtime_failures() {
        for (source, location) in [
            ("# canvas\n mode = 2", "line 2, column 9:"),
            ("TEXT='é'; x=0", "line 1, column 11:"),
            ("mode=1\n rgba=\"x + missing\"", "line 2, column 12:"),
            ("mode=1; rgba=2", "line 1, column 14:"),
        ] {
            let error = CanvasConfig::parse(source)
                .err()
                .expect("expected an invalid patch");
            assert!(error.starts_with(location), "{source:?}: {error}");
        }
        let config = CanvasConfig::parse("mode=1\n  rgba='1/(t-2)'").unwrap();
        let error = config
            .render(2, 3)
            .err()
            .expect("expected division by zero");
        assert_eq!(
            error,
            "line 2, column 9: rgba at t=2: division or modulo by zero (x=0, y=0, frame=3)"
        );
    }

    #[test]
    fn named_channel_expressions_follow_each_pixel_and_frame() {
        let config = CanvasConfig::parse(
            "mode=1; w=2; l=2\n\
             rgba='rval<<24 | gval<<16 | bval<<8 | aval'\n\
             rval = (x + frame) & 255\n\
             gval = y & 255\n\
             bval = (rval ^ gval) & 255\n\
             aval = (t | y) & 255",
        )
        .unwrap();
        for frame in [0, 7] {
            let CanvasFrame::Pixels { pixels, .. } = config.render(10, frame).unwrap() else {
                panic!("expected pixels");
            };
            let r = frame as u8;
            assert_eq!(
                pixels,
                [
                    [r, 0, r, 10],
                    [r + 1, 0, r + 1, 11],
                    [r, 1, r ^ 1, 13],
                    [r + 1, 1, (r + 1) ^ 1, 13]
                ]
            );
        }
        let config = CanvasConfig::parse("mode=0; w=1; l=1; shade=x+y+frame; b='shade'").unwrap();
        let CanvasFrame::Characters { cells, .. } = config.render(0, 7).unwrap() else {
            panic!("expected characters");
        };
        assert_eq!(cells[0].background, 7);
    }

    #[test]
    fn rgba_coordinates_frame_time_and_alpha_are_independent() {
        let config =
            CanvasConfig::parse("mode=1; w=2; l=2; rgba='(x<<24)|(y<<16)|(frame<<8)|(t&255)'")
                .unwrap();
        let CanvasFrame::Pixels { size, pixels } = config.render(10, 7).unwrap() else {
            panic!("expected pixels");
        };
        assert_eq!(size, [2, 2]);
        assert_eq!(
            pixels,
            [[0, 0, 7, 10], [1, 0, 7, 11], [0, 1, 7, 12], [1, 1, 7, 13]]
        );
        let config = CanvasConfig::parse("mode=1; w=1; l=1; rgba='0xff400080'").unwrap();
        let CanvasFrame::Pixels { pixels, .. } = config.render(0, 0).unwrap() else {
            panic!("expected pixels");
        };
        assert_eq!(pixels, [[255, 64, 0, 128]]);
        for (color, expected) in [
            ("0xff400000", [255, 64, 0, 0]),
            ("0xff4000ff", [255, 64, 0, 255]),
            ("0x1_ff40_0080", [255, 64, 0, 128]),
            ("-1", [255, 255, 255, 255]),
        ] {
            let config = CanvasConfig::parse(&format!("mode=1; w=1; l=1; rgba='{color}'")).unwrap();
            let CanvasFrame::Pixels { pixels, .. } = config.render(0, 0).unwrap() else {
                panic!("expected pixels");
            };
            assert_eq!(pixels, [expected]);
        }
    }

    #[test]
    fn character_mode_preserves_console_mapping_and_accepts_coordinates() {
        let source = "w=2; l=2; b='t'; f='x+y+frame'; c='text[(x+y)%len(text)]'; text='AB'";
        let config = CanvasConfig::parse(source).unwrap();
        assert_eq!(config.mode(), Mode::Characters);
        let CanvasFrame::Characters { size, cells } = config.render(254, 3).unwrap() else {
            panic!("expected characters");
        };
        assert_eq!(size, [2, 2]);
        assert_eq!(
            cells.iter().map(|cell| cell.character).collect::<String>(),
            "ABBA"
        );
        assert_eq!(
            cells.iter().map(|cell| cell.background).collect::<Vec<_>>(),
            [254, 255, 0, 1]
        );
        assert_eq!(
            cells.iter().map(|cell| cell.foreground).collect::<Vec<_>>(),
            [3, 4, 4, 5]
        );
        let console = Config::parse(crate::config::STARTER).unwrap();
        let graphical = CanvasConfig::parse(crate::config::STARTER).unwrap();
        let CanvasFrame::Characters { cells, .. } = graphical.render(0, 0).unwrap() else {
            panic!("expected characters");
        };
        for (t, cell) in cells.iter().enumerate() {
            assert_eq!(*cell, console.cell(t as i64).unwrap());
        }
    }

    #[test]
    fn failed_frames_report_the_sample_and_can_recover() {
        let config = CanvasConfig::parse("mode=1; w=2; l=2; rgba='255//(frame-y)'").unwrap();
        let error = config.render(0, 1).err().unwrap();
        assert!(
            error.contains("rgba at t=2") && error.contains("x=0, y=1, frame=1"),
            "{error}"
        );
        assert!(config.render(4, 2).is_ok());
        let config = CanvasConfig::parse("w=2; l=1; c=\"'A'[x]\"").unwrap();
        assert!(config.render(0, 0).err().unwrap().contains("c at t=1"));
    }

    #[test]
    fn rejects_invalid_modes_dimensions_types_and_reserved_names() {
        for source in [
            "mode=2",
            "mode=-1",
            "mode='1'",
            "mode=1; w=0",
            "mode=1; l=513",
            "rgba=255",
            "mode=1; rgba=\"'A'\"",
            "mode=1; rgba='missing'",
            "x=1",
            "y=1",
            "frame=1",
        ] {
            assert!(CanvasConfig::parse(source).is_err(), "{source}");
        }
        assert!(CanvasConfig::parse(STARTER).unwrap().render(0, 0).is_ok());
        assert!(Config::parse("x=12; y=3; frame=5; b='x+y+frame'").is_ok());
    }

    #[test]
    fn palette_uses_the_xterm_cube_and_grayscale() {
        assert_eq!(ansi_color(16), [0, 0, 0]);
        assert_eq!(ansi_color(21), [0, 0, 255]);
        assert_eq!(ansi_color(231), [255, 255, 255]);
        assert_eq!(ansi_color(232), [8, 8, 8]);
        assert_eq!(ansi_color(255), [238, 238, 238]);
    }
}

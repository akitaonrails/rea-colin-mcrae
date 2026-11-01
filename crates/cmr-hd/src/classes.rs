//! Which images to upscale and how, by where they live in the game folder.
//! The models behave differently (docs/UPSCALING.md): the general model
//! keeps photos and painted art but invents texture on fine patterns; the
//! artwork model gives the cleanest edges and text but flattens texture.
//! Sprites are keyed: pure black is transparent.

/// How an image is upscaled.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Class {
    /// Full-screen pictures and painted backdrops (menus, loading, legal
    /// screens, car and country pictures): the general model, opaque.
    Picture,
    /// Front-end interface pieces (buttons, sliders, panels, flags, logos):
    /// the artwork model, black transparent.
    Interface,
    /// The race HUD (dials, pace-note arrows, positions): the artwork model,
    /// black transparent.
    Hud,
    /// The stages' sky and distant scenery (the bitmaps inside each
    /// `*.hzn`): redrawn elsewhere (`tools/hd-redraw/sky.py`) and imported;
    /// drawn as 3D textures, opaque.
    Sky,
    /// The cars' textures (the bitmaps inside `Objects/Cars/**/*.c3d`):
    /// redrawn elsewhere (`tools/hd-redraw/redraw.py`) and imported; drawn as
    /// 3D textures, 8-bit with pure black transparent, like the HUD's.
    Car,
    /// Not upscaled by this tool: 3D textures and effects (smoke, grass,
    /// snow; these are tiled or drawn as textures, see docs/UPSCALING.md).
    Skip,
}

impl Class {
    /// The Real-ESRGAN model for the class.
    pub fn model(self) -> Option<&'static str> {
        match self {
            Class::Picture => Some("realesrgan-x4plus"),
            Class::Interface | Class::Hud => Some("realesrgan-x4plus-anime"),
            Class::Sky | Class::Car | Class::Skip => None,
        }
    }

    /// Whether the game loads the HD version (F27: the race HUD's textures
    /// and the front end's images).
    pub fn runtime(self) -> bool {
        self != Class::Skip
    }

    /// Whether an image of this class with `colours` colours is flat art,
    /// redrawn with smooth outlines (the HUD always; interface pieces of
    /// few colours: logos, flags, icons) rather than upscaled.
    pub fn flat(self, colours: usize) -> bool {
        self == Class::Hud || (self == Class::Interface && colours <= 32)
    }

    /// Whether the game draws it as a texture (the race HUD), kept as 8-bit
    /// palette indices; the rest are 2D surfaces.
    pub fn texture(self) -> bool {
        matches!(self, Class::Hud | Class::Car)
    }

    /// Whether pure black marks transparency (the game's colour key 0).
    pub fn keyed(self) -> bool {
        matches!(self, Class::Interface | Class::Hud | Class::Car)
    }

    /// The class named in a manifest (unknown names are skipped).
    pub fn from_name(name: &str) -> Class {
        [
            Class::Picture,
            Class::Interface,
            Class::Hud,
            Class::Sky,
            Class::Car,
        ]
        .into_iter()
        .find(|c| c.name() == name)
        .unwrap_or(Class::Skip)
    }

    pub fn name(self) -> &'static str {
        match self {
            Class::Picture => "picture",
            Class::Interface => "interface",
            Class::Hud => "hud",
            Class::Sky => "sky",
            Class::Car => "car",
            Class::Skip => "skip",
        }
    }
}

/// Sheets outside `osdnew` that the race's own screens draw as textures:
/// the manufacturers' badges and the countries' flags (stage results).
const RACE_SHEETS: [&str; 2] = ["game/images/logos2.bmp", "game/images/cuntlogo.bmp"];

/// The class of an image from its path inside the game folder (either
/// separator, any case).
pub fn classify(path: &str) -> Class {
    let p = path.replace('\\', "/").to_ascii_lowercase();
    let p = p.trim_start_matches("./");
    let picture_dirs = [
        "new_fe/images/backdrop/",
        "new_fe/images/cars/",
        "new_fe/images/country/",
        "new_fe/images/thumbs/",
        "new_fe/images/load/",
        "game/welcome/",
        "game/images/loading/",
    ];
    if p.contains(".hzn/") {
        Class::Sky
    } else if p.starts_with("game/objects/cars/") && p.contains(".c3d/") {
        Class::Car
    } else if picture_dirs.iter().any(|d| p.starts_with(d))
        || (p.starts_with("ini/") && p.ends_with(".bmp") && !p.contains("font"))
    {
        Class::Picture
    } else if p.starts_with("game/images/osdnew/") || RACE_SHEETS.contains(&p) {
        Class::Hud
    } else if p.starts_with("new_fe/images/") || p.starts_with("game/setrep/") {
        Class::Interface
    } else {
        Class::Skip
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn classes_by_folder() {
        assert_eq!(classify(r"New_Fe\Images\Backdrop\Main.bmp"), Class::Picture);
        assert_eq!(classify("./ini/English/legal.bmp"), Class::Picture);
        assert_eq!(classify("ini/loading.bmp"), Class::Picture);
        assert_eq!(classify("ini/cdFONT.BMP"), Class::Skip, "a font sheet");
        assert_eq!(
            classify("Game/Images/osdnew/high/common/ARRWS.BMP"),
            Class::Hud
        );
        assert_eq!(
            classify("New_Fe/Images/Button/Done_in.bmp"),
            Class::Interface
        );
        assert_eq!(classify("Game/SetRep/L_ARROW.BMP"), Class::Interface);
        assert_eq!(
            classify("Game/Images/Dust/Dust0.bmp"),
            Class::Skip,
            "an effect texture"
        );
        assert_eq!(classify("Game/Images/CLOTA1.BMP"), Class::Skip);
        assert_eq!(classify("Game/NZ/1/nz1.hzn/1.bmp"), Class::Sky);
        assert_eq!(classify("Game/Objects/Cars/SubDay.c3d/2.bmp"), Class::Car);
        assert_eq!(
            classify("Game/NZ/1/NZ1.c3d/0.bmp"),
            Class::Skip,
            "a stage model"
        );
        assert_eq!(
            classify("Game/Images/LOGOS2.BMP"),
            Class::Hud,
            "results badges"
        );
    }

    #[test]
    fn models_and_keys() {
        assert_eq!(Class::Picture.model(), Some("realesrgan-x4plus"));
        assert!(!Class::Picture.keyed());
        assert_eq!(Class::Hud.model(), Some("realesrgan-x4plus-anime"));
        assert!(Class::Hud.keyed() && Class::Interface.keyed());
        assert_eq!(Class::Skip.model(), None);
        for c in [
            Class::Picture,
            Class::Interface,
            Class::Hud,
            Class::Sky,
            Class::Car,
            Class::Skip,
        ] {
            assert_eq!(Class::from_name(c.name()), c);
        }
        assert_eq!(Class::from_name("unknown"), Class::Skip);
    }
}

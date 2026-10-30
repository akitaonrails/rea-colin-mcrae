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
            Class::Skip => None,
        }
    }

    /// Whether the game can load the HD version (F27 swaps texture loads;
    /// the race HUD is drawn with textures, the menus aren't yet).
    pub fn runtime(self) -> bool {
        self == Class::Hud
    }

    /// Whether the art is flat colours (`lock::exact`), not shaded
    /// (`lock::back_project`).
    pub fn flat(self) -> bool {
        self == Class::Hud
    }

    /// Whether pure black marks transparency (the game's colour key 0).
    pub fn keyed(self) -> bool {
        matches!(self, Class::Interface | Class::Hud)
    }

    /// The class named in a manifest (unknown names are skipped).
    pub fn from_name(name: &str) -> Class {
        [Class::Picture, Class::Interface, Class::Hud]
            .into_iter()
            .find(|c| c.name() == name)
            .unwrap_or(Class::Skip)
    }

    pub fn name(self) -> &'static str {
        match self {
            Class::Picture => "picture",
            Class::Interface => "interface",
            Class::Hud => "hud",
            Class::Skip => "skip",
        }
    }
}

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
    if picture_dirs.iter().any(|d| p.starts_with(d))
        || (p.starts_with("ini/") && p.ends_with(".bmp") && !p.contains("font"))
    {
        Class::Picture
    } else if p.starts_with("game/images/osdnew/") {
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
    }

    #[test]
    fn models_and_keys() {
        assert_eq!(Class::Picture.model(), Some("realesrgan-x4plus"));
        assert!(!Class::Picture.keyed());
        assert_eq!(Class::Hud.model(), Some("realesrgan-x4plus-anime"));
        assert!(Class::Hud.keyed() && Class::Interface.keyed());
        assert_eq!(Class::Skip.model(), None);
        for c in [Class::Picture, Class::Interface, Class::Hud, Class::Skip] {
            assert_eq!(Class::from_name(c.name()), c);
        }
        assert_eq!(Class::from_name("unknown"), Class::Skip);
    }
}

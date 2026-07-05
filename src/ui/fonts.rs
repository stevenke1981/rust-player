use egui::FontFamily;

/// Load system CJK fonts so Traditional/Simplified Chinese and Japanese render correctly.
pub fn setup_fonts(ctx: &egui::Context) {
    let Some(data) = load_system_cjk_font() else {
        log::warn!("no CJK system font found; CJK UI text may show missing glyphs");
        return;
    };

    let mut fonts = egui::FontDefinitions::default();
    fonts
        .font_data
        .insert("cjk".to_owned(), egui::FontData::from_owned(data).into());

    fonts
        .families
        .entry(FontFamily::Proportional)
        .or_default()
        .insert(0, "cjk".to_owned());

    fonts
        .families
        .entry(FontFamily::Monospace)
        .or_default()
        .insert(0, "cjk".to_owned());

    ctx.set_fonts(fonts);
    log::info!("loaded system CJK font for UI");
}

fn load_system_cjk_font() -> Option<Vec<u8>> {
    #[cfg(windows)]
    {
        const CANDIDATES: &[&str] = &[
            r"C:\Windows\Fonts\msjh.ttc",
            r"C:\Windows\Fonts\msjhbd.ttc",
            r"C:\Windows\Fonts\msyh.ttc",
            r"C:\Windows\Fonts\msyhbd.ttc",
            r"C:\Windows\Fonts\mingliu.ttc",
            r"C:\Windows\Fonts\msgothic.ttc",
            r"C:\Windows\Fonts\meiryo.ttc",
        ];
        for path in CANDIDATES {
            if let Ok(data) = std::fs::read(path) {
                log::debug!("using CJK font: {path}");
                return Some(data);
            }
        }
    }

    #[cfg(target_os = "macos")]
    {
        const CANDIDATES: &[&str] = &[
            "/System/Library/Fonts/PingFang.ttc",
            "/System/Library/Fonts/STHeiti Light.ttc",
            "/Library/Fonts/Arial Unicode.ttf",
        ];
        for path in CANDIDATES {
            if let Ok(data) = std::fs::read(path) {
                return Some(data);
            }
        }
    }

    #[cfg(target_os = "linux")]
    {
        const CANDIDATES: &[&str] = &[
            "/usr/share/fonts/opentype/noto/NotoSansCJK-Regular.ttc",
            "/usr/share/fonts/truetype/noto/NotoSansCJK-Regular.ttc",
            "/usr/share/fonts/noto-cjk/NotoSansCJK-Regular.ttc",
        ];
        for path in CANDIDATES {
            if let Ok(data) = std::fs::read(path) {
                return Some(data);
            }
        }
    }

    None
}
//! UI / CLI localization. Default: Traditional Chinese (zh-Hant).

#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum Language {
    #[default]
    ZhHant,
    ZhHans,
    En,
    Ja,
}

impl Language {
    pub const ALL: [Language; 4] = [
        Language::ZhHant,
        Language::ZhHans,
        Language::En,
        Language::Ja,
    ];

    pub fn parse(s: &str) -> Option<Self> {
        match s.to_ascii_lowercase().as_str() {
            "zh-hant" | "zh-tw" | "zh-hk" | "zh-mo" | "zh-hk-tw" => Some(Self::ZhHant),
            "zh-hans" | "zh-cn" | "zh-sg" => Some(Self::ZhHans),
            "zh" => Some(Self::ZhHant),
            "en" | "en-us" | "en-gb" => Some(Self::En),
            "ja" | "ja-jp" => Some(Self::Ja),
            _ => None,
        }
    }

    pub fn code(self) -> &'static str {
        match self {
            Self::ZhHant => "zh-Hant",
            Self::ZhHans => "zh-Hans",
            Self::En => "en",
            Self::Ja => "ja",
        }
    }

    pub fn native_name(self) -> &'static str {
        match self {
            Self::ZhHant => "繁體中文",
            Self::ZhHans => "简体中文",
            Self::En => "English",
            Self::Ja => "日本語",
        }
    }
}

#[derive(Clone, Copy, Default)]
pub struct Locale {
    lang: Language,
}

impl Locale {
    pub fn new(lang: Language) -> Self {
        Self { lang }
    }

    pub fn language(self) -> Language {
        self.lang
    }

    pub fn set_language(&mut self, lang: Language) {
        self.lang = lang;
    }

    pub fn app_name(self) -> &'static str {
        "Rust Player"
    }

    pub fn no_media_open(self) -> &'static str {
        match self.lang {
            Language::ZhHant => "未開啟媒體檔案",
            Language::ZhHans => "未打开媒体文件",
            Language::En => "No media file open",
            Language::Ja => "メディアファイル未選択",
        }
    }

    pub fn open_file(self) -> &'static str {
        match self.lang {
            Language::ZhHant => "開啟檔案",
            Language::ZhHans => "打开文件",
            Language::En => "Open File",
            Language::Ja => "ファイルを開く",
        }
    }

    pub fn supported_formats(self) -> &'static str {
        "MP4 · M4A · MP3"
    }

    pub fn language_label(self) -> &'static str {
        match self.lang {
            Language::ZhHant => "語言",
            Language::ZhHans => "语言",
            Language::En => "Language",
            Language::Ja => "言語",
        }
    }

    pub fn idle_hint(self) -> String {
        let open = self.open_file();
        match self.lang {
            Language::ZhHant => format!("拖放檔案至視窗，或點擊「{open}」"),
            Language::ZhHans => format!("拖放文件到窗口，或点击「{open}」"),
            Language::En => format!("Drop a file onto the window, or click \"{open}\""),
            Language::Ja => format!("ファイルをウィンドウにドロップするか、「{open}」をクリック"),
        }
    }

    pub fn drop_title_idle(self) -> &'static str {
        match self.lang {
            Language::ZhHant => "拖放媒體檔案至此",
            Language::ZhHans => "拖放媒体文件到此处",
            Language::En => "Drop media file here",
            Language::Ja => "メディアファイルをここにドロップ",
        }
    }

    pub fn drop_title_active(self) -> &'static str {
        match self.lang {
            Language::ZhHant => "放開以開始播放",
            Language::ZhHans => "松开以开始播放",
            Language::En => "Release to play",
            Language::Ja => "離して再生開始",
        }
    }

    pub fn drop_subtitle_idle(self) -> &'static str {
        match self.lang {
            Language::ZhHant => "參考 VLC / ffplay — 拖曳檔案即可播放",
            Language::ZhHans => "参考 VLC / ffplay — 拖入文件即可播放",
            Language::En => "Like VLC / ffplay — drag a file to play",
            Language::Ja => "VLC / ffplay 風 — ドラッグで再生",
        }
    }

    pub fn drop_subtitle_active(self) -> &'static str {
        self.supported_formats()
    }

    pub fn unsupported_file_type(self, ext: &str) -> String {
        match self.lang {
            Language::ZhHant => format!("不支援的檔案類型：{ext}"),
            Language::ZhHans => format!("不支持的文件类型：{ext}"),
            Language::En => format!("Unsupported file type: {ext}"),
            Language::Ja => format!("未対応のファイル形式：{ext}"),
        }
    }

    pub fn unknown_extension(self) -> &'static str {
        match self.lang {
            Language::ZhHant => "未知",
            Language::ZhHans => "未知",
            Language::En => "unknown",
            Language::Ja => "不明",
        }
    }

    pub fn window_title(self, filename: &str) -> String {
        format!("{} — {filename}", self.app_name())
    }

    pub fn window_title_default(self) -> String {
        self.app_name().to_string()
    }

    pub fn file_dialog_title(self) -> &'static str {
        match self.lang {
            Language::ZhHant => "開啟媒體檔案",
            Language::ZhHans => "打开媒体文件",
            Language::En => "Open Media File",
            Language::Ja => "メディアファイルを開く",
        }
    }

    pub fn file_dialog_filter(self) -> &'static str {
        match self.lang {
            Language::ZhHant => "媒體檔案",
            Language::ZhHans => "媒体文件",
            Language::En => "Media Files",
            Language::Ja => "メディアファイル",
        }
    }

    pub fn virtual_audio_warning(self) -> &'static str {
        match self.lang {
            Language::ZhHant => "無音訊輸出裝置，已啟用虛擬音訊（僅同步時間軸）",
            Language::ZhHans => "无音频输出设备，已启用虚拟音频（仅同步时间轴）",
            Language::En => "No audio output device; using virtual audio (clock sync only)",
            Language::Ja => "音声出力デバイスなし。仮想音声で同期のみ",
        }
    }

    pub fn headless_requires_path(self) -> &'static str {
        match self.lang {
            Language::ZhHant => "無介面模式需要指定媒體檔案路徑",
            Language::ZhHans => "无界面模式需要指定媒体文件路径",
            Language::En => "Headless mode requires a media file path",
            Language::Ja => "ヘッドレスモードにはメディアファイルのパスが必要です",
        }
    }

    pub fn usage(self) -> String {
        let open = self.open_file();
        match self.lang {
            Language::ZhHant => format!(
                r#"rust-player — Rust 原生媒體播放器

用法：
  rust-player                              啟動播放器（拖放 / {open}）
  rust-player <檔案> [--no-ui] [--lang]    開啟媒體檔案
  rust-player audio <檔案> [--progress]    Phase 1：音訊播放
  rust-player decode <檔案> [--frames N]     Phase 2：視訊解碼
  rust-player render <檔案>                Phase 3：GPU 渲染
  rust-player <URL>                        Phase 8：HTTP / HLS 串流
  rust-player help

語言（--lang）：
  zh-Hant（預設）· zh-Hans · en · ja

快捷鍵：
  Space        播放 / 暫停
  ← / →        快轉 ±10 秒
  Ctrl+O       {open}
"#
            ),
            Language::ZhHans => format!(
                r#"rust-player — Rust 原生媒体播放器

用法：
  rust-player                              启动播放器（拖放 / {open}）
  rust-player <文件> [--no-ui] [--lang]    打开媒体文件
  rust-player audio <文件> [--progress]    Phase 1：音频播放
  rust-player decode <文件> [--frames N]     Phase 2：视频解码
  rust-player render <文件>                Phase 3：GPU 渲染
  rust-player help

语言（--lang）：
  zh-Hant · zh-Hans（默认繁体可用 zh-Hant）· en · ja

快捷键：
  Space        播放 / 暂停
  ← / →        快进 ±10 秒
  Ctrl+O       {open}
"#
            ),
            Language::En => format!(
                r#"rust-player — native Rust media player

Usage:
  rust-player                              Launch player (drag & drop / {open})
  rust-player <file> [--no-ui] [--lang]     Open media file
  rust-player audio <file> [--progress]     Phase 1: audio playback
  rust-player decode <file> [--frames N]    Phase 2: video decode
  rust-player render <file>                 Phase 3: GPU render
  rust-player help

Languages (--lang):
  zh-Hant (default) · zh-Hans · en · ja

Shortcuts:
  Space        Play / Pause
  ← / →        Seek ±10s
  Ctrl+O       {open}
"#
            ),
            Language::Ja => format!(
                r#"rust-player — Rust ネイティブメディアプレーヤー

使い方：
  rust-player                              プレーヤー起動（ドロップ / {open}）
  rust-player <ファイル> [--no-ui] [--lang]  メディアを開く
  rust-player audio <ファイル> [--progress]  Phase 1：音声再生
  rust-player decode <ファイル> [--frames N] Phase 2：動画デコード
  rust-player render <ファイル>              Phase 3：GPU レンダリング
  rust-player help

言語（--lang）：
  zh-Hant（既定）· zh-Hans · en · ja

ショートカット：
  Space        再生 / 一時停止
  ← / →        ±10 秒シーク
  Ctrl+O       {open}
"#
            ),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_is_zh_hant() {
        assert_eq!(Language::default(), Language::ZhHant);
        assert_eq!(Locale::default().language(), Language::ZhHant);
    }

    #[test]
    fn parse_language_codes() {
        assert_eq!(Language::parse("zh-Hant"), Some(Language::ZhHant));
        assert_eq!(Language::parse("zh-tw"), Some(Language::ZhHant));
        assert_eq!(Language::parse("zh"), Some(Language::ZhHant));
        assert_eq!(Language::parse("zh-Hans"), Some(Language::ZhHans));
        assert_eq!(Language::parse("en"), Some(Language::En));
        assert_eq!(Language::parse("ja"), Some(Language::Ja));
        assert!(Language::parse("fr").is_none());
    }

    #[test]
    fn zh_hant_strings() {
        let loc = Locale::new(Language::ZhHant);
        assert_eq!(loc.no_media_open(), "未開啟媒體檔案");
        assert!(loc.unsupported_file_type("txt").contains("不支援"));
    }

    #[test]
    fn en_strings() {
        let loc = Locale::new(Language::En);
        assert_eq!(loc.no_media_open(), "No media file open");
        assert!(loc.unsupported_file_type("txt").contains("Unsupported"));
    }
}
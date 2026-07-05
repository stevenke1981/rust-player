# Rust 原生播放器 — 實作計畫

## 目標

以純 Rust 生態建立跨平台媒體播放器，分四個階段漸進交付，每階段可獨立驗收。

| Phase | 名稱 | 交付物 |
|-------|------|--------|
| 1 | 音訊先行 | MP3/AAC 流暢播放 + 精確進度 |
| 2 | 視訊解碼 | Demux + AV1 解碼為 YUV + PTS log |
| 3 | GPU 渲染 | wgpu 畫布 + YUV→RGB Shader |
| 4 | 同步與 UI | Audio Master Clock + 進度條/暫停/快轉 |

## 技術選型

| 層級 | Crate | 理由 |
|------|-------|------|
| 音訊解碼 | `symphonia` | 純 Rust，支援 MP3/AAC/FLAC 等 |
| 音訊輸出 | `cpal` | 跨平台原生音訊 I/O |
| 容器 Demux | `mp4` + `symphonia` | MP4/MKV 封包提取；音訊仍走 symphonia |
| 視訊解碼 | `rav1d` | dav1d 的 Rust 安全移植，純 Rust AV1 解碼 |
| GPU | `wgpu` + `winit` | 跨平台 Vulkan/Metal/DX12 |
| UI | `egui` + `egui-wgpu` | 輕量即時 UI，進度條與控制項 |
| 同步 | 自研 Audio Master Clock | 音訊為主時鐘，視訊追趕/丟幀 |

## 架構概覽

```
┌─────────────┐     ┌──────────────┐     ┌─────────────┐
│  Demux      │────▶│ Video Decoder│────▶│ Frame Queue │
│  (mp4)      │     │  (rav1d)     │     │  (YUV+PTS)  │
└─────────────┘     └──────────────┘     └──────┬──────┘
                                                  │
┌─────────────┐     ┌──────────────┐              ▼
│  symphonia  │────▶│  cpal output │     ┌─────────────┐
│  decoder    │     │  + clock     │────▶│ wgpu render │
└─────────────┘     └──────────────┘     │ YUV→RGB     │
                           ▲              └─────────────┘
                           │                     │
                    Audio Master Clock           ▼
                           │              ┌─────────────┐
                           └──────────────│  egui UI    │
                                          └─────────────┘
```

## 目錄結構

```
rust-player/
├── Cargo.toml
├── plan.md
├── spec.md
├── todos.md
├── test.md
├── final.md
├── assets/              # 測試用媒體（gitignore）
└── src/
    ├── main.rs          # CLI 入口 + 視窗啟動
    ├── lib.rs
    ├── error.rs
    ├── audio/
    │   ├── mod.rs
    │   ├── decoder.rs   # symphonia 解碼
    │   ├── output.rs    # cpal 播放
    │   └── clock.rs     # 播放時鐘與進度
    ├── video/
    │   ├── mod.rs
    │   ├── demux.rs     # MP4 封包提取
    │   └── decoder.rs   # rav1d AV1 解碼
    ├── render/
    │   ├── mod.rs
    │   ├── pipeline.rs  # wgpu 管線
    │   └── yuv.wgsl     # YUV420→RGB shader
    ├── sync/
    │   └── mod.rs       # A/V 同步演算法
    └── ui/
        └── mod.rs       # egui 控制面板
```

## 階段實作順序

### Phase 1 — 音訊先行（約 1–2 天）

1. 初始化 Cargo workspace / binary crate
2. 實作 `audio::decoder`：symphonia 開啟檔案、選音軌、解碼為 f32 PCM
3. 實作 `audio::output`：cpal 建立 stream，ring buffer 餵資料
4. 實作 `audio::clock`：以已播放 sample 數 / sample_rate 計算 `position_secs`
5. CLI：`rust-player play <file.mp3>` 播放並每秒印出進度

**驗收門檻**：MP3 與 AAC 連續播放 ≥30s 無爆音/卡頓；進度誤差 < 50ms。

### Phase 2 — 視訊解碼（約 2–3 天）

1. 實作 `video::demux`：mp4 crate 讀取 video track，逐 sample 輸出 `(pts, data)`
2. 實作 `video::decoder`：rav1d 初始化、送 OBU/Frame、輸出 YUV420
3. 整合：開啟含 AV1 的 MP4，解碼前 N 幀並 `log::info!` 印 PTS 與解析度
4. 音訊軌仍由 symphonia 處理（同一檔案雙軌）

**驗收門檻**：成功解碼 AV1 MP4；每幀 PTS 單調遞增；log 可見 YUV 平面尺寸。

### Phase 3 — GPU 渲染（約 2–3 天）

1. winit 建立視窗；wgpu 建立 swapchain
2. 上傳 Y/U/V 三平面為 texture；編寫 WGSL BT.601/BT.709 YUV→RGB
3. 從 frame queue 取最新幀繪製至畫布
4. 視窗標題顯示解析度與當前 PTS

**驗收門檻**：視窗可見正確色彩之 AV1 影片；無明顯撕裂；60fps UI loop。

### Phase 4 — 同步與 UI（約 2–3 天）

1. `sync` 模組：Audio Master Clock
   - `audio_pts = samples_played / sample_rate`
   - 視訊幀 `|frame_pts - audio_pts| > threshold` 時丟幀或等待
2. egui 疊加層：進度條、播放/暫停、±10s 快轉、時間顯示
3. Seek：symphonia seek + demux seek（關鍵幀對齊）
4. 狀態機：`Playing | Paused | Seeking`

**驗收門檻**：A/V 唇形同步誤差 < 80ms；UI 操作即時響應；seek 後 500ms 內恢復播放。

## 風險與緩解

| 風險 | 緩解 |
|------|------|
| rav1d 編譯時間長 | 啟用 `default-features = false`，僅 arm64/x86_64 |
| cpal buffer underrun | 預填 100ms ring buffer；callback 內零分配 |
| wgpu YUV 格式 | 手動平面紋理 + shader 轉換，避免依賴硬體 YUV |
| MP4 AV1 封裝差異 | 支援 `av01` + `dav1` codec fourcc；OBU 組裝 |

## 依賴版本（鎖定策略）

- Rust edition 2021，MSRV 1.75+
- 所有 crate 使用 crates.io 穩定版，Cargo.lock 納入版控

## 完成定義

四階段 test.md 全部通過後，撰寫 `final.md` 記錄實際結果、已知限制與後續路線圖。

---

## Phase 5 — 體驗強化（進行中）

在 MVP 四階段完成後，強化播放器可用性與品質。

| 項目 | 交付物 |
|------|--------|
| 鍵盤快捷鍵 | Space 播放/暫停、←/→ ±10s |
| 音量控制 | UI 滑桿 + cpal 輸出增益 |
| 程式品質 | 修復 wgpu 棄用警告、clippy |
| 壓力測試 | T6 seek / 暫停播放自動化 |

**驗收門檻**：快捷鍵與音量即時生效；`cargo test` 含 T6 壓力案例通過。

### 後續路線圖（Phase 6+）

| Phase | 方向 |
|-------|------|
| 6 | H.264/H.265 解碼支援 |
| 7 | 多執行緒解碼與 1080p 效能優化 |
| 8 | 硬體加速、字幕、串流 |
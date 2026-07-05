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

---

## CBM 專案檢視與優化改善計畫（2026-07-05）

### 範圍與完成定義

**目標**：依據 CBM 對 `rust-player` 的索引與本地源碼檢視，優先改善目前播放影片黑畫面問題，並規劃可驗證的品質、架構、效能與測試優化。

**不在本輪直接變更範圍**：不新增外部大型媒體框架、不重寫播放器、不刪除既有功能、不變更使用者資料或 Git 歷史。

**完成定義**：

1. 黑畫面問題可被定位：UI 或 log 可指出是未收到 frame、解碼失敗、sync 等待、surface error 或 unsupported codec。
2. 常見 AV1 MP4 測試檔起播 2 秒內顯示第一幀，否則顯示可讀錯誤。
3. seek 後不永久黑畫面；新幀未 ready 前保留上一幀或 loading overlay。
4. 新增/更新單元測試與手動 GUI 驗收步驟。

### CBM 發現摘要

| 發現 | 證據 | 風險 |
|------|------|------|
| `src/player.rs` 是最高熱點 | CBM query：40 個 function/class symbols | App lifecycle、render、UI、player state 混在同檔，維護風險高 |
| render 無 frame 時清黑 | `RenderPipeline::render()` 與 `PlayerApp::redraw()` clear BLACK，只有 `bind_group` 時 draw | 解碼尚未出幀或錯誤被吞時，使用者只看到黑畫面 |
| `MediaPlayer::tick()` 可能 clone 大型 frame | `last_video_frame: Option<DecodedFrame>` 並 clone | 4K/高 fps 下 CPU/記憶體壓力高 |
| worker 錯誤多在 log | `video/worker.rs` demux/decode 錯誤未回傳 UI | 黑畫面不可診斷 |
| codec 支援文件落差 | 文件說 H.264/H.265 非目標，但程式有 h264/h265 decoder | 驗收與使用者預期不一致 |
| audio output 只支援 F32 output format | `AudioOutput::new()` 對非 F32 回 `unsupported sample format` | 部分 Windows 音訊裝置可能進 virtual sink，影響 A/V clock 行為 |

### Phase 6 — 黑畫面修復（最高優先）

1. **建立 playback/render 診斷狀態**
   - 新增 `VideoPlaybackStatus`：`demux_packets`、`decoded_frames`、`uploaded_frames`、`last_frame_pts`、`last_error`、`waiting_reason`。
   - `VideoDecodeWorker` 透過 status channel 回報 demux/decode/init/seek 狀態。
   - UI 顯示「載入中 / 等待首幀 / 解碼失敗 / 不支援 codec」。

2. **修正首幀與 seek bootstrap**
   - 起播時：收到第一幀立即可顯示，不因 audio clock 起始誤差永久等待。
   - seek 時：清空 queue 後保留上一幀直到新幀 ready，或顯示 semi-transparent loading overlay。
   - `AvSync` 增加 `pop_frame_for_display_with_policy(Startup|Normal|Seeking)` 或簡化為 `allow_bootstrap_frame`。

3. **Render 層避免靜默黑畫面**
   - `RenderPipeline` 增加 `has_frame()`、`uploaded_frame_count()`。
   - render/composite pass 若無 frame，UI 必須覆蓋提示。
   - `get_current_texture()` 不再 `expect("surface texture")`，改為處理 `Lost/Outdated/Timeout/OutOfMemory`。

4. **Frame validation**
   - 上傳前驗證 plane 長度與 UV 尺寸，錯誤時阻止上傳並回報 UI。
   - 支援奇數寬高時使用 `(width + 1) / 2`、`(height + 1) / 2` 的 UV 尺寸策略。

5. **優先驗證素材**
   - AV1 MP4（既有主路徑）
   - H.264 MP4（實驗路徑）
   - video-only MP4（virtual audio clock）
   - 有音訊但無可解碼視訊 / unsupported codec（錯誤可見）

### Phase 7 — 架構與效能改善

1. 拆分 `src/player.rs`
   - `player/app.rs`：winit lifecycle
   - `player/compositor.rs`：video pass + egui pass
   - `player/media.rs`：`MediaPlayer` 狀態與控制
   - `player/status.rs`：診斷資料模型
2. 減少 frame clone
   - `DecodedFrame` 改為 `Arc<DecodedFrame>` 或 frame buffer handle。
   - `AvSync` queue 與 `last_video_frame` 儲存共享指標。
3. audio output sample format 支援
   - 支援 I16/U16 output format 或用 cpal conversion，降低 virtual sink fallback 機率。
4. 色彩與尺寸策略
   - 根據 codec metadata 選 BT.601/BT.709，加入 limited/full range 設定。
   - render shader 增加 uniform color matrix。
5. 測試與 CI
   - `cargo test` 必跑。
   - 若有測試素材，加入 `decode --frames 1` 與 `--no-ui` smoke test。

### 建議實作順序

| 順序 | 任務 | 驗收 |
|------|------|------|
| 1 | 加入 status model 與 UI overlay | 黑畫面時顯示等待/錯誤原因 |
| 2 | 修正 sync startup/seek bootstrap | 起播/seek 不永久等待早期幀 |
| 3 | Render `has_frame` 與 surface error handling | 無 frame 不再靜默黑畫面；surface lost 可恢復 |
| 4 | Frame validation + 奇數尺寸 | plane mismatch 有明確錯誤 |
| 5 | 補測試與文件 | `cargo test` 通過，test.md T7/T8 可執行 |
| 6 | 拆分 `player.rs` 與減少 frame clone | 無行為回歸，效能風險降低 |

---

## Phase 9 — H.264 播放修復計畫（CBM 第二輪檢視 2026-07-06）

### 範圍與完成定義

**目標**：修復 H.264 MP4「無法正常播放」（decoded=7/241、大量 openh264 `Native:16`、黑畫面/卡頓），並依 CBM 證據提出可驗證的優化。

**在範圍**：`src/video/{nal,h264,h265,worker,frame}.rs`、`src/render/pipeline.rs`、`src/sync/mod.rs` 的最小必要修改；五份文件更新。

**不在範圍**：不新增大型媒體框架、不重寫播放器、不改 Git 歷史、不動使用者資料、不引入硬體解碼。

**完成定義**：
1. H.264 MP4 起播後 `decoded_frames / demuxed_packets ≥ 95%`。
2. 奇數寬高影片可上傳顯示，無 frame validation warn。
3. 短片尾幀不遺失（EOF flush 生效）。
4. `cargo build`、`cargo test`、`cargo clippy -- -D warnings` 全綠。

### 根因對應任務（詳見 spec.md §10）

| ID | 根因 | 任務 | 檔案 | 優先 |
|----|------|------|------|------|
| P9-1 | R1 extradata 空→SPS/PPS 未送 | extradata_to_annex_b fallback + hex log + config_sent 修正 | `src/video/nal.rs`、`src/video/h264.rs` | P0 |
| P9-2 | R2 UV 尺寸不一致 | `pack_i420` 改用 `div_ceil(2)` 與 render 對齊 | `src/video/frame.rs` | P0 |
| P9-3 | R3/R6 無 flush、EOF 空轉 | decode_loop EOF flush + end-of-stream 標記 | `src/video/worker.rs` | P1 |
| P9-4 | R4 last_frame_pts 未更新 | 送 frame 時更新 pts | `src/video/worker.rs` | P1 |
| P9-5 | R5 抗損性 | openh264 error concealment 設定 | `src/video/h264.rs` | P2 |

### 建議實作順序

| 順序 | 任務 | 驗收 |
|------|------|------|
| 1 | P9-1 extradata 修復（已部分實作 fallback，待實測） | H.264 起播 decoded≈demuxed，log 無 `extradata_annex_b=0` 致命路徑 |
| 2 | P9-2 UV 尺寸一致 | 奇數尺寸幀無 validation warn |
| 3 | P9-3 EOF flush | 短片尾幀完整、CPU 空轉消除 |
| 4 | P9-4 last_frame_pts | UI 進度診斷正確 |
| 5 | P9-5 抗損 + 測試素材 | 壞幀不連鎖；smoke test 可跑 |

### 風險與緩解

| 風險 | 緩解 |
|------|------|
| 無測試素材無法自動驗收 | 產生極小合成 H.264/AV1 clip 進 `assets/`（gitignore 保留樣本或 CI 生成） |
| fallback AVCC 誤解析 HVCC | 先試標準路徑、再 HVCC、最後才 fallback，且皆有 bounds check |
| flush 時序不當造成 openh264 錯誤 | 僅於 EOF 呼叫一次 flush，參考 openh264 `flush_remaining()` |

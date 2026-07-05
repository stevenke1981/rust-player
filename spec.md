# Rust 原生播放器 — 技術規格

## 1. 系統概述

本專案為命令列 + 視窗化媒體播放器，核心能力為音訊播放、視訊解碼、GPU 呈現與 A/V 同步。

### 1.1 支援格式（Phase 1–4）

| 類型 | 格式 | 解碼器 |
|------|------|--------|
| 音訊 | MP3, AAC (ADTS/MP4) | symphonia |
| 視訊 | AV1 in MP4 (av01/dav1) | rav1d |
| 容器 | MP4 (.mp4, .m4a) | mp4 + symphonia |

### 1.2 非目標（本版）

- H.264/H.265 解碼
- 串流（HLS/DASH）
- 硬體加速解碼
- 字幕渲染

---

## 2. Phase 1 — 音訊子系統

### 2.1 模組：`audio::decoder`

**職責**：從檔案解碼音訊為統一 PCM 格式。

```rust
pub struct AudioDecoder {
    // symphonia FormatReader + Decoder
}

impl AudioDecoder {
    pub fn open(path: &Path) -> Result<Self>;
    pub fn sample_rate(&self) -> u32;
    pub fn channels(&self) -> u16;
    pub fn duration_secs(&self) -> Option<f64>;
    pub fn decode_next(&mut self) -> Result<Option<AudioBuffer>>;
    pub fn seek(&mut self, position_secs: f64) -> Result<()>;
}

pub struct AudioBuffer {
    pub samples: Vec<f32>,      // interleaved
    pub channels: u16,
    pub sample_rate: u32,
    pub pts_secs: f64,          // 此 buffer 起始時間戳
}
```

**規則**：
- 輸出統一為 f32 interleaved PCM
- 若來源為整數格式，轉換為 `sample / i16::MAX as f32` 或對應比例
- `pts_secs` 由累計 sample 數推算，精度 f64

### 2.2 模組：`audio::output`

**職責**：透過 cpal 輸出至預設裝置。

```rust
pub struct AudioOutput {
    // cpal Stream + ring buffer
}

impl AudioOutput {
    pub fn new(sample_rate: u32, channels: u16) -> Result<Self>;
    pub fn write(&mut self, samples: &[f32]) -> Result<()>;
    pub fn start(&mut self) -> Result<()>;
    pub fn pause(&mut self);
    pub fn resume(&mut self);
    pub fn clear(&mut self);
}
```

**Ring Buffer 規格**：
- 容量：至少 `sample_rate * channels * 0.5`（500ms）
- cpal callback 從 ring buffer 讀取；不足時輸出靜音
- 使用 `parking_lot::Mutex` 或 lock-free ring buffer

### 2.3 模組：`audio::clock`

**職責**：提供精確播放進度（Audio Master Clock 基礎）。

```rust
pub struct PlaybackClock {
    samples_played: AtomicU64,
    sample_rate: u32,
    paused_at: Option<f64>,
    is_paused: AtomicBool,
}

impl PlaybackClock {
    pub fn position_secs(&self) -> f64;
    pub fn on_samples_played(&self, count: u64);
    pub fn pause(&self);
    pub fn resume(&self);
    pub fn seek(&self, position_secs: f64);
    pub fn duration_secs(&self) -> Option<f64>;
}
```

**精度要求**：
- `position_secs()` 解析度：1/sample_rate 秒
- 與實際輸出誤差 < 50ms（不含系統音訊延遲）

### 2.4 Phase 1 CLI

```
rust-player audio <path> [--progress]
```

- 播放至結束或 Ctrl+C
- `--progress`：每 500ms 印出 `position / duration`

---

## 3. Phase 2 — 視訊子系統

### 3.1 模組：`video::demux`

```rust
pub struct VideoPacket {
    pub pts_secs: f64,
    pub dts_secs: f64,
    pub data: Vec<u8>,
    pub is_keyframe: bool,
}

pub struct Mp4Demuxer {
    // mp4::Mp4Reader
}

impl Mp4Demuxer {
    pub fn open(path: &Path) -> Result<Self>;
    pub fn video_codec(&self) -> VideoCodec;  // Av1
    pub fn next_packet(&mut self) -> Result<Option<VideoPacket>>;
    pub fn seek(&mut self, pts_secs: f64) -> Result<()>;
    pub fn timebase(&self) -> (u32, u32);     // timescale
}
```

**PTS 計算**：
```
pts_secs = (composition_offset + decode_timestamp) / timescale
```

### 3.2 模組：`video::decoder`

```rust
pub struct DecodedFrame {
    pub pts_secs: f64,
    pub width: u32,
    pub height: u32,
    pub y_plane: Vec<u8>,
    pub u_plane: Vec<u8>,
    pub v_plane: Vec<u8>,
    pub y_stride: usize,
    pub uv_stride: usize,
}

pub struct Av1Decoder {
    // rav1d::Context
}

impl Av1Decoder {
    pub fn new() -> Result<Self>;
    pub fn decode(&mut self, packet: &VideoPacket) -> Result<Vec<DecodedFrame>>;
    pub fn flush(&mut self) -> Result<Vec<DecodedFrame>>;
}
```

### 3.3 Phase 2 CLI

```
rust-player decode <path> [--frames N]
```

- 預設解碼前 30 幀
- 每幀 log：`PTS={pts:.3}s size={w}x{h} Y={y_len} U={u_len} V={v_len}`

---

## 4. Phase 3 — GPU 渲染

### 4.1 模組：`render::pipeline`

```rust
pub struct RenderPipeline {
    // wgpu Device, Queue, Surface
}

impl RenderPipeline {
    pub fn new(window: &Window) -> Result<Self>;
    pub fn resize(&mut self, width: u32, height: u32);
    pub fn upload_frame(&mut self, frame: &DecodedFrame);
    pub fn render(&mut self) -> Result<()>;
}
```

### 4.2 YUV→RGB Shader（WGSL）

- 輸入：三張 R8Unorm texture（Y full size, U/V half size）
- 輸出：sRGB BGRA8 swapchain
- 色彩矩陣：BT.709（預設），可透過 uniform 切換 BT.601
- 公式（BT.709）：
  ```
  R = Y + 1.5748 * V'
  G = Y - 0.1873 * U' - 0.4681 * V'
  B = Y + 1.8556 * U'
  ```
  其中 U', V' 為 (U-128)/255, (V-128)/255

### 4.3 Phase 3 執行模式

```
rust-player render <path>
```

- 開啟視窗，解碼並即時繪製（Phase 4 前無同步，盡快顯示）

---

## 5. Phase 4 — 同步與 UI

### 5.1 模組：`sync::AvSync`

```rust
pub struct AvSync {
    clock: Arc<PlaybackClock>,
    frame_queue: VecDeque<DecodedFrame>,
    sync_threshold_ms: f64,   // 預設 40ms
    max_queue_frames: usize,    // 預設 8
}

impl AvSync {
    pub fn push_frame(&mut self, frame: DecodedFrame);
    pub fn pop_frame_for_display(&mut self) -> Option<DecodedFrame>;
    // 若 frame.pts < audio_pts - threshold → 丟棄（落後）
    // 若 frame.pts > audio_pts + threshold → 等待（超前）
    // 若 queue > max → 丟棄最舊非關鍵幀
}
```

### 5.2 模組：`ui::PlayerUi`

**控制項**：
| 控制 | 行為 |
|------|------|
| 播放/暫停 | 切換 cpal stream + clock |
| 進度條拖曳 | seek 至對應時間 |
| -10s / +10s | `position ± 10`，clamp 至 [0, duration] |
| 時間標籤 | `current / total` 格式 `MM:SS.mmm` |

### 5.3 主迴圈

```
loop {
    winit event poll
    if playing {
        decode audio → cpal write
        decode video → sync.push_frame
        if let frame = sync.pop_frame_for_display() { render.upload(frame) }
        clock.on_samples_played(...)
    }
    render.render()
    ui.draw(egui)
}
```

### 5.4 Phase 4 CLI

```
rust-player <path>          # 完整播放器
rust-player <path> --no-ui  # 僅同步 log 模式（測試用）
```

---

## 6. 錯誤處理

```rust
#[derive(Debug, thiserror::Error)]
pub enum PlayerError {
    #[error("IO: {0}")]
    Io(#[from] std::io::Error),
    #[error("Audio decode: {0}")]
    AudioDecode(String),
    #[error("Video demux: {0}")]
    VideoDemux(String),
    #[error("Video decode: {0}")]
    VideoDecode(String),
    #[error("Render: {0}")]
    Render(String),
    #[error("Unsupported format: {0}")]
    Unsupported(String),
}
```

- 使用者可見錯誤以 `eprintln!` + exit code 1
- 內部以 `tracing` / `log` 記錄

---

## 7. 效能指標

| 指標 | 目標 |
|------|------|
| 音訊 callback CPU | < 5% 單核 |
| 1080p AV1 解碼 | ≥ 24fps（軟解） |
| UI 幀率 | ≥ 30fps |
| 記憶體（1080p） | < 200MB |

---

## 8. 建置與執行

```bash
cargo build --release
cargo run --release -- audio test.mp3 --progress
cargo run --release -- decode test_av1.mp4
cargo run --release -- render test_av1.mp4
cargo run --release -- test_av1.mp4
```

**系統需求**：Windows 10+、支援 Vulkan/DX12/Metal 的 GPU。

---

## 9. CBM 檢視後新增規格 — 黑畫面修復與品質強化（2026-07-05）

### 9.1 CBM 索引摘要

- 專案：`cbm+rust-player`
- 索引結果：38 files、319 symbols、878 edges
- 主要模組熱點：`src/player.rs`、`src/video/demux.rs`、`src/audio/output.rs`、`src/video/decoder.rs`、`src/render/pipeline.rs`、`src/video/worker.rs`
- 觀察：目前播放路徑已具備 demux、decode worker、A/V sync、wgpu render、egui overlay，但播放狀態、首幀、解碼錯誤與 render fallback 的可觀測性不足。

### 9.2 目前播放影片黑畫面問題規格

**現象**：使用完整播放器播放影片時，視窗可能只顯示黑畫面，UI 仍可顯示或程式未崩潰。

**必須修復的行為**：

1. 首幀可視化：
   - 載入媒體後，在播放時鐘開始追幀前，必須盡快顯示第一個可解碼視訊幀。
   - 若 2 秒內未取得任何 decoded frame，UI 必須顯示「等待視訊幀 / 解碼失敗」狀態，而不是只有黑畫面。
2. Render fallback：
   - `RenderPipeline` 沒有 `bind_group` / 尚未 `upload_frame()` 時，不得讓使用者誤判為正常播放；必須顯示載入、無畫面或錯誤 overlay。
   - `render` 層應提供 `has_frame()`、`current_pts()`、`uploaded_frame_count()` 供 UI 顯示診斷資訊。
3. A/V sync 啟動策略：
   - `AvSync` 需支援 startup/seek bootstrap：第一個收到的視訊幀在合理範圍內應可先顯示，避免因 audio clock 與 frame PTS 差距導致一直等待。
   - seek 後應保留上一幀或顯示 seek loading overlay，直到新位置首幀 ready。
4. Decode worker 可觀測性：
   - decode worker 必須把 demux/decode 錯誤、已解碼幀數、最後 frame PTS 回傳主執行緒，避免錯誤只在 debug log 中被吞掉。
   - 對 unsupported codec、extradata 缺失、OBU/NAL 轉換失敗，要轉為 UI 可見錯誤。
5. Frame 資料驗證：
   - `DecodedFrame` 上傳前必須驗證 `Y.len()`、`U.len()`、`V.len()` 與實際 plane width/height 相符。
   - 奇數寬高、非 I420 layout、limited/full range、BT.601/BT.709 色彩矩陣需有明確策略。

### 9.3 優化規格

| 類別 | 新規格 | 驗收標準 |
|------|--------|----------|
| 首幀顯示 | 新增 `PlaybackStartupState` 或等價狀態機 | 載入 720p AV1/H.264 測試檔後 2s 內看到第一幀或明確錯誤 |
| Render 診斷 | render 層暴露 frame count、has_frame、surface error | UI 可顯示目前是否已上傳 frame |
| 解碼診斷 | worker 回傳 `VideoWorkerStatus` | 黑畫面時可看到 demux/decode/queue 狀態 |
| 同步策略 | startup 與 seek 使用 bootstrap frame policy | 起播與 seek 後不永久等待 early frame |
| 效能 | 避免每次 tick clone 大型 `DecodedFrame` | 以 `Arc<DecodedFrame>` 或 frame handle 降低複製成本 |
| 架構 | 拆分 `player.rs` 中 app/event/render/composition 職責 | `PlayerApp`、render compose、playback state 分檔，單檔複雜度下降 |
| 測試 | 補黑畫面回歸測試 | `cargo test` 覆蓋 sync bootstrap、frame validation、worker status |

### 9.4 支援格式規格修正

既有文件宣稱 H.264/H.265 為非目標，但目前程式碼已包含 `video/h264.rs`、`video/h265.rs` 與 `VideoDecoder::{Av1,H264,H265}`。後續規格改為：

- AV1/MP4：主要支援路徑，必須保持可播放。
- H.264/MP4：實驗支援，需補足測試素材與錯誤回報。
- H.265/MP4：實驗支援，需確認 `rust_h265` 解碼能力與實測限制。
- 不支援或失敗的 codec 必須顯示清楚錯誤，不得靜默黑畫面。

---

## 10. CBM 再檢視 — H.264 無法正常播放修復規格（2026-07-06）

### 10.1 CBM 索引摘要（第二輪）

- 專案：`cbm+rust-player`
- 索引結果：40 files、441 symbols、940 edges（CONTAINS 440 / CALLS 333 / IMPORTS 161 / IMPLEMENTS 6）
- 符號熱點（`symbols` 表統計）：`src/player.rs`(54)、`src/i18n/mod.rs`(33)、`src/video/demux.rs`(27)、`src/audio/sink.rs`(23)、`src/audio/output.rs`(21)、`src/sync/mod.rs`(20)、`src/render/pipeline.rs`(18)。
- 現象：H.264 MP4 拖入後畫面黑或嚴重卡頓；debug log 顯示 `extradata_annex_b=0 bytes`、`config_sent=true`、`annex_len=41`、`demuxed=241 decoded=7`、openh264 大量 `Native:16`。

### 10.2 根因清單（依證據排序）

| # | 根因 | 證據 | 影響 |
|---|------|------|------|
| R1 | `extradata_to_annex_b()` 對非標準 avcC（byte5 保留位非 `111`）回傳空 → `config_sent = extradata_annex_b.is_empty()` 為 true → SPS/PPS 永不送達 openh264 | `src/video/nal.rs` L31-42、`src/video/h264.rs` L25；log `extradata_annex_b=0` | 幾乎所有幀解碼失敗（decoded=7/241） |
| R2 | `pack_i420()` 用 `w/2`、`h/2` 計算 UV 平面，`upload_frame()` 用 `div_ceil(2)` 驗證；奇數寬高時 UV 尺寸不符 → validation 拒絕上傳 | `src/video/frame.rs` L44-45 vs `src/render/pipeline.rs` L200-201 | 奇數尺寸影片永久黑畫面 |
| R3 | `decode_loop()` 從不呼叫 `decoder.flush()`；openh264/HEVC 內部緩衝的尾幀與 B-frame 重排幀遺失 | `src/video/worker.rs` L125-204 無 flush 呼叫 | 尾段幀遺失、短片可能整片無輸出 |
| R4 | worker `last_frame_pts` 從未寫入，恆為 0.0 | `src/video/worker.rs` 僅寫 demuxed/decoded；`playback_status()` 讀 `last_frame_pts` | UI 進度/診斷顯示錯誤 |
| R5 | openh264 decoder 未啟用 error concealment，單一壞幀後續放大為連續錯誤 | `src/video/h264.rs` `Decoder::new()` 用預設 config | 抗損性差、卡頓 |
| R6 | demux EOF（`Ok(None)`）後外層 loop 持續空轉，不 flush、不標記 end-of-stream | `src/video/worker.rs` L143 `break` 後續圈空轉 | 播放結束後尾幀不出、CPU 空耗 |

### 10.3 修復規格

1. **extradata 解析健壯化（R1，最高優先）**
   - `extradata_to_annex_b()` 必須：標準路徑失敗時，仍嘗試 AVCC 解析；再失敗嘗試 HVCC；皆失敗才回空。
   - 當 `extradata_annex_b` 為空但 extradata 非空時，`config_sent` 不得無條件設 true；需保留在首個關鍵幀嘗試 in-band SPS/PPS。
   - 必須輸出 extradata 前 32 bytes 的 hex debug log 以利診斷。
   - 驗收：H.264 MP4 起播後 `decoded_frames` 接近 `demuxed_packets`（≥ 95%）。

2. **YUV 平面尺寸一致（R2）**
   - `pack_i420()` 與 `upload_frame()`／`create_plane_textures()` 必須採用相同 UV 尺寸公式：`div_ceil(2)`。
   - 驗收：奇數寬高（如 1920×817）測試幀可正常上傳，無 validation warn。

3. **解碼結束 flush（R3、R6）**
   - demux 到 EOF 時呼叫 `decoder.flush()`，將尾幀送入 queue，並將 worker 標記 end-of-stream，之後降頻或阻塞等待命令。
   - 驗收：短片（< 30 幀）輸出幀數 = 實際可解碼幀數；播放結束 CPU 使用回落。

4. **worker 狀態完整（R4）**
   - 每次成功送出 frame 更新 `last_frame_pts = frame.pts_secs`。
   - 驗收：UI 診斷卡片 last_frame_pts 隨播放遞增。

5. **openh264 抗損（R5）**
   - decoder 啟用 error concealment（`DecoderConfig`）與適當 flush 策略。
   - 驗收：單一損壞幀不致造成後續連續 `Native:16`。

### 10.4 功能優化建議（非阻塞，路線圖）

| 類別 | 建議 | 價值 |
|------|------|------|
| 架構 | 拆分 `src/player.rs`（54 symbols）為 `player/{app,media,compositor,status}.rs` | 降低單檔複雜度與維護風險 |
| 效能 | decode worker 依 queue 飽和度自適應 sleep，取代固定 1/5ms | 降低 CPU 空轉 |
| 相容 | `AudioOutput` 支援 I16/U16 output format | 減少 virtual sink fallback |
| 色彩 | 依 codec metadata 選 BT.601/BT.709 + limited/full range（shader uniform） | 正確色彩 |
| 診斷 | UI 顯示 codec、解析度、fps、decoded/demuxed 比率 | 使用者可見健康度 |
| 測試 | 加入極小合成 H.264/AV1 素材進 `assets/`，納入 smoke test | 可自動回歸 |

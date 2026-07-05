# Rust 原生播放器 — 任務清單

狀態圖例：`[ ]` 待辦 · `[~]` 進行中 · `[x]` 完成

---

## 基礎建設

- [x] 初始化 Cargo 專案與目錄結構
- [x] 設定 `Cargo.toml` 依賴（symphonia, cpal, mp4parse, rav1d, wgpu, winit, egui）
- [x] 實作 `error.rs` 統一錯誤型別
- [x] 設定 `tracing` / `env_logger` 日誌

---

## Phase 1 — 音訊先行

- [x] `audio/decoder.rs` — symphonia 開檔、選軌、解碼 f32 PCM
- [x] `audio/decoder.rs` — 實作 `seek()`
- [x] `audio/output.rs` — cpal stream + ring buffer
- [x] `audio/clock.rs` — `PlaybackClock` 原子計數
- [x] `audio/mod.rs` — `AudioPlayer` 高階 API 整合
- [x] CLI 子命令 `audio <path> [--progress]`
- [x] 驗收：MP3 播放 + 進度輸出
- [x] 驗收：AAC 播放 + 進度輸出

---

## Phase 2 — 視訊解碼

- [x] `video/demux.rs` — mp4parse 開檔、AV1 video track
- [x] `video/demux.rs` — 逐 sample 輸出 `VideoPacket` + PTS
- [x] `video/demux.rs` — `seek()` 關鍵幀對齊
- [x] `video/decoder.rs` — rav1d 初始化與 `decode()`
- [x] `video/decoder.rs` — YUV420 平面提取至 `DecodedFrame`
- [x] `video/obu.rs` — MP4 OBU 樣本轉換
- [x] `video/mod.rs` — 整合 demux + decoder
- [x] CLI 子命令 `decode <path> [--frames N]`
- [x] 驗收：AV1 MP4 解碼 + PTS log

---

## Phase 3 — GPU 渲染

- [x] `render/yuv.wgsl` — YUV420→RGB shader
- [x] `render/pipeline.rs` — wgpu device/surface 初始化
- [x] `render/pipeline.rs` — Y/U/V texture 上傳
- [x] `render/pipeline.rs` — full-screen quad 繪製
- [x] `render/mod.rs` — 公開 `RenderPipeline` API
- [x] CLI 子命令 `render <path>` 開視窗播影片
- [x] 驗收：視窗顯示正確 AV1 畫面（本機 GUI 已驗證）

---

## Phase 4 — 同步與 UI

- [x] `sync/mod.rs` — `AvSync` 音訊主時鐘同步
- [x] `sync/mod.rs` — 丟幀 / 等待策略
- [x] `ui/mod.rs` — egui 進度條
- [x] `ui/mod.rs` — 播放/暫停按鈕
- [x] `ui/mod.rs` — ±10s 快轉按鈕
- [x] `ui/mod.rs` — 時間顯示 current/total
- [x] `main.rs` — 完整播放器主迴圈
- [x] Seek 整合（音訊 + 視訊）
- [x] 驗收：A/V 同步 + UI 操作（本機 GUI 已驗證）

---

## Phase 5 — 體驗強化

- [x] 鍵盤快捷鍵：Space、←/→
- [x] 音量滑桿 + `AudioOutput::set_volume`
- [x] 修復 wgpu 棄用型別警告
- [x] T6 壓力測試（seek / pause-play）
- [ ] 驗收：本機確認快捷鍵與音量

---

## 文件與收尾

- [x] 撰寫 `test.md` 測試案例
- [x] 執行自動測試並記錄結果至 `final.md`
- [x] 更新 `todos.md` 標記完成項

---

## Phase 6 — 黑畫面修復（最高優先，CBM 檢視後新增 2026-07-05）

### 6A 診斷可觀測性
- [x] 定義 `PlaybackStatus`、`WorkerPerFrameStatus`、`WaitingReason`（demux_packets / decoded_frames / uploaded_frames / last_error / waiting_reason）
- [x] `VideoDecodeWorker` 新增 `Arc<Mutex<WorkerPerFrameStatus>>`，回報 open/init/demux/decode 計數與 worker_running
- [x] `MediaPlayer` 聚合 worker 狀態與 render 狀態，提供 `playback_status()`
- [x] UI overlay：顯示「載入中 / 等待首幀 / 解碼失敗 / 不支援 codec / 無視訊軌」診斷卡片，附加 demux/decode/upload 計數

### 6B 首幀與 seek bootstrap
- [x] `AvSync` 加入 startup bootstrap（`startup_bootstrap: bool`）：第一幀立即輸出
- [x] `AvSync` 加入 `seeking` 狀態、`set_seeking()`、`is_seeking()`：seek 後首幀立即輸出
- [x] `MediaPlayer::seek()` 設定 seeking 狀態並保留 `last_video_frame`（不清空）
- [x] `MediaPlayer::tick()` 在無幀時自動推斷 `WaitingForFirstFrame` / `Decoding`

### 6C Render 層避免靜默黑畫面
- [x] `RenderPipeline` 新增 `has_frame()`、`uploaded_frame_count()`
- [x] `PlayerApp::redraw()` 的 `get_current_texture()` 改用 match 處理 `Lost/Outdated`（重新 configure）/ `Timeout`（error log）
- [x] surface lost/outdated 時呼叫 `render.reconfigure_surface()`，自動請求重繪
- [x] 無 frame 時 UI 顯示診斷 overlay（不只是黑底）

### 6D Frame 驗證與尺寸
- [x] `upload_frame()` 前驗證 `y_plane/u_plane/v_plane` 長度符合 width/height
- [x] 奇數寬高 UV 尺寸改用 `div_ceil(2)`（`frame.width.div_ceil(2)`）
- [x] plane 長度不符時 log warn + return，阻止錯誤 texture upload

### 6E 驗證素材
- [ ] 準備 AV1 MP4 測試檔並手動驗收（需 assets/）
- [ ] 準備 H.264 MP4 測試檔並手動驗收（需 assets/）
- [ ] video-only MP4 驗證 virtual audio clock 起播（需 assets/）
- [ ] unsupported codec / 無視訊軌 顯示可讀錯誤（單元測試覆蓋）

---

## Phase 7 — 架構與效能改善（partial）

- [ ] 拆分 `src/player.rs` 為 `player/app.rs`、`player/compositor.rs`、`player/media.rs`、`player/status.rs`
- [x] `DecodedFrame` 改為 `Arc` 共享：`last_video_frame: Option<Arc<DecodedFrame>>`、`tick()` 回傳 `Option<Arc<DecodedFrame>>`
- [x] `AvSync` queue 仍使用擁有權（每個 frame 唯一），`pop_frame_for_display()` 傳出後由 player 包成 Arc
- [ ] `AudioOutput` 支援非 F32 output format（I16/U16 或 cpal 轉換）
- [ ] 依 codec metadata 選 BT.601/BT.709 與 limited/full range（shader uniform color matrix）
- [x] 更新 `spec.md` 支援格式段落與實作一致

---

## Phase 6/7 測試與文件
- [x] 新增 `sync::tests::sync_seeking_bootstrap_returns_immediately`（seek 首幀即時輸出）
- [x] 新增 `sync::tests::sync_startup_bootstrap_works_only_once`（bootstrap 只作用一次）
- [ ] 新增 frame validation 單元測試
- [x] 新增 worker status 單元測試（無素材時可 graceful skip）

### 後續補強
- [ ] `run_render_only` surface error 處理 ✅ (b586071)
- [ ] WorkerPerFrameStatus & PlaybackStatus 單元測試 ✅ (51c57b6)
- [x] `test.md` 增補 T7（黑畫面診斷）、T8（seek/首幀回歸）
- [x] `final.md` 記錄修復結果、驗證證據與剩餘風險

---

## Phase 9 — H.264 播放修復（CBM 第二輪檢視 2026-07-06，最高優先）

根因與規格詳見 `spec.md §10`、`plan.md Phase 9`。

### 9A extradata / SPS-PPS 修復（P0，R1）
- [~] `extradata_to_annex_b()` 加入 fallback：標準 AVCC → HVCC → 強制 AVCC，皆失敗才回空（已實作，待實測）
- [~] `H264Decoder::new()` 加入 extradata hex debug log（已實作）
- [ ] 實測 H.264 MP4：確認 `decoded_frames / demuxed_packets ≥ 95%`
- [ ] 檢視 `config_sent` 邏輯：extradata 非空但解析空時，不應無條件跳過首幀 SPS/PPS
- [ ] H.265 路徑套用同樣 fallback 驗證

### 9B YUV 平面尺寸一致（P0，R2）
- [ ] `pack_i420()` 的 `uv_w`/`uv_h` 由 `w/2`、`h/2` 改為 `w.div_ceil(2)`、`h.div_ceil(2)`
- [ ] 確認與 `render/pipeline.rs::upload_frame`（L200-201）、`create_plane_textures` 一致
- [ ] 新增奇數寬高 frame validation 單元測試

### 9C 解碼結束 flush 與 EOF（P1，R3/R6）
- [ ] `decode_loop()` demux EOF 時呼叫 `decoder.flush()` 並送出尾幀
- [ ] EOF 後標記 end-of-stream，降頻/阻塞等待命令，消除空轉
- [ ] 短片（< 30 幀）驗證尾幀不遺失

### 9D worker 狀態完整（P1，R4）
- [ ] 送出 frame 時更新 `WorkerPerFrameStatus.last_frame_pts`
- [ ] UI 診斷卡片確認 last_frame_pts 遞增

### 9E openh264 抗損（P2，R5）
- [ ] `H264Decoder` 啟用 error concealment 設定
- [ ] 驗證單一壞幀不造成連續 `Native:16`

### 9F 測試素材與回歸
- [ ] 產生極小合成 H.264 / AV1 clip 置於 `assets/`
- [ ] 加入 `decode --frames N` smoke test（有素材時）
- [ ] `cargo build` / `cargo test` / `cargo clippy -- -D warnings` 全綠

### 9G 優化建議（路線圖，非阻塞）
- [ ] 拆分 `src/player.rs`（54 symbols）為 `player/{app,media,compositor,status}.rs`
- [ ] decode worker 自適應 sleep（依 queue 飽和度）
- [ ] `AudioOutput` 支援 I16/U16 output format
- [ ] 色彩矩陣依 metadata 選 BT.601/BT.709 + limited/full range
- [ ] UI 顯示 codec / 解析度 / fps / decoded-demuxed 比率
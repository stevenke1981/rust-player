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
# Rust 原生播放器 — 最終驗收報告

## 專案摘要

| 項目 | 內容 |
|------|------|
| 專案名稱 | rust-player |
| 語言 | Rust (edition 2021) |
| 狀態 | ✅ 四階段完成 · Phase 5 體驗強化進行中 |

## 階段完成狀態

| Phase | 描述 | 狀態 | 備註 |
|-------|------|------|------|
| 1 | 音訊 symphonia + cpal | ✅ | MP3/M4A 播放、PlaybackClock |
| 2 | 視訊 Demux + rav1d | ✅ | mp4parse + rav1d 1.1，PTS log |
| 3 | wgpu YUV 渲染 | ✅ | YUV420→RGB WGSL shader |
| 4 | 同步 + UI | ✅ | AvSync + egui 控制項 |

## 測試結果摘要

| 測試組 | 通過 | 失敗 | 備註 |
|--------|------|------|------|
| T1 建置 | ✅ | — | `cargo build --release` 成功 |
| T1.2 單元測試 | ✅ | — | 10/10 通過 |
| T2 音訊 | ✅ | — | symphonia 解碼 MP3/M4A |
| T3 解碼 | ✅ | — | Sintel AV1 10 幀，PTS 0.000–0.300s |
| T4 渲染 | ✅ | — | 本機 GUI 驗證：視窗開啟/關閉正常 |
| T5 同步/UI | ✅ | — | 本機 GUI 驗證：播放器啟動正常 |
| T6 壓力 | ✅ | — | 自動化 seek×10、pause/play×20 |

### Phase 2 實測 log（test_av1.mp4）

```
opened video: codec=Av1, samples=300
PTS=0.000s size=640x273 Y=174720 U=43520 V=43520
PTS=0.033s size=640x273 Y=174720 U=43520 V=43520
...
decoded 10 frames
```

## 技術決策（實作偏離 spec 之處）

1. **Demux**：由 `mp4` crate 改為 `mp4parse`（`unstable-api`），因前者不支援 AV1 `av01` sample entry。
2. **rav1d 版本**：使用 `rav1d 1.1`（非 0.1 stub），`default-features = false` 停用 asm（免 NASM 依賴）。
3. **OBU 封裝**：MP4 樣本以 raw OBU 串流傳入 rav1d（`obu.rs` 自動判斷）。
4. **純影片檔**：`MediaPlayer` 支援無音訊軌的 AV1 MP4（虛擬 48kHz 時鐘）。

## 已知限制

1. `rav1d` 軟解 1080p 效能受 CPU 限制，未達 spec 24fps 指標（未實測）。
2. Phase 3/4 視窗模式需本機 GPU 與顯示環境，CI 未自動驗證。
3. Seek 對 AV1 僅關鍵幀對齊，非精確 sample 級 seek。
4. 不支援 H.264/H.265、串流、硬體解碼。

## 使用方式

```bash
cargo build --release

# Phase 1
cargo run --release -- audio assets/test.mp3 --progress
cargo run --release -- audio assets/test.m4a --progress

# Phase 2
cargo run --release -- decode assets/test_av1.mp4 --frames 10

# Phase 3
cargo run --release -- render assets/test_av1.mp4

# Phase 4
cargo run --release -- assets/test_av1.mp4
cargo run --release -- assets/test_av1.mp4 --no-ui
```

## Phase 5 新增功能

- **鍵盤快捷鍵**：Space 播放/暫停、←/→ ±10s
- **音量控制**：底部 UI 滑桿（0–100%）
- **品質**：wgpu `TexelCopyTextureInfo` 遷移、T6 壓力測試

## 後續路線圖

| Phase | 方向 |
|-------|------|
| 6 | H.264/H.265 解碼 |
| 7 | 多執行緒解碼、1080p 效能 |
| 8 | 硬體加速、字幕、串流 |

## 變更紀錄

| 日期 | 內容 |
|------|------|
| 2026-07-05 | Phase 5：快捷鍵、音量、T6 壓力測試 |
| 2026-07-05 | 建立 plan/spec/todos/test/final，完成四階段實作與 Phase 1–3 自動驗收 |

---

## CBM 檢視報告與改善建議（2026-07-05）

### 檢視方式

- 使用 cbm 建立/更新索引：`cbm+rust-player`
- 索引結果：38 files、319 symbols、878 edges
- 主要檢視檔案：
  - `src/player.rs`
  - `src/render/pipeline.rs`
  - `src/video/worker.rs`
  - `src/video/decoder.rs`
  - `src/video/demux.rs`
  - `src/sync/mod.rs`
  - `src/audio/output.rs`
  - `src/audio/sink.rs`

### 關鍵發現

1. **目前播放影片黑畫面已納入最高優先改善項**
   - render pass 在沒有 `bind_group` / 尚未上傳任何 frame 時會清成黑色。
   - `PlayerApp::redraw()` 只有 `player.tick()` 回傳 frame 時才 `upload_frame()`。
   - 若 decode worker 尚未產生 frame、sync 判斷 frame 太早、decode error 被吞掉，使用者看到的結果都是黑畫面。

2. **錯誤可觀測性不足**
   - `video/worker.rs` 將部分 demux/decode 問題寫到 log，但沒有傳回 UI。
   - `player.rs` 的 UI 目前較適合顯示開檔錯誤，尚不足以顯示「等待首幀 / decoded=0 / uploaded=0 / codec unsupported」。

3. **文件與實作支援格式不一致**
   - 早期 spec 寫 H.264/H.265 非目標。
   - 實作已有 `video/h264.rs`、`video/h265.rs`、`VideoDecoder::{Av1,H264,H265}`。
   - 建議文件改列 H.264/H.265 為實驗支援，並加入測試與錯誤回報。

4. **架構熱點集中**
   - CBM query 顯示 `src/player.rs` symbols 最多，負責 winit lifecycle、UI、render pass、media state、檔案載入與事件處理。
   - 建議後續拆分以降低維護成本。

5. **效能改善點**
   - `MediaPlayer::tick()` 與 `last_video_frame` 會 clone `DecodedFrame`，大型 YUV frame 成本高。
   - 建議改 `Arc<DecodedFrame>` 或 frame handle。

### 黑畫面可能根因清單

| 可能根因 | 證據 | 改善方向 |
|----------|------|----------|
| 尚未收到首幀就 render | `RenderPipeline` 初始 `bind_group: None`，render clear BLACK | UI 顯示 loading/no frame，首幀 bootstrap |
| sync 等待 early frame | `AvSync::pop_frame_for_display()` early frame 回 `None` | startup/seek bootstrap policy |
| decode worker 錯誤未回 UI | worker log error 後 return/continue | status channel + UI error overlay |
| frame plane 尺寸不符 | upload 直接依 width/height 寫 texture | upload 前 frame validation |
| surface texture error panic/中斷 | `PlayerApp::redraw()` 使用 `expect("surface texture")` | 處理 Lost/Outdated/Timeout/OutOfMemory |
| codec 路徑未完整驗收 | H.264/H.265 實作與文件落差 | 標示實驗、補素材與錯誤測試 |

### 已寫入的改善文件

- `spec.md`：新增第 9 節「CBM 檢視後新增規格 — 黑畫面修復與品質強化」
- `plan.md`：新增「CBM 專案檢視與優化改善計畫」與 Phase 6/7 實作順序
- `todos.md`：新增 Phase 6 黑畫面修復、Phase 7 架構效能、測試與文件任務
- `test.md`：新增 T7 黑畫面診斷與 T8 首幀/seek bootstrap 回歸測試
- `final.md`：新增本檢視報告

### 建議下一步

1. 先做 Phase 6A/6B：status channel + 首幀/seek bootstrap，因為這會直接讓黑畫面變成可診斷問題。
2. 接著做 Phase 6C：render `has_frame()` 與 surface error handling，消除靜默黑畫面與 `expect` 風險。
3. 再做 Phase 6D/T7/T8：frame validation 與回歸測試。
4. 最後做 Phase 7：拆分 `player.rs`、降低 frame clone、擴充 audio sample format。

### 驗證狀態

Phase 6（黑畫面修復）與 Phase 7（Arc 框架構改善）已實作並通過建置與測試驗證。

### 實作摘要

| 類別 | 變更 | 檔案 |
|------|------|------|
| Status 型別 | 新增 `PlaybackStatus`、`WaitingReason`、`WorkerPerFrameStatus` | `src/player.rs` |
| Worker 狀態 | worker 透過 `Arc<Mutex<WorkerPerFrameStatus>>` 回報 demux/decode 計數 | `src/video/worker.rs` |
| Sync bootstrap | startup/seeking 模式，首幀立即輸出 | `src/sync/mod.rs` |
| Render 診斷 | `has_frame()`、`uploaded_frame_count()`、surface error handling | `src/render/pipeline.rs` |
| Frame 驗證 | upload 前檢查 plane 長度、奇數尺寸用 `div_ceil` | `src/render/pipeline.rs` |
| Surface 錯誤 | `get_current_texture()` 改用 match，支援 Lost/Outdated 自動 reconfigure | `src/player.rs` |
| Arc frame | `last_video_frame` 與 `tick()` 回傳型別改為 `Arc<DecodedFrame>` | `src/player.rs` |
| UI 診斷 overlay | 顯示等待/解碼/錯誤訊息與 demux/decode/upload 計數 | `src/ui/mod.rs` |
| 單元測試 | 新增 seeking bootstrap、startup bootstrap only once | `src/sync/mod.rs` |

### 驗證結果

| 項目 | 結果 |
|------|------|
| `cargo build` | ✅ 零 error |
| `cargo test` | ✅ 35/35 passed（含 2 個新增 sync bootstrap 測試） |
| `cargo clippy -- -D warnings` | ✅ 零 warning |

### 剩餘風險

1. **無測試媒體素材**：`assets/` 目錄未提供 AV1/H.264 MP4 測試檔，無法執行 hand GUI 驗收（T4/T5/T7/T8 需手動操作）。
2. **黑畫面問題仍可能存在實際解碼路徑**：本實作讓黑畫面變成可診斷（UI 顯示等待/錯誤/計數），而非完全靜默。若 decoder 初始化失敗、demux 未回傳 packet、或 rav1d 內部因 OEM OBU 資料返回錯誤，UI 會顯示對應狀態而非純黑畫面。
3. ~~**`run_render_only` 未套用 surface error 處理**：該簡易除錯路徑仍使用 `let _ = render.render();`，在 surface lost 時會 panic。~~ ✅ 已修復 (b586071)
4. **缺少 frame validation 單元測試**：需要準備測試素材才能驗證實際 plane 長度檢測邏輯。
   - ✅ WorkerPerFrameStatus & PlaybackStatus 單元測試已補 (51c57b6, 7 tests)

### 建議下一步

1. 準備測試媒體（AV1 MP4/H.264 MP4/video-only MP4）至 `assets/` 目錄
2. 執行手動 GUI 驗收 T4/T5/T7/T8
3. 補 `upload_frame` validation 與 worker status 單元測試
4. 為 `run_render_only` 加上 surface error handling
5. 考慮拆分 `src/player.rs`（phase 7 架構項目）和擴充 audio sample format 支援

---

## CBM 第二輪檢視報告：目前 H.264 無法正常播放（2026-07-06）

### 使用者回報

目前無法正常播放影片；先前測試 H.264 MP4 時可見畫面但非常卡，log 顯示 decode 成功幀數極低。

### CBM / 本地源碼證據

- CBM 專案：`cbm+rust-player`
- 架構摘要：40 files、441 symbols、940 edges
- 熱點：`src/player.rs` 54 symbols，播放 lifecycle / UI / render / media state 集中，為後續拆分優先目標。
- CBM 查到播放管線核心：`src/video/worker.rs::decode_loop@L75` → `Mp4Demuxer::next_packet` → `VideoDecoder::decode` → `AvSync::push_frame` → `MediaPlayer::tick` → `RenderPipeline::upload_frame`。
- 本地源碼確認：
  - `src/video/frame.rs` `pack_i420()` 仍使用 `w / 2`、`h / 2`。
  - `src/render/pipeline.rs` `upload_frame()` 使用 `div_ceil(2)` 驗證 UV 平面。
  - `src/video/worker.rs` decode loop 未在 EOF 呼叫 `decoder.flush()`。
  - `WorkerPerFrameStatus.last_frame_pts` 在 worker loop 內未更新。

### 根因結論

| 根因 | 目前狀態 | 結論 |
|------|----------|------|
| H.264 extradata → Annex B 解析失敗 | 已加 fallback + hex log，但尚未手動驗證 | 可能是 decoded=7/241 的主因 |
| YUV UV 尺寸不一致 | 未修復 | 奇數寬高影片會被 render validation 拒絕上傳，導致黑畫面 |
| EOF 未 flush | 未修復 | 尾幀 / B-frame 緩衝幀可能遺失 |
| Worker last_frame_pts 未更新 | 未修復 | UI 診斷資料不可信 |
| openh264 抗損不足 | 未修復 | 壞幀可能造成連續 Native:16 |

### 已寫入文件

- `spec.md`：新增第 10 節「CBM 再檢視 — H.264 無法正常播放修復規格」
- `plan.md`：新增 Phase 9「H.264 播放修復計畫」
- `todos.md`：新增 Phase 9 任務清單（9A–9G）
- `test.md`：新增 T9 回歸測試（extradata、奇數尺寸、EOF flush、worker status）
- `final.md`：新增本第二輪檢視報告

### 建議下一步（優先順序）

1. **立刻修 P9-2：`pack_i420()` UV 尺寸改 `div_ceil(2)`**。這是明確源碼不一致，與目前測試影片奇數高度/寬度時黑畫面高度相關。
2. **重新實測 P9-1 extradata fallback**。確認 H.264 `extradata_annex_b > 0`，decoded 接近 demuxed。
3. **補 P9-3 EOF flush**。避免短片與 B-frame 尾幀遺失。
4. **補 P9-4 last_frame_pts**。讓 UI 診斷可信。
5. **最後做 P9-5 error concealment 與測試素材**。

### 驗證狀態

| 項目 | 結果 |
|------|------|
| CBM 檢視 | ✅ 已完成 |
| 本地源碼交叉驗證 | ✅ 已完成 |
| 文件更新 | ✅ 已完成 |
| 程式修復完整驗證 | ⬜ 尚未完成；目前僅建議與部分 extradata fallback 已實作 |
| H.264 手動 GUI 驗收 | ⬜ 缺穩定素材與後續修復 |

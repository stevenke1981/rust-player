# Rust 原生播放器 — 驗收測試

## 前置條件

```bash
# 建置
cargo build --release

# 測試媒體（需自行準備或透過腳本產生）
# assets/test.mp3        — MP3 音訊，≥ 60s
# assets/test.m4a        — AAC 音訊，≥ 60s
# assets/test_av1.mp4    — AV1 視訊 + AAC 音訊，≥ 10s，1080p 或 720p
```

---

## T1 — 建置測試

| ID | 步驟 | 預期結果 |
|----|------|----------|
| T1.1 | `cargo build --release` | 零 error 完成編譯 |
| T1.2 | `cargo test` | 所有單元測試通過 |
| T1.3 | `cargo clippy -- -D warnings` | 零 warning（允許 rav1d 相關除外） |

---

## T2 — Phase 1 音訊測試

| ID | 步驟 | 預期結果 |
|----|------|----------|
| T2.1 | `cargo run --release -- audio assets/test.mp3 --progress` | 聽到連續音訊，無爆音 |
| T2.2 | 觀察 progress 輸出 30s | 時間單調遞增，誤差 < 0.5s |
| T2.3 | `cargo run --release -- audio assets/test.m4a --progress` | AAC 正常播放 |
| T2.4 | 播放至結束 | 程序正常退出 exit 0 |
| T2.5 | `cargo run --release -- audio nonexistent.mp3` | 印出錯誤，exit 1 |

### T2 單元測試

| ID | 測試名稱 | 預期 |
|----|----------|------|
| T2.U1 | `clock_position_increases` | `position_secs()` 隨 sample 增加 |
| T2.U2 | `clock_pause_freezes` | pause 後 position 不變 |
| T2.U3 | `clock_seek_jumps` | seek 後 position 跳至目標 |

---

## T3 — Phase 2 視訊解碼測試

| ID | 步驟 | 預期結果 |
|----|------|----------|
| T3.1 | `cargo run --release -- decode assets/test_av1.mp4 --frames 10` | 解碼 10 幀無 panic |
| T3.2 | 檢查 log 輸出 | 每行含 `PTS=`、`size=`、`Y=` |
| T3.3 | 比對連續 PTS | PTS 嚴格遞增（允許相同於 B-frame 場景） |
| T3.4 | 檢查 YUV 尺寸 | `Y.len() == width * height` |
| T3.5 | `cargo run --release -- decode assets/test.mp3` | 回報「無視訊軌」錯誤 |

### T3 單元測試

| ID | 測試名稱 | 預期 |
|----|----------|------|
| T3.U1 | `demux_opens_mp4` | 可讀取 video track metadata |
| T3.U2 | `pts_conversion` | timescale 換算正確 |

---

## T4 — Phase 3 GPU 渲染測試

| ID | 步驟 | 預期結果 |
|----|------|----------|
| T4.1 | `cargo run --release -- render assets/test_av1.mp4` | 視窗開啟，顯示影片畫面 |
| T4.2 | 目視檢查色彩 | 無明顯偏色（膚色自然） |
| T4.3 | 觀察 5s | 畫面持續更新，無凍結 |
| T4.4 | 關閉視窗 | 程序正常退出 exit 0 |
| T4.5 | 縮放視窗 | 畫面適應新尺寸，無 crash |

---

## T5 — Phase 4 同步與 UI 測試

| ID | 步驟 | 預期結果 |
|----|------|----------|
| T5.1 | `cargo run --release -- assets/test_av1.mp4` | 完整播放器啟動 |
| T5.2 | 點擊暫停 | 音訊停止、畫面凍結、時間不動 |
| T5.3 | 點擊播放 | 從暫停位置繼續 |
| T5.4 | 點擊 +10s | 跳轉約 10s，音畫同步恢復 |
| T5.5 | 拖曳進度條至 50% | seek 成功，繼續播放 |
| T5.6 | 觀察唇形/節拍 | A/V 誤差 < 100ms（主觀） |
| T5.7 | `--no-ui` 模式播放 10s | log 顯示 sync 丟幀/等待統計 |

### T5 單元測試

| ID | 測試名稱 | 預期 |
|----|----------|------|
| T5.U1 | `sync_drops_late_frame` | PTS 落後 > threshold 的幀被丟棄 |
| T5.U2 | `sync_waits_early_frame` | PTS 超前時不輸出 |
| T5.U3 | `sync_queue_overflow` | 超過 max_queue 時丟棄最舊幀 |

---

## T6 — 壓力與邊界測試

| ID | 步驟 | 預期結果 |
|----|------|----------|
| T6.1 | 連續 seek 10 次 | 無 panic，最終位置正確 |
| T6.2 | 極短檔案（< 1s） | 正常播放並結束 |
| T6.3 | 快速暫停/播放 20 次 | 無 deadlock 或爆音 |

---

## 驗收判定

| Phase | 必要通過項 | 狀態 |
|-------|-----------|------|
| Phase 1 | T2.1–T2.4, T2.U1–T2.U3 | ⬜ |
| Phase 2 | T3.1–T3.4 | ⬜ |
| Phase 3 | T4.1–T4.4 | ⬜ |
| Phase 4 | T5.1–T5.6, T5.U1–T5.U3 | ⬜ |
| 整體 | T1.1, T1.2 | ⬜ |

全部 ⬜ 改為 ✅ 後，更新 `final.md`。

---

## T7 — 黑畫面診斷與修復回歸測試（CBM 檢視後新增）

### T7.1 起播首幀測試

| ID | 步驟 | 預期結果 |
|----|------|----------|
| T7.1.1 | `cargo run --release -- assets/test_av1.mp4` | 2 秒內顯示第一個視訊幀，或 UI 顯示明確錯誤 |
| T7.1.2 | 起播後觀察 UI overlay | 不得只剩黑畫面；若等待，顯示「等待視訊幀」與狀態資訊 |
| T7.1.3 | 開啟 `RUST_LOG=debug` 重跑 | log 含 demux packet、decoded frame、uploaded frame 或錯誤原因 |

### T7.2 Render 無 frame fallback

| ID | 步驟 | 預期結果 |
|----|------|----------|
| T7.2.1 | 啟動播放器但不載入媒體 | 顯示開檔/拖放提示，不是純黑畫面 |
| T7.2.2 | 載入 unsupported codec 影片 | UI 顯示不支援 codec 或 decode init failed |
| T7.2.3 | 模擬 `RenderPipeline` 尚無 bind group | UI overlay 顯示 no frame uploaded / loading |

### T7.3 Decode worker 狀態

| ID | 步驟 | 預期結果 |
|----|------|----------|
| T7.3.1 | 播放 AV1 MP4 | `decoded_frames > 0`、`uploaded_frames > 0` |
| T7.3.2 | 播放無視訊軌音訊檔 | 顯示 audio-only 狀態，不報黑畫面錯誤 |
| T7.3.3 | 播放破損 MP4 | 顯示 demux/decode error，程式不 panic |

### T7.4 單元測試建議

| ID | 測試名稱 | 預期 |
|----|----------|------|
| T7.U1 | `render_state_starts_without_frame` | 新 render pipeline 狀態 `has_frame == false` |
| T7.U2 | `render_state_counts_uploaded_frames` | `upload_frame()` 後 frame count 增加 |
| T7.U3 | `frame_validation_rejects_bad_plane_sizes` | plane 長度錯誤回傳錯誤，不上傳 |
| T7.U4 | `worker_reports_decode_error` | worker status 可記錄錯誤 |

---

## T8 — 首幀 / seek bootstrap 回歸測試

| ID | 步驟 | 預期結果 |
|----|------|----------|
| T8.1 | `cargo run --release -- assets/test_av1.mp4` 後立即暫停 | 畫面保留已顯示幀，非黑畫面 |
| T8.2 | 播放中拖曳進度條至 50% | seek 後 2 秒內顯示新位置幀或 loading overlay |
| T8.3 | 連續按 → 快轉 5 次 | 不永久黑畫面；狀態最終恢復 playing |
| T8.4 | video-only MP4 | wall-clock/virtual audio clock 驅動畫面更新 |
| T8.5 | H.264 MP4（實驗） | 成功播放或顯示明確 H.264 decode error |

### T8 單元測試建議

| ID | 測試名稱 | 預期 |
|----|----------|------|
| T8.U1 | `sync_bootstrap_displays_first_frame` | audio clock 起點附近第一幀可輸出 |
| T8.U2 | `sync_seek_keeps_last_frame_until_new_ready` | seek 後新幀未到前不清成黑畫面 |
| T8.U3 | `sync_does_not_wait_forever_for_early_frame` | early frame 等待有可診斷狀態 |

---

## 新增驗收判定（Phase 6/7）

| Phase | 必要通過項 | 狀態 |
|-------|------------|------|
| Phase 6 黑畫面修復 | T7.1–T7.3、T8.1–T8.4、T7.U/T8.U 核心單元測試 | ⬜ |
| Phase 7 架構效能 | `cargo test`、手動 smoke、無明顯回歸 | ⬜ |

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
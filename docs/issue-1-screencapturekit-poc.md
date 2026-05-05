# Issue #1: ScreenCaptureKit per-app 音声キャプチャ PoC

PR: https://github.com/duosonic62/knob/pull/5

## 目的

macOS の ScreenCaptureKit を使い、**特定アプリの音声だけを単独で取得できるか**を検証する。
これが成立しないとソフトウェアミキサー全体の前提が崩れるため、最初に確認すべき技術PoC。

## 結果

Spotify 再生中にキャプチャ開始 → 停止後 WAV を確認し、Spotify の音のみが録音されていることを確認。
音量は小さめだが、これは SCK がノーマライズ済みの信号を渡す仕様によるもの（後段ゲインで対処可能）。

---

## アーキテクチャ

```
Frontend (Svelte)
    ↓ invoke("list_audio_apps" | "start_capture" | "stop_capture")
Tauri コマンド (src-tauri/src/lib.rs)
    ↓
audio::capture (src-tauri/src/audio/capture.rs)
    ↓
screencapturekit crate v1.5.4
    ↓
ScreenCaptureKit.framework (macOS)
```

### 主要な型・関数

| シンボル | 役割 |
|---|---|
| `AppInfo` | bundle_id / name / pid を持つ DTO |
| `CaptureState` | `Mutex<Option<CaptureSession>>` を保持する Tauri State |
| `CaptureSession` | 実行中の `SCStream` + `WavWriter` をまとめた構造体 |
| `list_audio_apps()` | `SCShareableContent::get()` で起動中アプリ一覧を返す |
| `start_capture(bundle_id)` | `SCContentFilter` でアプリを絞り込み `SCStream` を起動 |
| `stop_capture()` | ストリームを停止し WAV をファイナライズ、保存パスを返す |

### フィルタ設定

```rust
let filter = SCContentFilter::create()
    .with_display(display)
    .with_including_applications(&[target_app], &[])
    .build();

let config = SCStreamConfiguration::new()
    .with_captures_audio(true)
    .with_excludes_current_process_audio(true)
    .with_sample_rate(48000)
    .with_channel_count(2)
    .with_width(2)   // 映像不要だが SCK は最低サイズ必須
    .with_height(2);
```

`width=2, height=2` は映像フレームを最小化するワークアラウンド。
ScreenCaptureKit は audio-only モードを公式にはサポートしていない。

### 音声ハンドラ内の処理

SCK は **non-interleaved PCM** (チャンネルごとに別バッファ) を渡す。
WAV への書き込み前にインターリーブ変換が必要：

```rust
// channels[0] = L, channels[1] = R (それぞれ独立バッファ)
let frame_count = channels[0].len();
for frame in 0..frame_count {
    for ch in &channels {
        wav.write_sample(ch[frame]);  // L, R, L, R, ... の順に書き込み
    }
}
```

RMS は 50 コールバックごとに間引いてログ出力（`Arc<AtomicU64>` でカウント）。
クロージャが `Fn`（`FnMut` 不可）という制約があるため、カウンタは `Arc<AtomicU64>` で保持する。

---

## 権限設定

### NSScreenCaptureUsageDescription

`src-tauri/Info.plist` に権限の説明文を記載し、`tauri.conf.json` で参照する。
この文言がないと macOS が権限ダイアログを表示しない。

```xml
<key>NSScreenCaptureUsageDescription</key>
<string>Knob はアプリ別音量制御のためにアプリの音声をキャプチャします。</string>
```

```json
// tauri.conf.json
"macOS": {
  "infoPlist": "Info.plist"
}
```

### TCC（Transparency, Consent, Control）

`pnpm tauri dev` はバイナリを直接起動するため `.app` バンドルが作成されず、
macOS の TCC がアプリを認識できない（System Settings の一覧に現れない）。

**対処**: `pnpm tauri build --debug` で `src-tauri/target/debug/bundle/macos/knob.app` を生成し、
そのバンドルを起動することで TCC の権限ダイアログが表示される。

権限のリセットが必要な場合:
```bash
tccutil reset ScreenCapture com.duosonic62.knob
```

---

## ビルド時のはまりポイント

### 1. pnpm esbuild ビルドスクリプトブロック

pnpm 11 はデフォルトでビルドスクリプトを禁止する。
`pnpm-workspace.yaml` で明示的に許可が必要：

```yaml
# pnpm-workspace.yaml
allowBuilds:
  esbuild: true
```

### 2. macOS 26 + CommandLineTools での Swift runtime dylib エラー

```
dyld: Library not loaded: @rpath/libswift_Concurrency.dylib
```

Apple の新リンカー（ld-1167+）は `@rpath` エントリのうち、ディスク上に実ファイルがないものを削除する。
macOS 26 では `libswift_Concurrency.dylib` は dyld shared cache にのみ存在するため、
`screencapturekit` クレートが追加する `-rpath,/usr/lib/swift` が削除されてしまう。

**対処** (`build.rs`):
CommandLineTools の `swift-5.5/macosx/` パスを追加 rpath として指定する。
起動時に ObjC の duplicate-class 警告が出るが、クラッシュはしない。

```rust
let path = format!("{base}/usr/lib/swift-5.5/macosx");
println!("cargo:rustc-link-arg=-Wl,-rpath,{path}");
```

---

## 既知の制限（次 Issue 以降で対処）

| 制限 | 対応 Issue |
|---|---|
| WAV 保存先が `/tmp` にハードコード | Issue #3 (設定永続化) |
| 音量が小さい（SCK のノーマライズ仕様） | Issue #2 (フェーダー UI でゲイン調整) |
| 複数アプリ同時キャプチャ非対応 | 未定 |
| ビデオフレームも生成されている（`width=2, height=2` で最小化中） | 将来の最適化 |

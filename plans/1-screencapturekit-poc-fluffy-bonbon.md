# Issue #1: ScreenCaptureKit によるアプリ別音声取得 PoC

## Context

Knob は macOS 上でアプリ別に音量を制御するソフトウェアミキサーを目指している。実装の最初の山場は「特定アプリの音声ストリームを単独で取得できるか」で、これが成立しないとミキサー全体の前提が崩れる。

ScreenCaptureKit（macOS 12.3+）は `SCContentFilter` でアプリを指定し、`SCStreamConfiguration.capturesAudio = true` にすることでそのアプリの音声のみをキャプチャできる。Rust からは [`screencapturekit`](https://github.com/doom-fish/screencapturekit-rs) クレートが安全な薄いバインディングを提供しており、`14_app_capture.rs` および `22_tauri_app/` のサンプルが存在する。

このPoCのゴールは、**任意の起動中アプリの音声を Tauri バックエンドで取得し、ログ出力 + WAVファイル化できることを確認する**こと。完了後、Issue #2 のフェーダーUIや #4 の仮想デバイスへのルーティングと接続していく。

## 完了条件

1. `npm run tauri dev` で起動した Knob 上で、起動中の音声出力アプリを一覧取得できる
2. アプリ（例：Spotify）の bundle ID を指定してキャプチャ開始 → そのアプリで音楽再生中、RMS値がコンソールに継続的に出力される
3. キャプチャ停止後、`/tmp/knob_poc_<bundle_id>.wav` が生成され、QuickTime 等で再生して当該アプリの音だけが入っていることを確認できる
4. 画面録画権限の許可フローが正しく動く（未許可なら明示的なエラーが返る）

## 実装範囲

### 0. ブランチ作成

実装着手前に main から作業ブランチを切る。

```bash
git checkout -b issue-1-screencapturekit-poc
```

最終的にこのブランチを Issue #1 に紐づくPRとしてレビューに回す。

### 1. Rust 側依存追加

**ファイル: `src-tauri/Cargo.toml`**

```toml
[target.'cfg(target_os = "macos")'.dependencies]
screencapturekit = "*"     # 着手時に `cargo search screencapturekit` で最新を固定
hound = "3.5"              # WAV書き出し
parking_lot = "0.12"       # 同期プリミティブ
log = "0.4"
env_logger = "0.11"
```

> `screencapturekit` は v0.x と v1.x で API が大きく異なる。着手時に最新メジャー版を確認し、`14_app_capture.rs` のサンプルが該当バージョンで動くことを確認してから固定する。

### 2. 音声キャプチャモジュール

**新規ファイル: `src-tauri/src/audio/mod.rs`**, **`src-tauri/src/audio/capture.rs`**

責務:
- `list_audio_apps()` → 起動中アプリのうちオーディオを再生し得るもの（簡易にすべての `running_applications()`）を `Vec<AppInfo { bundle_id, name, pid }>` で返す
- `start_capture(bundle_id)` → 該当アプリへの `SCContentFilter` を作り、`SCStream` を起動。サンプルハンドラ内で：
  - RMS を計算して `log::info!` に出力（数百msごとに間引く）
  - `hound::WavWriter` に書き込み
- `stop_capture()` → ストリーム停止、WAVクローズ、保存パスを返す
- 状態は `Mutex<Option<CaptureSession>>` として保持。複数同時キャプチャは PoC では非対応。

**設計メモ:**
- 動画出力は不要だが、SCK は内部的にビデオフレームも生成する。`width=2, height=2` の最小設定 + `SCStreamOutputType::Screen` ハンドラ未登録（または no-op）でドロップさせる
- `SCStreamOutputType::Audio` のハンドラのみ実装し、`CMSampleBuffer` から PCM を抽出
- サンプルレートは 48kHz / ステレオ固定で開始（`with_sample_rate(48000).with_channel_count(2)`）

### 3. Tauri コマンド配線

**変更: `src-tauri/src/lib.rs`**

```rust
mod audio;

#[tauri::command]
async fn list_audio_apps() -> Result<Vec<audio::AppInfo>, String> { ... }

#[tauri::command]
async fn start_capture(bundle_id: String) -> Result<(), String> { ... }

#[tauri::command]
async fn stop_capture() -> Result<String, String> { ... }   // 返り値は WAVパス
```

`invoke_handler` から既存の `greet` を外し、上記3つを登録する。

### 4. Info.plist の権限文言

Tauri v2 では (a) `src-tauri/Info.plist` を新規作成して CLI 生成分にマージさせる方式と、(b) `tauri.conf.json` の `bundle.macOS.infoPlist` で記述する方式の二つが使える。本PoCでは設定が一箇所にまとまる **(b) 方式** を採用する。

**変更: `src-tauri/tauri.conf.json`**

```json
"bundle": {
  "...": "...",
  "macOS": {
    "infoPlist": {
      "NSScreenCaptureUsageDescription": "Knob はアプリ別音量制御のためにアプリの音声をキャプチャします。"
    }
  }
}
```

> Info.plist の使用説明文言が無いと、画面録画権限ダイアログがそもそも表示されない（または OS にクラッシュ扱いされる）ので必須。

### 5. 動作確認用の最小フロントエンド

**変更: `src/routes/+page.svelte`**

既存の greet サンプルを置き換え：
- 「アプリ一覧取得」ボタン → 結果を `<ul>` 表示、各行にキャプチャ開始ボタン
- 「停止」ボタン → 停止して保存先パスを表示
- ログ表示は `console.log` でブラウザ DevTools に流す（PoC のため凝らない）

## 検証手順

> 注: `tauri.conf.json` は `beforeDevCommand` が `pnpm dev` を指している。`pnpm` が未インストールなら `npm i -g pnpm` するか、`tauri.conf.json` を `npm run dev` に揃える（PoC着手時に判断）。

1. `pnpm install`（または `npm install`）
2. `pnpm tauri dev`（または `npm run tauri dev`）で起動
3. 起動時に macOS から画面録画許可を求められるので許可する（必要なら「システム設定 → プライバシーとセキュリティ → 画面収録」で Knob にチェック後アプリ再起動）
4. Spotify などで音楽を再生開始
5. UI で「アプリ一覧取得」→ Spotify を選んでキャプチャ開始
6. ターミナル上の `RUST_LOG=info` ログで RMS 値が時系列に動くこと、ゼロにならないことを確認
7. 5〜10秒後に「停止」→ 表示された WAV パスを QuickTime で開き、Spotify の音だけが録音されていることを確認
8. **対照実験**: Spotify を停止し他アプリ（YouTube等）の音だけ流した状態で再度キャプチャ → 無音 or 極小 RMS になることを確認（アプリ分離の検証）

> **権限を再要求したい場合**: `tccutil reset ScreenCapture com.duosonic62.knob` でリセット可能（macOS の TCC 許可キャッシュをクリア）。

## 既知のリスク・要確認事項

- **`screencapturekit` クレートのバージョン**: v0.x と v1.x で API 形式が異なる。実装開始時に最新版を確認し、ドキュメントで `with_application` / app filter API 名を確定する
- **音声のみ vs ビデオ必須**: Apple ドキュメント上 SCK は audio-only モードを正式サポートしていない。`width=2, height=2` で実質 audio-only 化するワークアラウンドが必要
- **macOS バージョン**: 開発機が 13 以上であることを前提とする。`Cargo.toml` の cfg ガードで macOS 限定にする
- **サンドボックス**: `tauri.conf.json` のデフォルトでは hardened runtime + sandbox が効く可能性。screen recording 権限が降りない場合は entitlements を見直す

## 参考リンク

- [screencapturekit-rs (GitHub)](https://github.com/doom-fish/screencapturekit-rs)
- [`14_app_capture.rs` 例](https://github.com/doom-fish/screencapturekit-rs/tree/main/screencapturekit/examples)
- [`22_tauri_app/` Tauri統合例](https://github.com/doom-fish/screencapturekit-rs/tree/main/screencapturekit/examples)
- [Apple: SCStreamConfiguration.capturesAudio](https://developer.apple.com/documentation/screencapturekit/scstreamconfiguration/capturesaudio)
- [Apple: ScreenCaptureKit 概要](https://developer.apple.com/documentation/screencapturekit/)

## スコープ外（次のIssueへ）

- 仮想オーディオデバイスへのルーティング → Issue #4
- フェーダーUI・ミュート操作 → Issue #2
- 設定の永続化 → Issue #3
- 複数アプリの同時キャプチャ
- マイク入力のキャプチャ

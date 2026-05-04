# Knob

macOS向けのソフトウェアオーディオミキサー。アプリケーションごとの音量をフェーダーで独立制御できることを目指しています。

## 機能概要（計画中）

- アプリケーション別の音声キャプチャ（CoreAudio / ScreenCaptureKit）
- フェーダーUIによる音量・ミュート操作
- 仮想オーディオデバイスへのルーティング（ループバック出力）
- 設定の永続化（チャンネル構成・ルーティング設定）

## 技術スタック

| レイヤー | 技術 |
|---|---|
| フレームワーク | [Tauri v2](https://tauri.app/) |
| フロントエンド | [SvelteKit](https://kit.svelte.dev/) + TypeScript |
| バックエンド | Rust |
| オーディオ | CoreAudio / ScreenCaptureKit（macOS） |

## 開発環境のセットアップ

### 前提条件

- [Rust](https://www.rust-lang.org/tools/install) (stable)
- [Node.js](https://nodejs.org/) v20以上
- macOS 13 (Ventura) 以上

### 手順

```bash
# 依存関係のインストール
npm install

# 開発サーバー起動（ホットリロード付き）
npm run tauri dev

# プロダクションビルド
npm run tauri build
```

### フロントエンドのみ確認

```bash
npm run dev
```

## プロジェクト構成

```
knob/
├── src/                    # SvelteKit フロントエンド
│   └── routes/
│       └── +page.svelte    # メインUI
├── src-tauri/              # Rust バックエンド
│   ├── src/
│   │   ├── main.rs
│   │   └── lib.rs          # Tauri コマンド定義
│   └── tauri.conf.json     # Tauri 設定
└── static/                 # 静的アセット
```

## ライセンス

MIT

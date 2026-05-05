# Issue #4: macOS 仮想オーディオデバイスの選択肢と実装方針

PR: *(作成後に更新)*

## 目的

Knob がアプリ別に取得した音声（ScreenCaptureKit — Issue #1 参照）を **「個別チャンネルとして」DAW / 配信ソフトへ渡す**ために必要な仮想オーディオデバイスの選択肢を整理し、推奨を決定する。

## 結果 (TL;DR)

- **v1 推奨（Knob 0.x）**: BlackHole 16ch をユーザに別途インストールしてもらう構成を採用。起動時に未検出ならガイド画面（`brew install` / pkg リンク）を出す。
- **v2 検討（Knob 1.x）**: 自作 AudioServerPlugin を `.pkg` で同梱。要 Developer ID 署名 + Notarization パイプライン整備。v1 の UX フィードバック後に着手判断。
- Loopback はエンドユーザに $99 を要求するため Knob が前提にするのは非現実的と判断し却下。

---

## 候補比較表

| 軸 | BlackHole (16ch) | Loopback (Rogue Amoeba) | 自作 AudioServerPlugin |
|---|---|---|---|
| **ライセンス** | GPL-3.0 ([GitHub](https://github.com/ExistentialAudio/BlackHole)) | 商用 $99/ユーザ ([rogueamoeba.com](https://rogueamoeba.com)) | Apple SDK 無償 (要 Developer Program) |
| **入手方式** | `brew install blackhole-16ch` / pkg | rogueamoeba.com 購入 | Apple サンプル `NullAudio` / `libASPL` ([gavv/libASPL](https://github.com/gavv/libASPL)) |
| **チャンネル数** | 固定 (2ch / 16ch / 64ch の variant を選択) | 可変 (最大 64ch/デバイス) | 実装次第 (N ch / N デバイスを動的に設定可能) |
| **インストール先** | `/Library/Audio/Plug-Ins/HAL/BlackHoleXch.driver` | `/Applications/Loopback.app` + ARK plugin | `/Library/Audio/Plug-Ins/HAL/KnobDriver.driver` |
| **管理者権限** | 要 (インストーラが sudo) | 要 (ARK plugin インストール時) | 要 (`.driver` 配置時) |
| **Knob からの API 制御** | **なし** — チャンネル数・名前はビルド時固定。ランタイム変更不可 | ARK-SDK ライセンス要。ACE は macOS 15 で deprecated | **完全制御可** — XPC / URL scheme で動的設定可 |
| **スピーカーとの共存** | 要 Audio MIDI 設定で Multi-Output Device 手動作成 (注1) | ネイティブに共存 | 実装次第 (Aggregate Device 対応可) |
| **配布要件** | Developer ID + Notarization 済 pkg 形式 (BlackHole 側で完結) | Loopback 本体の購入のみ | 要 Developer ID + Notarization + `.pkg` インストーラ |
| **`.app` 内同梱** | **不可** — システム `/Library/` に配置が前提 | **不可** — 独立した有料アプリ | **条件付き可** — `App.app/Contents/PlugIns/` に置き entitlement + 署名で読み込み可能。ただし `/Library/Audio/Plug-Ins/HAL/` への配置が標準であり、`.app` 同梱でも初回起動時に管理者認証ダイアログか `.pkg` コピーが必要 |
| **対象 macOS** | 10.13+ | 10.14+ | 10.8+ (AudioServerPlugIn); DriverKit 対応は 11+ |
| **主な制約** | 固定 ch 数、Multi-Output 設定が複雑、GPL | ユーザへ $99 要求、ARK-SDK 商用 | 実装コスト大、Notarization 必須、Xcode CI 整備要 |

**注1**: Multi-Output Device では `Built-in Output` をマスタークロックに設定しないと AV 同期がずれる ([BlackHole Wiki](https://github.com/ExistentialAudio/BlackHole/wiki/Multi-Output-Device))。

### 比較から外した候補

| 候補 | 除外理由 |
|---|---|
| **Soundflower** | Apple Silicon 非対応、SIP によりカーネル拡張がブロック、メンテナンス停止 |
| **kext 系** | Apple が廃止方向。macOS 15+ でのロードが不安定 |
| **SoundSource / Audio Hijack** | 仮想デバイスドライバではなく録音・ルーティング用途のアプリ |

---

## 署名・公証要件の詳細（AudioServerPlugin）

AudioServerPlugin は kext ではなく **`coreaudiod`（システム所有のユーザー空間デーモン）にロードされる `.driver` バンドル**。

| 要件 | 理由 |
|---|---|
| **Developer ID 署名** | `coreaudiod` は未署名 / ad-hoc 署名のバンドルをロードしない。システムプロセスへのコード注入に相当するため |
| **Notarization** | App Store 外配布では Gatekeeper が `/Library/...` への配置を隔離。`.pkg` またはその中のバンドル全体を公証する必要がある ([Apple Developer Forums](https://developer.apple.com/forums/thread/116003)) |
| **Hardened Runtime** | Apple の公証要件のひとつ。`com.apple.security.device.audio-input` 等エンタイトルメントが必要 |
| **インストーラ必須** | `/Library/Audio/Plug-Ins/HAL/` への書き込みは管理者権限が必要。`.app` 同梱のみで完結不可 (ユーザ操作なしのサイレントインストールは不可) |

---

## BlackHole ハンズオン検証

### インストール

```bash
brew install blackhole-16ch
# → sudo が必要なため、ターミナルで直接実行
# 完了後、再起動または以下で再ロード:
sudo killall -9 coreaudiod
```

インストール後に `/Library/Audio/Plug-Ins/HAL/` に `BlackHole16ch.driver` が配置される。

### システムからの確認

インストール後の状態：

- **システム設定 > サウンド > 出力** に "BlackHole 16ch" が列挙される
- **Audio MIDI 設定.app** で 16ch デバイスとして確認可能

### Multi-Output Device（スピーカー同時出力）の構成

Audio MIDI 設定.app で以下を設定：

1. 左下の `+` → 「機器セットを作成」
2. **Mac mini スピーカー**（クロックソース に設定）+ **BlackHole 16ch**（音ズレ補正 を有効）を追加
3. システム出力をこの Multi-Output Device に設定

> 注: `Built-in Output` が表示されない機種（Mac mini 等）では接続中の出力デバイスをクロックソースに指定する。
> BlackHole はクロック源にしない（音ズレ・動画再生失敗の原因になる）。

**実機確認**: この構成でスピーカーから音が聞こえる状態で BlackHole 16ch 経由の録音に成功した。

### ミキシング検証

QuickTime Player（`ファイル → 新規オーディオ録音` → 入力を BlackHole 16ch に選択）で録音。

1. Spotify 再生中に QuickTime Player で 10 秒録音
2. 内蔵スピーカから音が聞こえる（耳での確認）
3. 録音ファイルを再生し、Spotify の再生内容と一致することを確認（耳での確認）

**結果**: ✅ スピーカー出力と BlackHole 経由録音が同時に成立した。N 本の virtual sink を OS が並列に扱えることを実証。

### Knob からの enumerate（将来実装の見通し）

現時点でコード変更はしない。将来 `coreaudio-sys` または `cidre` クレートで `kAudioHardwarePropertyDevices` を列挙すれば BlackHole デバイスが確実に含まれる（Audio MIDI 設定で見えている = CoreAudio API からも見える）。実装は別 Issue で対応。

---

## 自作 AudioServerPlugin PoC

### 背景

「Knob のアプリバンドルに含められないか」「実装難易度はどの程度か」「署名要件は机上のとおりか」を最小コストで判定するためのタイムボックス調査（上限 3〜5h）。

### Apple サンプルの参照

Apple 公式：[Creating an Audio Server Driver Plug-in](https://developer.apple.com/documentation/coreaudio/creating-an-audio-server-driver-plug-in)

オープンソース実装参考：
- [libASPL](https://github.com/gavv/libASPL) — C++17 ライブラリ、AudioServerPlugIn の抽象化層
- [BackgroundMusic](https://github.com/kyleneideck/BackgroundMusic) — 完全実装例
- [Pancake](https://github.com/0bmxa/Pancake) — Swift ラッパー

### PoC 実施結果（Issue #10 で実機検証済）

#### 環境

- Xcode 26.4.1 / Build 17E202 / macOS 26 / Apple Silicon (Mac mini)
- サンプル: Apple 公式 `CreatingAnAudioServerDriverPlugIn` (NullAudio.xcodeproj)
- SIP: **有効** (`csrutil status` = enabled)

#### 1. ビルド — ✅ 成功

```bash
xcodebuild -project NullAudio.xcodeproj -configuration Debug \
  CODE_SIGN_IDENTITY="" CODE_SIGNING_REQUIRED=NO
# → BUILD SUCCEEDED
```

出力: `build/Debug/NullAudio.driver`（Mach-O universal binary: arm64 + x86_64）

Xcode のリンカーが自動的に **ad-hoc 署名**（`flags=0x20002(adhoc,linker-signed)`、`TeamIdentifier=not set`）を付与。

#### 2. `/Library/Audio/Plug-Ins/HAL/` へのロード — ✅ 成功（ad-hoc 署名）

```bash
sudo cp -R build/Debug/NullAudio.driver /Library/Audio/Plug-Ins/HAL/
sudo killall -9 coreaudiod
system_profiler SPAudioDataType
# → "Null Audio Device: Input Channels: 2 / Output Channels: 2 / 44100Hz"
```

ad-hoc 署名のまま `coreaudiod` がロードし、OS のデバイス一覧に列挙された。

#### 3. 署名要件の実機検証

| 署名状態 | ロード結果 | 備考 |
|---|---|---|
| Xcode 自動（ad-hoc linker-signed） | **✅ ロード成功** | `Signature=adhoc`、TeamID なし |
| 署名完全除去（`codesign --remove-signature`） | **✅ ロード成功** | SIP 有効でも coreaudiod は拒否しない |
| Developer ID 署名 | 未検証（証明書未取得） | — |

**重要な気づき**: `coreaudiod` 自体は署名を要求しない。署名（Developer ID + Notarization）が必要になるのは**配布時の Gatekeeper**（ダウンロードファイルの quarantine 属性チェック）の段階。

- `sudo cp` による直接コピーは quarantine をバイパスするため、ローカルなら署名なしでも動く
- 配布する `.pkg` インストーラ経由なら、pkg 自体を Developer ID Installer で署名 + 公証すれば内部の `.driver` に quarantine が付かず、署名なしでもロード可能（ただし Apple のガイドラインは `.driver` 自体の署名を推奨）

#### 4. `.app` 内同梱 — ❌ 認識されない

```bash
mkdir -p /tmp/Knob.app/Contents/PlugIns/HAL
cp -R NullAudio.driver /tmp/Knob.app/Contents/PlugIns/HAL/
# /Library/Audio/Plug-Ins/HAL/ の NullAudio.driver を除去後:
sudo killall -9 coreaudiod
system_profiler SPAudioDataType | grep -i null  # → 出力なし
```

`coreaudiod` は `/Library/Audio/Plug-Ins/HAL/` および `~/Library/Audio/Plug-Ins/HAL/` のみを走査する。`.app/Contents/PlugIns/` は対象外であり、**`.app` 同梱で coreaudiod に直接読み込ませることは不可能**。

#### 結論（v2 実現可能性の更新）

| 論点 | Issue #4 時点の机上見積 | 実機検証後の結論 |
|---|---|---|
| ビルド難易度 | 不明 | `xcodebuild` 1 コマンドで完結 |
| coreaudiod 署名要件 | "ad-hoc では拒否される" と想定 | **署名不要**（coreaudiod は署名チェックしない） |
| 配布時の署名要件 | Developer ID + Notarization 必須 | **変わらず必須**（Gatekeeper が quarantine でブロック） |
| `.app` 内同梱 | 条件付き可能と想定 | **不可能**（`coreaudiod` のスキャン対象外） |
| pkg インストーラ | 必要と想定 | **必要**（`/Library/Audio/Plug-Ins/HAL/` への配置に管理者権限必要） |

**v2 のハードルは当初想定より低い**: coreaudiod 自体はシンプルで、`.driver` のビルドも容易。Knob v2 の工数の大半は配布インフラ（Developer ID 取得・`.pkg` 作成・Notarization CI）と HAL plugin 本実装（CoreAudio HAL API、C++ / Swift）。

---

## 推奨アプローチ

### v1 — BlackHole 依存（Knob 0.x、推奨）

```
Knob 起動時に CoreAudio HAL から BlackHole デバイスを列挙。
未検出の場合、ガイド画面に以下を表示:
  brew install blackhole-16ch
  または: https://existential.audio/blackhole/

BlackHole 16ch の ch 1-2 = アプリ A、ch 3-4 = アプリ B、… と割り当てる
routing 層を Issue #5 / #6 で実装。
```

**採用理由**: 実装コストが最小、GPL でライセンス問題がなく、ユーザ側のインストール手順も `brew` 1 コマンドで完結。Knob 側に署名インフラ不要。

**制約**:
- ユーザが 1 回だけ `brew install` する必要がある（UX の摩擦）
- チャンネル数が 16ch 固定（16 アプリが上限）
- アプリごとのデバイス命名（"Spotify", "Chrome" 等）はできない（ch 番号管理になる）

### v2 — 自作 AudioServerPlugin（Knob 1.x、defer）

```
自作 .driver を Developer ID 署名 + Notarization した .pkg として同梱。
初回起動時に管理者認証ダイアログ付きでインストール。
動的にアプリ数に合わせてチャンネル / デバイス数を変更可能。
```

**着手条件**:
- Apple Developer Program 加入（年 99 USD）— Developer ID Installer（pkg 署名用）と Developer ID Application（driver 署名用）の両証明書が必要
- Notarization パイプライン（CI に `xcrun notarytool submit` 組み込み）
- C++ / Swift で HAL plugin の実装（参考: `libASPL` / `BackgroundMusic`）
- `pkg` インストーラの整備（`/Library/Audio/Plug-Ins/HAL/` への配置に管理者権限が必要なため）

> **Issue #10 の実機検証で判明**: `coreaudiod` 自体は署名を要求しない。ビルドも `xcodebuild` 1 コマンドで完結。v2 の工数の大半は配布インフラ整備（pkg + Notarization CI）と HAL plugin 本実装であり、当初想定よりハードルは低い。

**着手判断**: v1 のリリース後、UX フィードバック（チャンネル名・デバイス命名への不満）が蓄積したタイミングで検討。

### 却下した選択肢

| 候補 | 却下理由 |
|---|---|
| **Loopback** | エンドユーザに $99 要求。Knob が前提にできない。ARK-SDK も商用ライセンス要 |
| **Soundflower / kext** | Apple Silicon / SIP 非対応、メンテナンス停止 |

---

## 既知の制限・次 Issue へのリンク

| 項目 | 対応 Issue |
|---|---|
| Knob から CoreAudio device 列挙（BlackHole 検出）| Issue #5（新規） |
| ScreenCaptureKit からの PCM を BlackHole の ch に書き戻す output 側実装 | Issue #6（新規） |
| アプリ → ch ペアのルーティングテーブル UI | Issue #7（新規） |
| AudioServerPlugin 本実装 + pkg 配布 + Notarization パイプライン | Issue #8（新規、v2 マイルストーン） |
| BlackHole 未検出時のインストールガイド画面 | Issue #5 に含める |

---

## 参考リンク

- [ExistentialAudio/BlackHole — GitHub](https://github.com/ExistentialAudio/BlackHole)
- [BlackHole Wiki: Multi-Output Device](https://github.com/ExistentialAudio/BlackHole/wiki/Multi-Output-Device)
- [Apple Developer: Creating an Audio Server Driver Plug-in](https://developer.apple.com/documentation/coreaudio/creating-an-audio-server-driver-plug-in)
- [Apple Developer: Building Audio Server Plug-in and Driver Extension](https://developer.apple.com/documentation/coreaudio/building-an-audio-server-plug-in-and-driver-extension)
- [Apple WWDC 2021: Create audio drivers with DriverKit](https://developer.apple.com/videos/play/wwdc2021/10190/)
- [gavv/libASPL — C++17 AudioServerPlugIn library](https://github.com/gavv/libASPL)
- [kyleneideck/BackgroundMusic — AudioServerPlugin 実装例](https://github.com/kyleneideck/BackgroundMusic)
- [Rogue Amoeba: ARK SDK / ACE Legacy](https://www.rogueamoeba.com/support/knowledgebase/?showArticle=ACE-Legacy)
- [Apple Developer Forums: AudioServerPlugIn Notarization](https://developer.apple.com/forums/thread/116003)

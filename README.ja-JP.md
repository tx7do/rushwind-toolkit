<div align="center">

# rushwind-toolkit

[中文](./README.md) | [English](./README.en-US.md) | **日本語**

</div>

---

RushWind エコシステムのツールキット：[`rush` CLI](#コマンド) と
[`rush-gen` コアライブラリ](#リポジトリ構成)。

[RushWind](https://github.com/tx7do/rushwind)（Rust 製マイクロサービス
フレームワーク）と
[rushwind-admin](https://github.com/tx7do/rushwind-admin)（契約駆動の管理
画面スキャフォールド）の開発チェーンに実際に存在するギャップを埋めます。

位置づけ：RushWind 側に対になるものが存在しない部分だけを担い、万能箱には
しません。契約とルートの生成はすでに `build.rs` の rushwind-gen-http が
担っており（proto を入力すると、ルートテーブル・エラーマッピング・サービ
ストレイトが出力される。手書きルートはゼロ）、watch とクロスコンパイルは
cargo エコシステムに任せ、フロントエンドページの生成は本リポジトリの範囲外
です。

## インストール

```shell
cargo install --git https://github.com/tx7do/rushwind-toolkit rush-cli
```

## コマンド

| コマンド | 状態 | 説明 |
|---|---|---|
| `rush adopt` | ✅ | rushwind-admin スナップショットを「上流ミラー」から下流所有リポジトリへ引き取る |
| `rush manifest` | ✅ | proto / react 同期面の sha256 マニフェストを検証・再構築 |
| `rush gen entity` | ✅ | ドメインエンティティのバックエンド一式を生成 |
| `rush new` | ✅ | `cargo run` ですぐ動く新規プロジェクトを生成 |
| `rush testbed` | ✅ | 差分ベンチのオーケストレーションとレポート要約（docker は起動しない） |

### `rush adopt` —— 下流の引き取り（二次開発の最初の一歩）

背景：rushwind-admin の proto 契約と react フロントエンドは go-wind-admin
から同期されたミラーであり、sha256 マニフェスト + CI ゲート（「手修正禁止」
が意味論）で守られています。これは保守者には正しくても、下流の二次開発者に
とっては逆方向です——自分の proto を追加したりフロントエンドページを編集す
ると必ず CI が落ち、マニフェストを再構築できる `sync` モードは上流チェック
アウト（Windows 側の `D:\GoProject\go-wind-admin`）を要求するからです。

`rush adopt` はリポジトリをミラーモードから所有モードへ切り替えます：

1. **現在のツリー**を基準に `backend/api/MANIFEST.sha256` と
   `frontend/admin/react.MANIFEST.sha256` を再構築；
2. `.github/workflows/ci.yml` から 2 つの同期ゲートステップ（直前のコメン
   トブロックごと）を剥離；
3. 上流ドリフト基準 `react.UPSTREAM.sha256` はデフォルトで保持してお知らせ
   し、`--prune-upstream-baseline` で削除。

冪等で再実行可能。`--dry-run` は何も書き込まず全工程をプレビュー。
`--keep-gates` はマニフェストのみ再構築し CI はそのまま（上流を追い続けた
い保守者向けモード）。

```shell
rush adopt --dry-run   # プレビュー
rush adopt             # 引き取り
```

### `rush manifest` —— マニフェスト保守

引き取り後に proto やフロントエンドを追加・変更したら、基準を再構築して
CI をグリーンのままにします：

```shell
rush manifest proto --rebuild   # backend/api/protos 面
rush manifest react --rebuild   # frontend/admin/react 面
rush manifest proto             # デフォルトは検証のみ（読み取り専用）
```

マニフェストのアルゴリズムは、置き換えたシェルスクリプトと逐語的に一致させ
ています：proto 面はハッシュ前に UTF-8 BOM を剥離。react 面は
`node_modules`/`dist` ディレクトリと git 無視パス（`git check-ignore` と同
じ意味論）を除外し、バイト単位でハッシュ。git ワークツリー内での実行を要求
します。検証は内容のマップ比較のため行順に影響されず、引き取り前の旧スクリ
プト生成マニフェストに対しても正確に機能します。

### `rush gen entity` —— ドメインエンティティのバックエンド一式生成

標準的な CRUD エンティティのバックエンド一式を rushwind-admin 向けに生成し
ます（テンプレート基準：リポジトリ内で最小の dict_type エンティティチェー
ン）。コマンド一発でコンパイルの通るスケルトンが手に入ります：

```shell
rush gen entity widget \
  --field code:string --field quantity:u32 --field is_active:bool
```

生成物：

- メッセージ面 proto：`backend/api/protos/widget/service/v1/widget.proto`
- admin HTTP アノテーション面 proto：
  `backend/api/protos/admin/service/v1/i_widget.proto`
  （List/Get/Create/Update/Delete の 5 ルート。プレフィックスのデフォルト
  は `/admin/v1/widgets`）
- SeaORM エンティティ：`backend/services/admin-api/src/data/sys_widgets.rs`
- repo（`repo_shell!` マクロ + テナントスコープの一括削除）：
  `backend/services/admin-api/src/data/repos/widget.rs`
- service 実装（rushwind-gen-http が生成する Handlers トレイトに整合）：
  `backend/services/admin-api/src/services/widget.rs`
- 5 箇所の登録（冪等な行レベル挿入）：`data.rs` / `migration.rs` /
  `data/repos/mod.rs` / `services.rs` / `server/rest.rs`（use インポート
  ブロック + マウントテーブル）
- proto MANIFEST を自動再構築（`--skip-manifest` でスキップ）

フィールド型は `string|i32|u32|bool|f64`。標準カラム（id / tenant_id /
sort_order / 監査者とタイムスタンプ）は自動で付きます。生成後の
`cargo check` はそのままグリーン（実際の rushwind-admin で gizmo エンティ
ティにより検証済み）。続けて `cargo fmt` を推奨。

`--dry-run` は書き込まずに全アクションをプレビュー。既存 DB への注意や
seed / フロントエンド / testbed など後続の手動ステップはコマンド出力の注記
に表示されます。

### `rush new` —— 新規プロジェクトのスキャフォールド

```shell
rush new my-server && cd my-server && cargo run
```

内蔵テンプレートは自己完結しています：rushwind の git 依存は
rushwind-admin と同じ rev にピン留めされ、1 枚の YAML ドキュメントでメモリ
ストレージエンジン、自動 CRUD エッジ（`/items`）、HTTP サーバーを組み立て
ます。`cargo run` の後に表示される案内へ従って `curl /health`、`/wired`、
`/items` を叩けば一式を体験できます。`git init` はデフォルトで実行
（`--no-git` でスキップ）。`--dir` で親ディレクトリを指定します。

`--template <dir>` で任意の外部テンプレート（rushwind リポジトリの
`examples/bootstrap-demo` など）に差し替え可能：ツリーごとコピーし
（`.git`/`target` は除外）、テンプレートの Cargo.toml のパッケージ名を推測
してトークン置換（アンダースコア変形も含む）。テキスト以外のファイルはバイ
ト単位でそのままコピーします。

### `rush testbed` —— 差分ベンチのオーケストレーションとレポート要約

`backend/testbed/README` の 3 ステップ実行手順をコマンドに凝縮します。
**コンテナ規律**：Go 参照側は docker 差分スタックで動いており、rush は決し
て起動しません。Go エンドポイントに到達できない場合は手動手順
（`cd backend/testbed && docker compose up -d --build`）とともに即座にエ
ラーになります。

```shell
rush testbed run     # ビルド → Rust 側（:7788）起動 → admin-diff で再生 → レポート要約
rush testbed report  # JSONL レポートをオフライン要約（既定 backend/testbed/reports/report.jsonl）
```

`run` のオーケストレーション：コーパス/豁免集/両クレートの存在確認 → Go エ
ンドポイントの事前確認（落ちていれば停止）→ Rust エンドポイントが落ちてい
れば `cargo build` して admin-api を起動し、準備完了を待機 → リポジトリの既
定引数（`--go/--rust/--corpus/--exemptions/--out`）でリプレイヤーを実行 →
起動したプロセスを回収（`--keep-server` で保持）→ リプレイヤーの終了コード
を透過し、レポートを要約。

`report` の要約：verdict のヒストグラム（Ok/Fail/Exempt/Pending/
Unreachable）、class×verdict マトリクス、豁免を超えた分歧（Fail）とその証
拠、到達不能ケースの一覧——JSONL を目で追う必要はもうありません。

## リポジトリ構成

```
rushwind-toolkit/
├── crates/
│   ├── rush-cli/     # `rush` 実行ファイルの入口（clap コマンド面）
│   └── rush-gen/     # コアライブラリ：マニフェスト2種 + adopt + エンティティチェーン + プロジェクトスキャフォールド
└── ...
```

コアライブラリと CLI は分離しています。将来デスクトップシェルを作る場合
（作るなら Tauri 志向）は CLI を経由せず `rush-gen` を直接再利用できます。

## ロードマップ

- [x] `rush adopt` / `rush manifest`
- [x] `rush gen entity`：ドメインエンティティのバックエンド一式生成（proto
      2 面 + SeaORM エンティティ + repo + Handlers トレイト実装 + 6 箇所の
      登録 + マニフェスト再構築）
- [x] `rush new`：新規プロジェクトのスキャフォールド（自己完結テンプレー
      ト + 外部テンプレートのコピーとパッケージ名リネーム）
- [x] `rush testbed`：差分ベンチのオーケストレーション（ビルド/起動/再生/
      回収、コンテナ規律：docker には触れない）+ JSONL レポートのオフライ
      ン要約
- [ ] フロントエンドページ生成：Vben / Element / React の 3 スタック向け
      ページスキャフォールド（Rust 側での再実装ではなく、既存のフロントエ
      ンド生成チェーンとの連携を優先）

## 関連リポジトリ

- [rushwind](https://github.com/tx7do/rushwind) — Rust 製マイクロサービスフレームワーク本体
- [rushwind-admin](https://github.com/tx7do/rushwind-admin) — 契約駆動の管理画面スキャフォールド

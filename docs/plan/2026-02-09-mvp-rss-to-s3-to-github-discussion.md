## Plan: MVP 実装計画（RSS→S3→Discussion）

README と docs/architecture.md を起点に、このリポジトリはまだ骨組み段階（実コード/スタック未配置）なので、まず「最小の縦切り（1 feed→S3→Discussion）」を動かし、その後に複数 feed・部分成功・フィードバック収集（Function URL）へ拡張する順で進めます。コスト最小・再実行可能性（S3 に原本/正規化を保存）・冪等性（S3 HeadObject）・設定は SSM/Secrets を前提にします。

### MVP の定義（縦切り）
- 入力: SSM から取得する feed URL（まずは 1 件）
- 出力: S3 に raw RSS / normalized items / digest artifacts を保存し、GitHub Discussion を 1 件作成する
- 実行: EventBridge Scheduler → Step Functions → 複数 Lambda（Rust）

### 受け入れ条件（Definition of Done）
1. 1 feed を取り込める（RSS2.0 または Atom のどちらかは最低限）
2. raw RSS が S3 に保存される（再実行・調査用）
3. 正規化 item がスキーマ v1 で S3 に保存される（`item_id` は決定的）
4. digest（JSON + Markdown）が S3 に保存される
5. GitHub Discussion が安定フォーマットで投稿され、@mention とフォームリンクを含む
6. 冪等: 同じ run を再実行しても items/digest/discussion が重複作成されない（S3 HeadObject でスキップ）
7. 1 feed の失敗が StateMachine 全体を失敗させない設計に拡張可能（MVPでは単一 feed でも例外設計は仕込む）

### Steps
1. README.md と docs/architecture.md を要件化し、MVP の I/O（S3 keys / 正規化スキーマv1 / Discussion フォーマット）を確定する
2. CDK(TypeScript) の app/stack 構成を作り、S3/SSM/Secrets/Step Functions/Lambda を最小権限で定義する
3. Rust Lambda: `fetch-sources`（HTTP fetch + retry）→ raw を S3 に保存
4. Rust Lambda: `parse-normalize`（S3 から raw を読んで RSS/Atom パース）→ 正規化(item_id 生成)→ items を S3 に保存（HeadObject で重複スキップ）
5. Rust Lambda: `build-digest`（items 集約/フィルタ/順序）→ digest.json + discussion.md を生成して S3 に保存
6. Rust Lambda: `post-discussion`（GitHub GraphQL）で Discussion 作成（安定フォーマット、@mention、フォームリンクを含む）
7. Rust Lambda(Function URL): `form-handler` を実装（GET: HTML生成 / POST: HMAC+exp 検証→S3 保存）
8. Step Functions を「部分成功」前提でオーケストレーションし、EventBridge Scheduler で定期起動にする

### 決めること（先に決めると詰まらない）
- SSM パラメータ名と JSON 形式（例: feeds 配列、対象 repo、category_id、@mention、時間窓、フィルタ条件）
- Secrets の名前/キー（GitHub token、フォーム署名用 HMAC secret）
- `digest_id` の定義（例: `run_id` と同一の ISO 文字列）
- Discussion のカテゴリ（category_id の取得方法・運用手順）
- items の日付パーティション基準（推奨: fetched_at の日付で `items/v1/date=YYYY-MM-DD/`）

### 冪等性（MVPでの具体）
- raw: 常に保存（`raw/v1/.../ts=<iso>.xml`）
- items: `items/v1/date=.../<item_id>.json` を `HeadObject` で存在確認し、あればスキップ
- digest: `digest/v1/date=.../run=<run_id>/digest.json` 等を `HeadObject` で存在確認し、あればスキップ
- discussion: 投稿済みマーカーを S3 に保存して `HeadObject` でスキップ（例: `digest/v1/.../run=<run_id>/posted.json` に Discussion URL/ID を記録）

### 部分成功（拡張を見据えた前提）
- 将来の複数 feed は Step Functions の Map で並列化し、feed 単位で Catch して失敗を集約ログに残す
- build-digest は「成功した feed の items」だけで digest を作れる設計にする

### テスト観点（最小）
- 正規化: RSS/Atom の fixture を用意（published 欠落、リンク揺れ、エンティティ等）
- `item_id` の安定性: 同一入力→同一 ID、URL/title/date の変化→異なる ID
- 冪等スキップ: S3 既存想定の分岐（AWS 呼び出し部は薄く、ドメインロジックを純粋関数でテスト）

### Further Considerations
1. 初期スコープ: 1 feed 固定（SSM）で縦切り→複数 feed は Map state で拡張
2. GitHub 認証: まず PAT、後で GitHub App へ移行（Secrets の形を揃える）
3. 冪等性粒度: raw は常に保存、items/digest/discussion は HeadObject でスキップ（Discussion は S3 に投稿済みマーカーを残す）
4. セキュリティ: secrets をログに出さない、Function URL は HMAC+exp を必須にし、期限切れ/改竄は 4xx
5. コスト: API Gateway を避け Function URL 優先、S3 は lifecycle で raw の保持期間を調整（必要なら）

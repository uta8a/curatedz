# Copilot Instructions (repo-wide)

## Language
**Always respond in Japanese (日本語)** when interacting with developers in this repository.

## Project intent
This repository implements a **low-cost RSS → S3 archive → GitHub Discussion notification** pipeline on AWS.
- **Infrastructure**: CDK (TypeScript)
- **Lambda functions**: Rust (for performance and low cost)
- **Orchestration**: Step Functions (for retry and observability)
- **Output**: GitHub Discussion posts with feedback form links (not email)
Avoid expensive/complex services (Bedrock, Glue/Athena, OpenSearch, etc.) unless explicitly requested later.

## Architecture constraints
- Scheduled execution (EventBridge Scheduler → Step Functions).
- Step Functions orchestrates multiple Lambda functions (Rust).
- Persist raw RSS and normalized items to S3 for replay/debug.
- Idempotency / dedupe should be implemented using **S3 object keys + HeadObject**, not DynamoDB (unless explicitly requested).
- Configuration should come from **SSM Parameter Store** (JSON strings), and secrets (GitHub token) from **Secrets Manager**.
- Prefer partial success: one broken feed must not fail the whole run (use Step Functions error handling).
- Output is **GitHub Discussion** (GraphQL API), not email.
- Feedback collection via **Lambda Function URL** serving dynamic HTML forms, storing results in S3.

## Coding style
### Rust (Lambda functions)
- Keep domain logic pure where possible:
  - Parsing, normalization, filtering, aggregation should be testable without AWS calls.
  - AWS calls should be in small modules using aws-sdk-rust.
- Use strong typing and error handling (Result<T, E>).
- Log with structured fields (feed_url, item_id, stage) using tracing or similar.
- Do not silently swallow exceptions. Catch and record feed-level failures, continue other feeds.

### TypeScript (CDK)
- Use TypeScript strict mode.
- Separate stacks by concern (pipeline, lambda, storage).
- Use CDK constructs idiomatically.

## Data model
Normalized item schema (v1) must include:
- schema_version, source_feed_url, item_id, canonical_url, title
- published_at, fetched_at
- optional: authors, tags, summary/snippet

Item ID:
- Deterministic hash: `sha256(canonical_url_or_link + title + published_at_iso)`
- Prefix with `sha256:` for clarity.

## S3 layout guidelines
- raw RSS:
  - `raw/v1/date=YYYY-MM-DD/feed=<url-escaped>/ts=<iso>.xml`
- normalized items:
  - `items/v1/date=YYYY-MM-DD/<item_id>.json`
- digest outputs:
  - `digest/v1/date=YYYY-MM-DD/run=<iso>/digest.json`
  - `digest/v1/date=YYYY-MM-DD/run=<iso>/discussion.md`
- feedback data:
  - `feedback/v1/digest_id=<digest_id>/submitted_at=<iso>-<rand>.json`

## GitHub Discussion output
- Post digest as a new Discussion in private repository.
- Include @mention to notify user.
- Provide:
  - Form link with signed token for feedback collection
  - Top section: selected items (title, link, short snippet)
  - Bottom section: remaining items (title + link only)
- Keep output stable (important for future evaluation tooling).

## Feedback form (Lambda Function URL)
- GET: dynamically generate HTML form showing digest items
- POST: validate and save user selections (like/unlike/non-selected) to S3
- Use HMAC-signed tokens with expiration for security
- No session state in Lambda; all state from URL params or S3

## Testing guidance
- Add fixtures for RSS variants:
  - RSS 2.0, Atom
  - Missing published date
  - Non-UTF8 / weird entities
- Unit tests should validate:
  - Normalization output
  - Idempotent skip logic (simulated)
  - Digest rendering

## Security / least privilege
- IAM policy should be minimal:
  - S3 PutObject/GetObject/HeadObject on the bucket prefixes
  - Secrets Manager GetSecretValue (for GitHub token)
  - SSM GetParameter(s)
- GitHub authentication:
  - Use GitHub App (preferred) or Personal Access Token
  - Required permissions: Discussions (write), Contents (read)
- Form handler token:
  - HMAC signature with expiration (digest_id + exp)
  - Validate on every request
- Never print secrets or tokens in logs (mask them).
